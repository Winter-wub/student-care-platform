//! Send and inbound robustness at the Deployment seam (#35): metered units
//! per Push request, partial sends recorded and resumable, keyed batches,
//! and webhook events handled once per event id.

use harpseal::{
    ActionContext, ActionId, Card, CardList, Error, Inbound, Locale, Message, Module,
    ModuleRegistrar, ReplyToken, Statement, Timestamp,
};
use harpseal_channel_fake::{FAILURE_RETRY_AFTER_SECS, FakeChannel, FakeProfile};
use harpseal_testing::{Harness, START_MS, TestApp};

/// Answers `echo.ping` with one text.
struct Echo;

impl Module for Echo {
    fn name(&self) -> &'static str {
        "echo"
    }
    fn register(&self, r: &mut ModuleRegistrar) {
        r.action(id("echo.ping"), |_: ActionContext| async move {
            Ok(vec![Message::text("พบกันที่มหาวิทยาลัยอ่าวแมวน้ำ")])
        });
    }
}

fn id(s: &str) -> ActionId {
    ActionId::new(s).unwrap()
}

/// `u1` presses `echo.ping`, with the platform event id `event`.
fn ping(event: Option<&str>, redelivery: bool) -> Inbound {
    Inbound::action(
        FakeChannel::account("u1"),
        id("echo.ping"),
        Some(ReplyToken {
            token: format!("r-{}", event.unwrap_or("none")),
            expires_at: Timestamp(START_MS + 60_000),
        }),
    )
    .with_event_id(event.map(str::to_string), redelivery)
}

/// A Deployment with the Echo Module and `u1` linked to a Student.
async fn linked(profile: FakeProfile) -> (Harness, TestApp) {
    let h = Harness::new().with_fake_profile(profile);
    let app = h.boot(h.builder().module(Echo).build().unwrap()).await;
    for sql in [
        "INSERT INTO student (id, student_code, display_name, locale, created_at, updated_at) \
         VALUES ('s1', '68010007', 'Nam Talay', 'th', 1, 1)",
        "INSERT INTO channel_account (id, channel, external_id, first_seen_at, updated_at) \
         VALUES ('a1', 'fake', 'u1', 1, 1)",
        "INSERT INTO link (id, student_id, channel_account_id, channel, status, method, linked_at, updated_at) \
         VALUES ('l1', 's1', 'a1', 'fake', 'active', 'email_otp', 1, 1)",
    ] {
        h.db().execute(Statement::new(sql)).await.unwrap();
    }
    (h, app)
}

fn texts(n: usize) -> Vec<Message> {
    (1..=n)
        .map(|i| Message::text(format!("ประกาศ {i}")))
        .collect()
}

fn cards(n: usize) -> Message {
    Message::CardList(CardList(
        (0..n)
            .map(|i| Card {
                title: format!("ข่าว {i}"),
                body: "คณะวิทยาศาสตร์ทางทะเล มหาวิทยาลัยอ่าวแมวน้ำ".into(),
                buttons: Vec::new(),
            })
            .collect(),
    ))
}

const OUTBOUND: &str = "SELECT count(*) FROM outbound_message";
const METERED: &str = "SELECT coalesce(sum(metered), 0) FROM usage_daily";
const FREE: &str = "SELECT coalesce(sum(free), 0) FROM usage_daily";

#[tokio::test]
async fn a_message_spread_over_two_push_requests_adds_two_metered_units() {
    let (h, app) = linked(FakeProfile::LineLike).await;
    let d = app.deployment();
    let to = FakeChannel::account("u1");
    // 6 000 chars: two text objects on LINE (5 000 each), one Push request.
    let short = Message::text("ก".repeat(6_000));
    let r = d.send(&to, None, &short, &Locale::th()).await.unwrap();
    assert_eq!(r.metered_units, 1, "two objects, one Push request");
    // 61 Cards: 6 carousels of at most 12, so Push requests of 5 and 1.
    let r = d.send(&to, None, &cards(61), &Locale::th()).await.unwrap();
    assert_eq!(r.metered_units, 2);
    let pushes = h.fake_channel().take_calls();
    assert_eq!(
        pushes.iter().map(|c| c.objects).collect::<Vec<_>>(),
        [2, 5, 1]
    );
    assert_eq!(app.count(METERED).await, 3, "1 + 2 units");
    assert_eq!(app.count(FREE).await, 0);
    let metered_rows = app
        .count("SELECT count(*) FROM outbound_message WHERE metering = 'metered'")
        .await;
    assert_eq!(metered_rows, 2, "one row per Message");
}

#[tokio::test]
async fn a_batch_failing_at_part_2_of_3_records_part_1_and_the_retry_sends_only_2_and_3() {
    let (h, app) = linked(FakeProfile::TelegramLike).await;
    let d = app.deployment();
    let to = FakeChannel::account("u1");
    let fake = h.fake_channel();
    let msgs = texts(3);
    fake.fail_next_batch_at(1);
    let got = d
        .send_batch_with_key(&to, None, &msgs, &Locale::th(), "job-7/u1")
        .await;
    let sent = match got {
        Err(Error::PartialSend {
            sent,
            total,
            source,
        }) => {
            assert_eq!((sent, total), (1, 3));
            assert!(matches!(
                *source,
                Error::RateLimited {
                    retry_after_secs: FAILURE_RETRY_AFTER_SECS
                }
            ));
            sent
        }
        other => panic!("{other:?}"),
    };
    assert_eq!(app.count(OUTBOUND).await, 1, "part 1 is recorded");
    assert_eq!(app.count(FREE).await, 1);
    let retry = d
        .send_batch_with_key(&to, None, &msgs[sent..], &Locale::th(), "job-7/u1")
        .await
        .unwrap();
    assert_eq!(retry.len(), 2);
    assert_eq!(
        format!("{:?}", fake.sent_to(&to)),
        format!("{msgs:?}"),
        "each part reached the Student exactly once, in order"
    );
    assert_eq!(app.count(OUTBOUND).await, 3);
    assert_eq!(fake.keys(), ["job-7/u1", "job-7/u1"]);
}

#[tokio::test]
async fn a_message_stopped_part_way_is_recorded_with_its_error() {
    let (h, app) = linked(FakeProfile::TelegramLike).await;
    let d = app.deployment();
    let to = FakeChannel::account("u1");
    h.fake_channel().fail_next_batch_at(2);
    let got = d.send_batch(&to, None, &[cards(4)], &Locale::th()).await;
    assert!(
        matches!(
            got,
            Err(Error::PartialSend {
                sent: 0,
                total: 1,
                ..
            })
        ),
        "{got:?}"
    );
    assert_eq!(h.fake_channel().sent_to(&to).len(), 2, "two Cards went out");
    let rows = app
        .query("SELECT kind, error, platform_message_id FROM outbound_message")
        .await;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows.get(0, "kind").unwrap().as_text(), Some("card_list"));
    let error = rows.get(0, "error").unwrap().as_text().unwrap();
    assert!(error.starts_with("partly sent: rate limited"), "{error}");
    assert!(
        rows.get(0, "platform_message_id")
            .unwrap()
            .as_text()
            .is_some()
    );
}

#[tokio::test]
async fn nothing_sent_is_the_channel_error_as_is() {
    let (h, app) = linked(FakeProfile::LineLike).await;
    h.fake_channel().fail_next_batch_at(0);
    let got = app
        .deployment()
        .send_batch(&FakeChannel::account("u1"), None, &texts(2), &Locale::th())
        .await;
    assert!(matches!(got, Err(Error::RateLimited { .. })), "{got:?}");
    assert_eq!(app.count(OUTBOUND).await, 0);
}

#[tokio::test]
async fn every_unkeyed_batch_gets_its_own_key() {
    let (h, app) = linked(FakeProfile::Full).await;
    let d = app.deployment();
    let to = FakeChannel::account("u1");
    for _ in 0..2 {
        d.send_batch(&to, None, &texts(1), &Locale::th())
            .await
            .unwrap();
    }
    let keys = h.fake_channel().keys();
    assert_eq!(keys.len(), 2);
    assert_ne!(keys[0], keys[1]);
}

#[tokio::test]
async fn the_same_webhook_event_delivered_twice_is_handled_once() {
    let (h, app) = linked(FakeProfile::LineLike).await;
    let fake = h.fake_channel();
    let to = FakeChannel::account("u1");
    assert_eq!(
        app.fake_webhook([ping(Some("evt-1"), false)]).await.status,
        200
    );
    assert_eq!(fake.sent_to(&to).len(), 1);
    // The platform retries the same event, flagged or not.
    for redelivery in [false, true] {
        let resp = app.fake_webhook([ping(Some("evt-1"), redelivery)]).await;
        assert_eq!(resp.status, 200, "a duplicate is still a 200");
    }
    assert_eq!(fake.sent_to(&to).len(), 1, "answered once");
    assert_eq!(app.count(OUTBOUND).await, 1);
    let report = app
        .deployment()
        .handle_webhook(
            "fake",
            fake.webhook(&[ping(Some("evt-1"), true), ping(Some("evt-2"), false)]),
        )
        .await
        .unwrap();
    assert_eq!((report.events, report.duplicates), (2, 1));
    assert!(report.failed.is_empty());
    assert_eq!(fake.sent_to(&to).len(), 2, "evt-2 is new");
}

#[tokio::test]
async fn events_without_an_id_are_always_handled() {
    let (h, app) = linked(FakeProfile::Full).await;
    app.fake_webhook([ping(None, false)]).await;
    app.fake_webhook([ping(None, false)]).await;
    let to = FakeChannel::account("u1");
    assert_eq!(h.fake_channel().sent_to(&to).len(), 2);
    assert_eq!(app.count("SELECT count(*) FROM inbound_event").await, 0);
}

#[tokio::test]
async fn the_same_event_id_on_another_channel_is_another_event() {
    let h = Harness::new();
    let tg = h.fake_channel().with_kind("fake2");
    let app = h
        .boot(
            h.builder()
                .channel(tg.clone())
                .module(Echo)
                .build()
                .unwrap(),
        )
        .await;
    app.fake_webhook([ping(Some("1001"), false)]).await;
    let mut other = ping(Some("1001"), false);
    if let Inbound::Action { from, .. } = &mut other {
        from.channel = harpseal::ChannelKind("fake2");
    }
    let report = app
        .deployment()
        .handle_webhook("fake2", tg.webhook(&[other]))
        .await
        .unwrap();
    assert_eq!(report.duplicates, 0);
    assert_eq!(app.count("SELECT count(*) FROM inbound_event").await, 2);
}

#[tokio::test]
async fn a_tick_forgets_event_ids_older_than_a_week_and_keeps_younger_ones() {
    let (h, app) = linked(FakeProfile::Full).await;
    app.fake_webhook([ping(Some("old"), false)]).await;
    h.clock().advance_ms(2 * 86_400_000);
    app.fake_webhook([ping(Some("young"), false)]).await;
    h.clock().advance_ms(6 * 86_400_000);
    // 8 and 6 days old: the webhook path never prunes.
    app.fake_webhook([ping(Some("new"), false)]).await;
    assert_eq!(app.count("SELECT count(*) FROM inbound_event").await, 3);
    let report = app
        .deployment()
        .tick(Timestamp(h.clock().now_ms()))
        .await
        .unwrap();
    assert_eq!(report.events_pruned, 1);
    let rows = app
        .query("SELECT event_id FROM inbound_event ORDER BY event_id")
        .await;
    let ids: Vec<&str> = (0..rows.len())
        .map(|i| rows.get(i, "event_id").unwrap().as_text().unwrap())
        .collect();
    assert_eq!(ids, ["new", "young"]);
}

const INBOUND_FAILED: &str = "SELECT count(*) FROM security_event WHERE kind = 'inbound.failed'";

#[tokio::test]
async fn a_handled_event_marks_its_claim_done() {
    let (_h, app) = linked(FakeProfile::Full).await;
    app.fake_webhook([ping(Some("evt-done"), false)]).await;
    let open = "SELECT count(*) FROM inbound_event WHERE done_at IS NULL";
    assert_eq!(app.count(open).await, 0);
    assert_eq!(app.count("SELECT count(*) FROM inbound_event").await, 1);
}

#[tokio::test]
async fn a_redelivery_of_a_claim_that_never_finished_is_logged_once_and_not_answered() {
    // #44: the process died between the claim and the reply, as a Worker
    // past its CPU limit would; the claim is left with no `done_at`.
    let (h, app) = linked(FakeProfile::Full).await;
    let to = FakeChannel::account("u1");
    h.db()
        .execute(
            Statement::new(
                "INSERT INTO inbound_event (channel, event_id, received_at) VALUES ('fake', ?, ?)",
            )
            .bind("evt-killed")
            .bind(h.clock().now_ms()),
        )
        .await
        .unwrap();
    let redeliver = || async {
        app.deployment()
            .handle_webhook(
                "fake",
                h.fake_channel().webhook(&[ping(Some("evt-killed"), true)]),
            )
            .await
            .unwrap()
    };
    // Within the lease the first delivery may still be running: a plain
    // duplicate.
    let report = redeliver().await;
    assert_eq!((report.duplicates, report.failed.len()), (1, 0));
    assert_eq!(app.count(INBOUND_FAILED).await, 0);
    // Past the lease it is abandoned: logged once, still not answered.
    h.clock().advance_ms(3 * 60_000);
    let report = redeliver().await;
    assert_eq!((report.duplicates, report.failed.len()), (1, 1));
    assert!(matches!(report.failed[0], ("action", Error::Conflict(_))));
    assert_eq!(app.count(INBOUND_FAILED).await, 1);
    let details = app
        .query(
            "SELECT channel_account_id, details FROM security_event WHERE kind = 'inbound.failed'",
        )
        .await;
    assert_eq!(
        details.get(0, "channel_account_id").unwrap().as_text(),
        Some("a1")
    );
    // A later redelivery finds the claim done: a plain duplicate again.
    h.clock().advance_ms(3 * 60_000);
    let report = redeliver().await;
    assert_eq!((report.duplicates, report.failed.len()), (1, 0));
    assert_eq!(app.count(INBOUND_FAILED).await, 1);
    assert!(h.fake_channel().sent_to(&to).is_empty(), "never answered");
}

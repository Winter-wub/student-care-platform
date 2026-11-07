//! Webhook claims whose handling never finished (#44, ADR-0009): marked
//! done after handling, and logged once as `inbound.lost` (never answered)
//! by a redelivery after the 60 s lease or else by the tick.

use harpseal::{
    ActionContext, ActionId, Error, Inbound, Message, Module, ModuleRegistrar, ReplyToken,
    Statement, Timestamp,
};
use harpseal_channel_fake::{FakeChannel, FakeProfile};
use harpseal_testing::{Harness, START_MS, TestApp};

/// Answers `echo.ping` with one text.
struct Echo;

impl Module for Echo {
    fn name(&self) -> &'static str {
        "echo"
    }
    fn register(&self, r: &mut ModuleRegistrar) {
        r.action(
            ActionId::new("echo.ping").unwrap(),
            |_: ActionContext| async move {
                Ok(vec![Message::text("พบกันที่มหาวิทยาลัยอ่าวแมวน้ำ")])
            },
        );
    }
}

/// `u1` presses `echo.ping`, with the platform event id `event`.
fn ping(event: &str) -> Inbound {
    Inbound::action(
        FakeChannel::account("u1"),
        ActionId::new("echo.ping").unwrap(),
        Some(ReplyToken {
            token: format!("r-{event}"),
            expires_at: Timestamp(START_MS + 600_000),
        }),
    )
    .with_event_id(Some(event.into()), false)
    .with_redelivery(true)
}

/// A Deployment with the Echo Module and `u1` linked to a Student.
async fn linked() -> (Harness, TestApp) {
    let h = Harness::new().with_fake_profile(FakeProfile::Full);
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

/// A claim left as a killed handler leaves it: taken now, never done.
async fn killed_claim(h: &Harness, event: &str) {
    h.db()
        .execute(
            Statement::new(
                "INSERT INTO inbound_event (channel, event_id, received_at, external_id, kind) \
                 VALUES ('fake', ?, ?, 'u1', 'action')",
            )
            .bind(event)
            .bind(h.clock().now_ms()),
        )
        .await
        .unwrap();
}

const LOST: &str = "SELECT count(*) FROM security_event WHERE kind = 'inbound.lost'";

#[tokio::test]
async fn inbound_a_handled_event_marks_its_claim_done() {
    let (_h, app) = linked().await;
    app.fake_webhook([ping("evt-done")]).await;
    let open = "SELECT count(*) FROM inbound_event WHERE done_at IS NULL";
    assert_eq!(app.count(open).await, 0);
    assert_eq!(app.count("SELECT count(*) FROM inbound_event").await, 1);
}

#[tokio::test]
async fn inbound_a_redelivery_of_a_claim_unfinished_after_60_s_is_logged_once_and_not_answered() {
    let (h, app) = linked().await;
    killed_claim(&h, "evt-killed").await;
    let redeliver = || async {
        let fake = h.fake_channel();
        let call = fake.webhook(&[ping("evt-killed")]);
        app.deployment().handle_webhook("fake", call).await.unwrap()
    };
    // Within the 60 s lease the first delivery may still be running: a
    // plain duplicate, no Security Event.
    h.clock().advance_ms(59_000);
    let report = redeliver().await;
    assert_eq!((report.duplicates, report.failed.len()), (1, 0));
    assert_eq!(app.count(LOST).await, 0);
    // 61 s after the claim it is abandoned: logged once, not answered,
    // counted in `failed` only.
    h.clock().advance_ms(2_000);
    let report = redeliver().await;
    assert_eq!((report.duplicates, report.failed.len()), (0, 1));
    assert!(matches!(report.failed[0], ("action", Error::Conflict(_))));
    assert_eq!(app.count(LOST).await, 1);
    let row = app
        .query(
            "SELECT channel_account_id, json_extract(details, '$.event_id') AS event_id, \
             json_extract(details, '$.external_id') AS external_id \
             FROM security_event WHERE kind = 'inbound.lost'",
        )
        .await;
    let text = |c: &str| row.get(0, c).unwrap().as_text().map(str::to_string);
    assert_eq!(text("channel_account_id").as_deref(), Some("a1"));
    assert_eq!(text("event_id").as_deref(), Some("evt-killed"));
    assert_eq!(text("external_id").as_deref(), Some("u1"));
    // A later redelivery, and the tick, find the claim done.
    h.clock().advance_ms(61_000);
    let report = redeliver().await;
    assert_eq!((report.duplicates, report.failed.len()), (1, 0));
    let tick = app.deployment().tick(Timestamp(h.clock().now_ms())).await;
    assert_eq!(tick.unwrap().events_lost, 0);
    assert_eq!(app.count(LOST).await, 1);
    let to = FakeChannel::account("u1");
    assert!(h.fake_channel().sent_to(&to).is_empty(), "never answered");
}

#[tokio::test]
async fn inbound_the_tick_logs_a_claim_no_redelivery_came_for_exactly_once() {
    let (h, app) = linked().await;
    killed_claim(&h, "evt-silent").await;
    let tick = || async {
        let now = Timestamp(h.clock().now_ms());
        app.deployment().tick(now).await.unwrap()
    };
    // Inside the lease the tick leaves it alone.
    h.clock().advance_ms(59_000);
    assert_eq!(tick().await.events_lost, 0);
    assert_eq!(app.count(LOST).await, 0);
    h.clock().advance_ms(2_000);
    assert_eq!(tick().await.events_lost, 1);
    assert_eq!(app.count(LOST).await, 1);
    let kind = app
        .query("SELECT json_extract(details, '$.event') AS event FROM security_event")
        .await;
    assert_eq!(kind.get(0, "event").unwrap().as_text(), Some("action"));
    // A second tick adds none; a late redelivery is a plain duplicate.
    assert_eq!(tick().await.events_lost, 0);
    let fake = h.fake_channel();
    let call = fake.webhook(&[ping("evt-silent")]);
    let report = app.deployment().handle_webhook("fake", call).await.unwrap();
    assert_eq!((report.duplicates, report.failed.len()), (1, 0));
    assert_eq!(app.count(LOST).await, 1);
    assert!(fake.sent_to(&FakeChannel::account("u1")).is_empty());
}

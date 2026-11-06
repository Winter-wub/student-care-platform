//! The Kernel send path at the Deployment seam: one batch per reply token,
//! Outbound Message rows and `usage_daily` written atomically, Capability
//! checks before sending, and one failing event not blocking the next.

use harpseal::{
    ActionContext, ActionId, Attachment, Channel, Delivery, Error, Inbound, Locale, Message,
    Metering, Module, ModuleRegistrar, ReplyToken, Statement, Timestamp,
};
use harpseal_channel_fake::{FakeChannel, FakeProfile};
use harpseal_testing::{Harness, START_MS, TestApp};

/// Answers `bulk.seven` with 7 texts, `bulk.fail` with an error and
/// `bulk.file` with an attachment.
struct Bulk;

impl Module for Bulk {
    fn name(&self) -> &'static str {
        "bulk"
    }
    fn register(&self, r: &mut ModuleRegistrar) {
        r.action(id("bulk.seven"), |_: ActionContext| async move {
            Ok((1..=7).map(|i| Message::text(format!("ข่าว {i}"))).collect())
        })
        .action(id("bulk.fail"), |_: ActionContext| async move {
            Err(Error::Platform("the Seal Bay registrar is down".into()))
        })
        .action(id("bulk.file"), |_: ActionContext| async move {
            Ok(vec![Message::Attachment(timetable())])
        });
    }
}

fn id(s: &str) -> ActionId {
    ActionId::new(s).unwrap()
}

fn timetable() -> Attachment {
    Attachment {
        file_key: "files/timetable.pdf".into(),
        file_name: "ตารางเรียน.pdf".into(),
        content_type: "application/pdf".into(),
        size_bytes: 2048,
        url: "https://files.seal-bay.invalid/timetable.pdf".into(),
    }
}

fn token(expires_at: i64) -> Option<ReplyToken> {
    Some(ReplyToken {
        token: "r-1".into(),
        expires_at: Timestamp(expires_at),
    })
}

fn press(account: &str, action: &str) -> Inbound {
    Inbound::Action {
        from: FakeChannel::account(account),
        action: id(action),
        reply: token(START_MS + 60_000),
        event_id: None,
        redelivery: false,
    }
}

/// A Deployment with the Bulk Module and `u1` linked to a Student.
async fn linked(profile: FakeProfile) -> (Harness, TestApp) {
    let h = Harness::new().with_fake_profile(profile);
    let app = h.boot(h.builder().module(Bulk).build().unwrap()).await;
    for sql in [
        "INSERT INTO student (id, student_code, display_name, locale, created_at, updated_at) \
         VALUES ('s1', '68010004', 'Karn Bai-ruea', 'th', 1, 1)",
        "INSERT INTO channel_account (id, channel, external_id, first_seen_at, updated_at) \
         VALUES ('a1', 'fake', 'u1', 1, 1)",
        "INSERT INTO link (id, student_id, channel_account_id, channel, status, method, linked_at, updated_at) \
         VALUES ('l1', 's1', 'a1', 'fake', 'active', 'email_otp', 1, 1)",
    ] {
        h.db().execute(Statement::new(sql)).await.unwrap();
    }
    (h, app)
}

const OUTBOUND: &str = "SELECT count(*) FROM outbound_message";
const METERED: &str = "SELECT coalesce(sum(metered), 0) FROM usage_daily";
const FREE: &str = "SELECT coalesce(sum(free), 0) FROM usage_daily";

#[tokio::test]
async fn a_reply_is_free_and_a_push_is_metered_on_a_line_like_channel() {
    let (h, app) = linked(FakeProfile::LineLike).await;
    let d = app.deployment();
    let to = FakeChannel::account("u1");
    let msg = Message::text("ยินดีต้อนรับสู่มหาวิทยาลัยอ่าวแมวน้ำ");
    let reply = d
        .send(&to, token(START_MS + 1).as_ref(), &msg, &Locale::th())
        .await;
    assert_eq!(reply.unwrap().metering, Metering::Free);
    let push = d.send(&to, None, &msg, &Locale::th()).await.unwrap();
    assert_eq!(push.metering, Metering::Metered);
    let seen = h.fake_channel().seen_by(&to);
    assert!(matches!(seen[0].delivery, Delivery::Reply(_)));
    assert!(matches!(seen[1].delivery, Delivery::Push));
    let rows = app
        .query("SELECT push, metering, kind, module FROM outbound_message ORDER BY sent_at, push")
        .await;
    assert_eq!(rows.len(), 2);
    assert_eq!(rows.get(0, "metering").unwrap().as_text(), Some("free"));
    assert_eq!(rows.get(1, "metering").unwrap().as_text(), Some("metered"));
    assert_eq!(rows.get(1, "push").unwrap().as_i64(), Some(1));
    assert_eq!(rows.get(0, "kind").unwrap().as_text(), Some("text"));
    let usage = app
        .query("SELECT day, metered, free FROM usage_daily")
        .await;
    assert_eq!(usage.len(), 1, "one row per (channel, day)");
    assert_eq!(usage.get(0, "day").unwrap().as_text(), Some("2026-11-05"));
    assert_eq!(usage.get(0, "metered").unwrap().as_i64(), Some(1));
    assert_eq!(usage.get(0, "free").unwrap().as_i64(), Some(1));
}

#[tokio::test]
async fn usage_rolls_back_when_the_outbound_insert_fails() {
    let (h, app) = linked(FakeProfile::LineLike).await;
    let d = app.deployment();
    let to = FakeChannel::account("u1");
    let msg = Message::text("แจ้งเตือน");
    d.send(&to, None, &msg, &Locale::th()).await.unwrap();
    assert_eq!(
        (app.count(OUTBOUND).await, app.count(METERED).await),
        (1, 1)
    );

    let db = h.db();
    db.execute(Statement::new(
        "CREATE TRIGGER fail_outbound BEFORE INSERT ON outbound_message \
         BEGIN SELECT RAISE(ABORT, 'forced failure'); END",
    ))
    .await
    .unwrap();
    let failed = d.send(&to, None, &msg, &Locale::th()).await;
    assert!(matches!(failed, Err(Error::Db(_))), "{failed:?}");
    assert_eq!(
        (app.count(OUTBOUND).await, app.count(METERED).await),
        (1, 1),
        "the usage upsert in the same batch as the failing insert was rolled back"
    );

    db.execute(Statement::new("DROP TRIGGER fail_outbound"))
        .await
        .unwrap();
    d.send(&to, None, &msg, &Locale::th()).await.unwrap();
    assert_eq!(
        (app.count(OUTBOUND).await, app.count(METERED).await),
        (2, 2)
    );
}

#[tokio::test]
async fn a_send_needing_a_missing_capability_is_unsupported_and_writes_nothing() {
    let (h, app) = linked(FakeProfile::LineLike).await;
    let to = FakeChannel::account("u1");
    let msgs = [Message::text("ก่อนไฟล์"), Message::Attachment(timetable())];
    let r = app
        .deployment()
        .send_batch(&to, None, &msgs, &Locale::th())
        .await;
    assert!(matches!(
        r,
        Err(Error::Unsupported(harpseal::Capability::SendAttachment))
    ));
    // Through a Module handler too: the event fails, nothing goes out.
    assert_eq!(
        app.fake_webhook([press("u1", "bulk.file")]).await.status,
        200
    );
    assert!(h.fake_channel().sent().is_empty(), "nothing was sent");
    assert_eq!(app.count(OUTBOUND).await, 0);
    assert_eq!(app.count("SELECT count(*) FROM usage_daily").await, 0);
}

#[tokio::test]
async fn seven_messages_go_as_five_replies_and_two_metered_pushes() {
    let (h, app) = linked(FakeProfile::LineLike).await;
    assert_eq!(
        app.fake_webhook([press("u1", "bulk.seven")]).await.status,
        200
    );
    let seen = h.fake_channel().seen_by(&FakeChannel::account("u1"));
    assert_eq!(seen.len(), 7);
    let replies = seen
        .iter()
        .filter(|s| matches!(s.delivery, Delivery::Reply(_)));
    assert_eq!(replies.count(), 5, "one reply token covers 5 objects");
    assert!(
        seen[5..]
            .iter()
            .all(|s| matches!(s.delivery, Delivery::Push))
    );
    // The 2 spill-over objects go in one Push request: one metered unit.
    let calls = h.fake_channel().calls();
    assert_eq!(calls.len(), 2, "one reply call, one push call");
    assert_eq!((calls[0].objects, calls[1].objects), (5, 2));
    assert_eq!(seen[5].metering, Metering::Metered);
    assert_eq!(seen[6].metering, Metering::Free);
    assert_eq!(app.count(OUTBOUND).await, 7);
    assert_eq!((app.count(FREE).await, app.count(METERED).await), (6, 1));
    let module = app
        .count("SELECT count(*) FROM outbound_message WHERE module = 'bulk' AND push = 1")
        .await;
    assert_eq!(module, 2);
}

#[tokio::test]
async fn one_failing_event_does_not_block_the_next_in_the_same_call() {
    let (h, app) = linked(FakeProfile::LineLike).await;
    let r = app
        .fake_webhook([press("u1", "bulk.fail"), press("u1", "bulk.seven")])
        .await;
    assert_eq!(r.status, 200);
    assert_eq!(
        h.fake_channel().sent_to(&FakeChannel::account("u1")).len(),
        7
    );
    let logged = app
        .query("SELECT details FROM security_event WHERE kind = 'inbound.failed'")
        .await;
    assert_eq!(logged.len(), 1);
    let details = logged.get(0, "details").unwrap().as_text().unwrap();
    assert!(details.contains("registrar is down"), "{details}");
    assert!(details.contains("\"event\":\"action\""), "{details}");
}

#[tokio::test]
async fn a_send_to_an_unseen_account_creates_its_channel_account_row() {
    let h = Harness::new();
    let app = h.start().await;
    let to = FakeChannel::account("u-imported");
    app.deployment()
        .send(&to, None, &Message::text("ประกาศ"), &Locale::th())
        .await
        .unwrap();
    let rows = app
        .query(
            "SELECT ca.flow_state, ca.first_seen_at FROM outbound_message o \
             JOIN channel_account ca ON ca.id = o.channel_account_id \
             WHERE ca.external_id = 'u-imported'",
        )
        .await;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows.get(0, "flow_state").unwrap().as_text(), Some("new"));
    assert_eq!(
        rows.get(0, "first_seen_at").unwrap().as_i64(),
        Some(START_MS)
    );
}

#[tokio::test]
async fn a_telegram_like_channel_never_replies_and_never_meters() {
    let (h, app) = linked(FakeProfile::TelegramLike).await;
    app.fake_webhook([press("u1", "bulk.seven")]).await;
    let seen = h.fake_channel().seen_by(&FakeChannel::account("u1"));
    assert_eq!(seen.len(), 7);
    assert!(seen.iter().all(|s| matches!(s.delivery, Delivery::Push)));
    assert!(seen.iter().all(|s| s.metering == Metering::Free));
    assert_eq!((app.count(FREE).await, app.count(METERED).await), (7, 0));
}

#[tokio::test]
async fn every_inbound_kind_can_be_posted_through_the_fake_channel() {
    let h = Harness::new();
    let app = h.start().await;
    let from = FakeChannel::account("u2");
    let events = [
        Inbound::Follow {
            from: from.clone(),
            reply: token(START_MS + 1),
            event_id: None,
            redelivery: false,
        },
        Inbound::Text {
            from: from.clone(),
            text: "สอบถามค่าเทอม".into(),
            reply: None,
            event_id: None,
            redelivery: false,
        },
        Inbound::File {
            from: from.clone(),
            file: harpseal::IncomingFile {
                platform_file_id: "f-1".into(),
                file_name: Some("ใบลา.pdf".into()),
                content_type: Some("application/pdf".into()),
                size_bytes: Some(10),
            },
            reply: None,
            event_id: None,
            redelivery: false,
        },
        Inbound::Blocked {
            from: from.clone(),
            event_id: None,
            redelivery: false,
        },
        Inbound::Unblocked {
            from: from.clone(),
            reply: None,
            event_id: None,
            redelivery: false,
        },
    ];
    let fake = h.fake_channel();
    let parsed = fake.parse_webhook(fake.webhook(&events)).await.unwrap();
    let kinds: Vec<&str> = parsed.iter().map(Inbound::kind).collect();
    assert_eq!(kinds, ["follow", "text", "file", "blocked", "unblocked"]);
    assert!(
        parsed[0].reply().is_some(),
        "the reply token survives the wire"
    );
    assert_eq!(app.fake_webhook(events).await.status, 200);
}

#[tokio::test]
async fn a_reply_token_works_once_on_a_line_like_channel() {
    let (_h, app) = linked(FakeProfile::LineLike).await;
    let d = app.deployment();
    let to = FakeChannel::account("u1");
    let token = token(START_MS + 60_000);
    let first = d
        .send(&to, token.as_ref(), &Message::text("ครั้งแรก"), &Locale::th())
        .await
        .unwrap();
    let second = d
        .send(
            &to,
            token.as_ref(),
            &Message::text("ครั้งที่สอง"),
            &Locale::th(),
        )
        .await
        .unwrap();
    assert_eq!((first.metering, first.pushed), (Metering::Free, false));
    assert_eq!(
        (second.metering, second.pushed),
        (Metering::Metered, true),
        "a spent token turns the second send into a metered Push"
    );
    let rows = app
        .count("SELECT count(*) FROM outbound_message WHERE push = 1 AND metering = 'metered'")
        .await;
    assert_eq!(rows, 1);
}

#[tokio::test]
async fn a_failed_event_is_logged_against_its_account() {
    let (_h, app) = linked(FakeProfile::LineLike).await;
    app.fake_webhook([press("u1", "bulk.fail")]).await;
    let account = app
        .count(
            "SELECT count(*) FROM security_event \
             WHERE kind = 'inbound.failed' AND channel_account_id = 'a1'",
        )
        .await;
    assert_eq!(account, 1);
}

//! What the send path writes for the usage page (#26): a real Fake send
//! lands on the local day the page reads, and each failed attempt at the
//! platform counts in `usage_daily.failed`; a 429 and a Push skipped
//! before the platform (the account blocked the bot) count nowhere.

mod usage_support;

use harpseal::{Error, Locale, Message};
use harpseal_channel_fake::FakeChannel;
use harpseal_testing::START_MS;
use usage_support::*;

async fn row(h: &harpseal_testing::Harness) -> (i64, i64, i64) {
    let m = int(h, "SELECT metered FROM usage_daily WHERE channel = 'fake'").await;
    let f = int(h, "SELECT free FROM usage_daily WHERE channel = 'fake'").await;
    let x = int(h, "SELECT failed FROM usage_daily WHERE channel = 'fake'").await;
    (m, f, x)
}

#[tokio::test]
async fn a_real_send_lands_on_the_local_day_the_page_reads() {
    let (h, app) = boot().await;
    // 23:30 on 5 November UTC is 06:30 on 6 November in +07:00.
    h.clock().set_ms(START_MS + 23 * 3_600_000 + 30 * 60_000);
    let to = FakeChannel::account("u1");
    app.deployment()
        .send(
            &to,
            None,
            &Message::text("ข่าวจากมหาวิทยาลัยอ่าวแมวน้ำ"),
            &Locale::th(),
        )
        .await
        .unwrap();
    let day = app
        .query("SELECT day FROM usage_daily WHERE channel = 'fake'")
        .await;
    assert_eq!(
        day.get(0, "day").and_then(|v| v.as_text()),
        Some("2026-11-06")
    );
    let b = signed_in(&h, &app, "officer", "Officer").await;
    let r = b.get("/admin/usage").await;
    assert!(r.body.contains("ใช้ไป 1 จาก 300 (0%)"), "{}", r.body);
    let at = r.body.find("<tr data-day=\"2026-11-06\">").unwrap();
    assert!(r.body[at..].starts_with(
        "<tr data-day=\"2026-11-06\"><td data-label=\"วันที่\">6 พ.ย. 2569</td><td class=\"num\" data-label=\"ข้อความที่คิดค่าบริการ\">1</td>"
    ));
}

#[tokio::test]
async fn a_send_the_platform_refuses_counts_as_failed() {
    let (h, app) = boot().await;
    let to = FakeChannel::account("u1");
    let fake = h.fake_channel();
    let d = app.deployment();
    d.send(&to, None, &Message::text("หนึ่ง"), &Locale::th())
        .await
        .unwrap();
    assert_eq!(row(&h).await, (1, 0, 0));
    // A 429 is "try later": the Job retries it, nothing counts.
    fake.fail_next_batch_at(0);
    let e = d
        .send(&to, None, &Message::text("สอง"), &Locale::th())
        .await;
    assert!(matches!(e, Err(Error::RateLimited { .. })), "{e:?}");
    assert_eq!(row(&h).await, (1, 0, 0));
    // Another error counts, once per attempt.
    fake.fail_next_batch_at_with(0, "fake: 500");
    let e = d
        .send(&to, None, &Message::text("สอง"), &Locale::th())
        .await;
    assert!(matches!(e, Err(Error::Platform(_))), "{e:?}");
    assert_eq!(row(&h).await, (1, 0, 1));
    fake.fail_next_batch_at_with(0, "fake: 500");
    assert!(
        d.send(&to, None, &Message::text("สอง"), &Locale::th())
            .await
            .is_err()
    );
    assert_eq!(row(&h).await, (1, 0, 2));
    // The platform answers that the Student blocked the bot: failed too.
    fake.set_unreachable(&to, true);
    let e = d
        .send(&to, None, &Message::text("สาม"), &Locale::th())
        .await;
    assert!(matches!(e, Err(Error::Unreachable)), "{e:?}");
    assert_eq!(row(&h).await, (1, 0, 3));
}

#[tokio::test]
async fn a_batch_stopped_part_way_counts_what_did_not_wholly_go_out() {
    let (h, app) = boot().await;
    let to = FakeChannel::account("u1");
    h.fake_channel().fail_next_batch_at_with(1, "fake: 500");
    let msgs: Vec<Message> = ["หนึ่ง", "สอง", "สาม"].map(Message::text).to_vec();
    let e = app
        .deployment()
        .send_batch(&to, None, &msgs, &Locale::th())
        .await;
    let Err(Error::PartialSend { sent, total, .. }) = e else {
        panic!("{e:?}");
    };
    assert_eq!((sent, total), (1, 3));
    let (metered, _, failed) = row(&h).await;
    assert_eq!(failed, 2);
    assert!(metered >= 1);
}

#[tokio::test]
async fn a_push_skipped_for_a_blocked_account_counts_nowhere() {
    let (h, app) = boot().await;
    let to = FakeChannel::account("u1");
    let d = app.deployment();
    d.send(&to, None, &Message::text("หนึ่ง"), &Locale::th())
        .await
        .unwrap();
    run(
        &h,
        "UPDATE channel_account SET blocked_at = 1 WHERE external_id = 'u1'",
    )
    .await;
    let e = d
        .send(&to, None, &Message::text("สอง"), &Locale::th())
        .await;
    assert!(matches!(e, Err(Error::Unreachable)));
    assert_eq!(row(&h).await, (1, 0, 0));
    assert_eq!(h.fake_channel().sent().len(), 1);
}

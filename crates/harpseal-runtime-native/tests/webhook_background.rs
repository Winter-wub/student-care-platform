//! Webhooks answered 200 after the claim and handled on a
//! [`harpseal_runtime_native::NativeBackground`] (#97), over a real TCP
//! connection: handling survives the client hanging up, runs one at a time
//! per chat account, records failures, and a crash mid-handling is still
//! caught by the lost-claim sweep (ADR-0009). Leases and takeovers:
//! `webhook_background_lease.rs`.

mod webhook_support;

use std::time::Duration;

use harpseal_channel_fake::FakeChannel;
use harpseal_runtime_native::NativeBackground;
use webhook_support::{
    OK, Setup, assert_none_starts, listen, open_claims, post, press, setup, status, tick_lost,
    wait_started,
};

#[tokio::test]
async fn background_handling_finishes_after_the_client_hangs_up() {
    let Setup { h, app, gate } = setup().await;
    let bg = NativeBackground::new(4);
    let addr = listen(&app, Some(&bg)).await;
    let call = h.fake_channel().webhook(&[press("u1", "a", "evt-1")]);
    // The platform hangs up while the handler is still waiting.
    let conn = post(addr, &call).await;
    wait_started(&gate).await;
    drop(conn);
    gate.release.add_permits(1);
    assert!(bg.wait_idle(Duration::from_secs(5)).await);
    assert_eq!(gate.log(), ["start a", "end a"]);
    assert_eq!(open_claims(&app).await, 0, "done_at is set");
    let sent = h.fake_channel().sent_to(&FakeChannel::account("u1"));
    assert_eq!(sent.len(), 1, "the reply went out");
}

#[tokio::test]
async fn inline_handling_stops_when_the_client_hangs_up() {
    // Without a Background the handling is the request's own future, so a
    // hang-up drops it half way: the failure #97 fixes.
    let Setup { h, app, gate } = setup().await;
    let addr = listen(&app, None).await;
    let call = h.fake_channel().webhook(&[press("u1", "a", "evt-1")]);
    let conn = post(addr, &call).await;
    wait_started(&gate).await;
    drop(conn);
    tokio::time::timeout(Duration::from_secs(5), gate.dropped.notified())
        .await
        .expect("the server dropped the handler");
    assert_eq!(gate.log(), ["start a", "dropped"]);
    assert_eq!(open_claims(&app).await, 1, "claim left unfinished");
}

#[tokio::test]
async fn background_events_of_one_account_run_one_at_a_time_in_order() {
    let Setup { h, app, gate } = setup().await;
    let bg = NativeBackground::new(4);
    let addr = listen(&app, Some(&bg)).await;
    let fake = h.fake_channel();
    assert_eq!(
        status(addr, &fake.webhook(&[press("u1", "a", "e1")])).await,
        OK
    );
    wait_started(&gate).await;
    // u1's second event waits behind the first; u2's runs alongside.
    assert_eq!(
        status(addr, &fake.webhook(&[press("u1", "b", "e2")])).await,
        OK
    );
    assert_eq!(
        status(addr, &fake.webhook(&[press("u2", "c", "e3")])).await,
        OK
    );
    wait_started(&gate).await;
    assert_none_starts(&gate).await;
    assert_eq!(gate.log(), ["start a", "start c"]);
    gate.release.add_permits(3);
    assert!(bg.wait_idle(Duration::from_secs(5)).await);
    let log = gate.log();
    let at = |e: &str| log.iter().position(|l| l == e).unwrap();
    assert!(at("end a") < at("start b"), "{log:?}");
    assert_eq!(log.len(), 6);
    assert_eq!(open_claims(&app).await, 0);
}

#[tokio::test]
async fn background_a_busy_account_does_not_hold_the_only_slot() {
    // One slot: u1's queued second event must not keep it while u2 waits.
    let Setup { h, app, gate } = setup().await;
    let bg = NativeBackground::new(1);
    let addr = listen(&app, Some(&bg)).await;
    let fake = h.fake_channel();
    let call = fake.webhook(&[press("u1", "a", "e1"), press("u1", "b", "e2")]);
    assert_eq!(status(addr, &call).await, OK);
    wait_started(&gate).await;
    assert_eq!(
        status(addr, &fake.webhook(&[press("u2", "c", "e3")])).await,
        OK
    );
    // a holds the only slot.
    assert_none_starts(&gate).await;
    // a ends and frees the slot: c, which has waited for it since before
    // b asked, takes it (tokio's semaphore is fair).
    gate.release.add_permits(1);
    wait_started(&gate).await;
    assert_eq!(gate.log(), ["start a", "end a", "start c"]);
    gate.release.add_permits(2);
    assert!(bg.wait_idle(Duration::from_secs(5)).await);
    assert_eq!(gate.log().len(), 6);
}

#[tokio::test]
async fn background_a_duplicate_and_a_bad_signature_are_answered_before_handling() {
    let Setup { h, app, gate } = setup().await;
    let bg = NativeBackground::new(4);
    let addr = listen(&app, Some(&bg)).await;
    let fake = h.fake_channel();
    let call = fake.webhook(&[press("u1", "a", "e1")]);
    assert_eq!(status(addr, &call).await, OK);
    wait_started(&gate).await;
    // The redelivery finds the claim taken: skipped, never handled twice.
    assert_eq!(status(addr, &call).await, OK);
    let mut forged = fake.webhook(&[press("u1", "b", "e2")]);
    forged.body.push(b' ');
    assert_eq!(status(addr, &forged).await, "HTTP/1.1 401 Unauthorized");
    gate.release.add_permits(2);
    assert!(bg.wait_idle(Duration::from_secs(5)).await);
    assert_eq!(gate.log(), ["start a", "end a"]);
    assert_eq!(app.count("SELECT count(*) FROM inbound_event").await, 1);
}

#[tokio::test]
async fn background_a_failure_is_logged_and_a_panic_is_left_to_the_sweep() {
    let s = setup().await;
    let bg = NativeBackground::new(4);
    let addr = listen(&s.app, Some(&bg)).await;
    let fake = s.h.fake_channel();
    let calls = [
        fake.webhook(&[press("u1", "fail", "e1")]),
        fake.webhook(&[press("u1", "panic", "e2")]),
        fake.webhook(&[press("u1", "a", "e3")]),
    ];
    for call in &calls {
        assert_eq!(status(addr, call).await, OK);
    }
    s.gate.release.add_permits(1);
    assert!(bg.wait_idle(Duration::from_secs(5)).await);
    let failed = "SELECT count(*) FROM security_event WHERE kind = 'inbound.failed'";
    assert_eq!(s.app.count(failed).await, 1);
    // The panic did not stall u1: its next event was still handled.
    assert_eq!(s.gate.log(), ["start a", "end a"]);
    // The panicked event's claim stays open, as for a killed process,
    // and the tick logs it once the lease has passed.
    assert_eq!(open_claims(&s.app).await, 1);
    s.h.clock().advance_ms(61_000);
    assert_eq!(tick_lost(&s).await, 1);
}

//! Background webhook handling against the 60 s claim lease (#97,
//! ADR-0009): a slow but progressing account keeps its order, a hung one
//! is taken over, a claim's lease restarts when its handling starts, and
//! an event the tick logged as lost while it waited is never answered.

mod webhook_support;

use std::time::Duration;

use harpseal_channel_fake::FakeChannel;
use harpseal_runtime_native::NativeBackground;
use webhook_support::{
    LOST, OK, assert_none_starts, listen, open_claims, press, setup, status, tick_lost,
    wait_started,
};

#[tokio::test]
async fn background_a_slow_account_keeps_its_order_past_the_lease() {
    // Three ~30 s events in one call: 61 s after the call the account is
    // still making progress, so a new call waits instead of taking over.
    let s = setup().await;
    let bg = NativeBackground::new(4);
    let addr = listen(&s.app, Some(&bg)).await;
    let fake = s.h.fake_channel();
    let call = fake.webhook(&[
        press("u1", "a", "e1"),
        press("u1", "b", "e2"),
        press("u1", "c", "e3"),
    ]);
    assert_eq!(status(addr, &call).await, OK);
    wait_started(&s.gate).await;
    s.h.clock().advance_ms(30_000);
    s.gate.release.add_permits(1);
    wait_started(&s.gate).await;
    s.h.clock().advance_ms(31_000);
    assert_eq!(
        status(addr, &fake.webhook(&[press("u1", "d", "e4")])).await,
        OK
    );
    assert_none_starts(&s.gate).await;
    assert_eq!(s.gate.log(), ["start a", "end a", "start b"]);
    s.gate.release.add_permits(3);
    assert!(bg.wait_idle(Duration::from_secs(5)).await);
    let order = [
        "start a", "end a", "start b", "end b", "start c", "end c", "start d", "end d",
    ];
    assert_eq!(s.gate.log(), order);
    assert_eq!(open_claims(&s.app).await, 0);
}

#[tokio::test]
async fn background_a_hung_account_is_taken_over_by_its_next_call() {
    let s = setup().await;
    let bg = NativeBackground::new(4);
    let addr = listen(&s.app, Some(&bg)).await;
    let fake = s.h.fake_channel();
    assert_eq!(
        status(addr, &fake.webhook(&[press("u1", "a", "e1")])).await,
        OK
    );
    wait_started(&s.gate).await;
    assert_eq!(
        status(addr, &fake.webhook(&[press("u1", "b", "e2")])).await,
        OK
    );
    // a hangs for over the lease: the next call runs without waiting.
    s.h.clock().advance_ms(61_000);
    assert_eq!(
        status(addr, &fake.webhook(&[press("u1", "c", "e3")])).await,
        OK
    );
    wait_started(&s.gate).await;
    assert_eq!(s.gate.log(), ["start a", "start c"]);
    s.gate.release.add_permits(2);
    assert!(bg.wait_idle(Duration::from_secs(5)).await);
    // b was queued behind the hung a: dropped by the takeover, never
    // answered, left for the tick.
    assert!(!s.gate.log().contains(&"start b".to_string()));
    assert_eq!(open_claims(&s.app).await, 1);
    s.h.clock().advance_ms(61_000);
    assert_eq!(tick_lost(&s).await, 1);
}

#[tokio::test]
async fn background_a_queued_claim_starts_its_lease_when_handled() {
    let s = setup().await;
    let bg = NativeBackground::new(4);
    let addr = listen(&s.app, Some(&bg)).await;
    let fake = s.h.fake_channel();
    assert_eq!(
        status(addr, &fake.webhook(&[press("u1", "a", "e1")])).await,
        OK
    );
    wait_started(&s.gate).await;
    assert_eq!(
        status(addr, &fake.webhook(&[press("u1", "b", "e2")])).await,
        OK
    );
    // b waits 50 s behind a, then runs; 70 s after its claim it has run
    // for 20 s only: not lost.
    s.h.clock().advance_ms(50_000);
    s.gate.release.add_permits(1);
    wait_started(&s.gate).await;
    s.h.clock().advance_ms(20_000);
    assert_eq!(tick_lost(&s).await, 0);
    s.gate.release.add_permits(1);
    assert!(bg.wait_idle(Duration::from_secs(5)).await);
    assert_eq!(s.app.count(LOST).await, 0);
    assert_eq!(open_claims(&s.app).await, 0);
}

#[tokio::test]
async fn background_an_event_logged_lost_while_queued_is_never_answered() {
    let s = setup().await;
    let bg = NativeBackground::new(4);
    let addr = listen(&s.app, Some(&bg)).await;
    let fake = s.h.fake_channel();
    assert_eq!(
        status(addr, &fake.webhook(&[press("u1", "a", "e1")])).await,
        OK
    );
    wait_started(&s.gate).await;
    assert_eq!(
        status(addr, &fake.webhook(&[press("u1", "b", "e2")])).await,
        OK
    );
    // Both claims pass the lease: a still running, b still queued.
    s.h.clock().advance_ms(61_000);
    assert_eq!(tick_lost(&s).await, 2);
    s.gate.release.add_permits(2);
    assert!(bg.wait_idle(Duration::from_secs(5)).await);
    // b is skipped, so its `inbound.lost` stays true.
    assert_eq!(s.gate.log(), ["start a", "end a"]);
    let sent = s.h.fake_channel().sent_to(&FakeChannel::account("u1"));
    assert_eq!(sent.len(), 1, "only a answered");
    assert_eq!(s.app.count(LOST).await, 2);
}

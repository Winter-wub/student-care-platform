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
    wait_pending, wait_started,
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

#[tokio::test]
async fn background_a_task_of_a_taken_over_lane_never_touches_a_newer_lane() {
    let s = setup().await;
    let bg = NativeBackground::new(4);
    let addr = listen(&s.app, Some(&bg)).await;
    let fake = s.h.fake_channel();
    let call = fake.webhook(&[press("u1", "a", "e1"), press("u1", "b", "e2")]);
    assert_eq!(status(addr, &call).await, OK);
    wait_started(&s.gate).await;
    // a hangs past the lease: a takeover runs `quick`, which ends and
    // leaves u1 idle (only a's task is left).
    s.h.clock().advance_ms(61_000);
    assert_eq!(
        status(addr, &fake.webhook(&[press("u1", "quick", "e3")])).await,
        OK
    );
    wait_pending(&bg, 1).await;
    // New work makes a new lane for u1; c runs on it.
    assert_eq!(
        status(addr, &fake.webhook(&[press("u1", "c", "e4")])).await,
        OK
    );
    wait_started(&s.gate).await;
    // a ends (first in line for the permit): its task must stop, not
    // handle b next to c.
    s.gate.release.add_permits(1);
    assert_none_starts(&s.gate).await;
    // ... and must not have removed c's lane: d waits behind c.
    assert_eq!(
        status(addr, &fake.webhook(&[press("u1", "d", "e5")])).await,
        OK
    );
    assert_none_starts(&s.gate).await;
    assert_eq!(s.gate.log(), ["start a", "quick", "start c", "end a"]);
    s.gate.release.add_permits(2);
    assert!(bg.wait_idle(Duration::from_secs(5)).await);
    let order = [
        "start a", "quick", "start c", "end a", "end c", "start d", "end d",
    ];
    assert_eq!(s.gate.log(), order);
    // b stays claimed, for the tick to log as lost.
    assert_eq!(open_claims(&s.app).await, 1);
}

#[tokio::test]
async fn background_an_account_waiting_for_a_slot_is_not_taken_over() {
    let s = setup().await;
    let bg = NativeBackground::new(1);
    let addr = listen(&s.app, Some(&bg)).await;
    let fake = s.h.fake_channel();
    // u2's c holds the only slot; u1's a waits for it.
    assert_eq!(
        status(addr, &fake.webhook(&[press("u2", "c", "e1")])).await,
        OK
    );
    wait_started(&s.gate).await;
    assert_eq!(
        status(addr, &fake.webhook(&[press("u1", "a", "e2")])).await,
        OK
    );
    assert_none_starts(&s.gate).await;
    // 61 s later u1 is throttled, not hung: b queues behind a.
    s.h.clock().advance_ms(61_000);
    assert_eq!(
        status(addr, &fake.webhook(&[press("u1", "b", "e3")])).await,
        OK
    );
    s.gate.release.add_permits(3);
    assert!(bg.wait_idle(Duration::from_secs(5)).await);
    let order = ["start c", "end c", "start a", "end a", "start b", "end b"];
    assert_eq!(s.gate.log(), order);
}

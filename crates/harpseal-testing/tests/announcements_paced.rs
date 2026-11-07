//! Paced edit chunks (#49 review): a later save does not drop queued
//! chunks, chunks past the 10th wait a minute, a re-run parent inserts
//! nothing twice, and a chunk rate-limited past its hop limit ends.

mod announcements_support;

use announcements_support::*;
use harpseal::Message;

async fn telegram_students(w: &World, n: usize) {
    for i in 0..n {
        let id = format!("t{i:03}");
        student(&w.h, &id, "th", "active", r#"{"programme":"MSC","year":2}"#).await;
        link(&w.h, &id, TG, "active").await;
    }
}

async fn later(w: &World, minutes: i64) {
    w.h.clock().advance_minutes(minutes);
    drain(w).await;
}

/// `edit_report` as `edited/total/failed`.
async fn report(w: &World) -> Vec<String> {
    texts(
        w,
        "SELECT (edit_report ->> 'edited') || '/' || (edit_report ->> 'total') || '/' \
         || (edit_report ->> 'failed') FROM announcement",
    )
    .await
}

/// Sends an announcement to `n` Telegram Students, then saves new texts:
/// returns the browser's announcement id.
async fn sent_then_edited(w: &World, b: &harpseal_testing::Browser<'_>, n: usize) -> String {
    telegram_students(w, n).await;
    let id = create(b, &form("เดิม", "ข้อความเดิม", "&do=confirm")).await;
    drain(w).await;
    later(w, 1).await;
    assert_eq!(
        count(
            w,
            "SELECT count(*) FROM announcement_delivery WHERE status = 'sent'"
        )
        .await,
        i64::try_from(n).unwrap()
    );
    let edit = format!("/admin/announcements/{id}/edit");
    b.get(&edit).await;
    let v = version(w, &id).await;
    let r = b
        .post(
            &edit,
            &format!("{}&version={v}", form("ใหม่", "ข้อความใหม่", "")),
        )
        .await;
    assert_eq!(r.status, 303, "{}", r.body);
    id
}

#[tokio::test]
async fn a_later_save_does_not_drop_the_edit_chunks_still_queued() {
    let w = boot().await;
    let b = signed_in(&w, "Officer").await;
    let id = sent_then_edited(&w, &b, 260).await;
    let start = w.h.clock().now_ms();
    drain(&w).await;
    // 11 chunks: the first 10 now, the 11th a minute later.
    let at = |n: u32| {
        let w = &w;
        async move {
            count(
                w,
                &format!(
                    "SELECT run_at FROM job WHERE kind = 'announcements.edit.chunk' \
                     AND unique_key LIKE '%:tg:{n}'"
                ),
            )
            .await
        }
    };
    assert_eq!(at(9).await, start);
    assert_eq!(at(10).await, start + 60_000);
    assert_eq!(w.tg.edited().len(), 250);
    assert_eq!(report(&w).await, ["250/260/0"]);
    // Before chunk 10 runs, Staff save again: only the expiry changes
    // (the version moves, no new edit Job).
    let edit = format!("/admin/announcements/{id}/edit");
    b.get(&edit).await;
    let v = version(&w, &id).await;
    let r = b
        .post(
            &edit,
            &format!(
                "{}&expires=2027-06-30&version={v}",
                form("ใหม่", "ข้อความใหม่", "")
            ),
        )
        .await;
    assert_eq!(r.status, 303, "{}", r.body);
    assert_eq!(
        count(
            &w,
            "SELECT count(*) FROM job WHERE kind = 'announcements.edit'"
        )
        .await,
        1
    );
    later(&w, 1).await;
    let edited = w.tg.edited();
    assert_eq!(edited.len(), 260, "every Card has the current text");
    assert!(
        edited
            .iter()
            .all(|(_, m)| matches!(m, Message::Card(c) if c.title == "ใหม่"))
    );
    assert_eq!(report(&w).await, ["260/260/0"]);
}

#[tokio::test]
async fn running_the_edit_parent_again_inserts_no_second_chunk() {
    let w = boot().await;
    let b = signed_in(&w, "Officer").await;
    sent_then_edited(&w, &b, 20).await;
    drain(&w).await;
    assert_eq!(w.tg.edited().len(), 20);
    let payload = texts(
        &w,
        "SELECT payload FROM job WHERE kind = 'announcements.edit'",
    )
    .await;
    let payload: serde_json::Value = serde_json::from_str(&payload[0]).unwrap();
    w.app
        .deployment()
        .enqueue("announcements.edit", payload, None)
        .await
        .unwrap();
    drain(&w).await;
    assert_eq!(
        count(
            &w,
            "SELECT count(*) FROM job WHERE kind = 'announcements.edit.chunk'"
        )
        .await,
        1,
        "the same key: nothing inserted"
    );
    assert_eq!(w.tg.edited().len(), 20, "nothing edited twice");
    assert_eq!(report(&w).await, ["20/20/0"], "the counts are kept");
}

#[tokio::test]
async fn a_chunk_rate_limited_past_its_hop_limit_counts_the_rest_as_failed() {
    let w = boot().await;
    let b = signed_in(&w, "Officer").await;
    // 5 edits go through, then every one is refused with a 429.
    w.tg.fail_maintenance(5, usize::MAX);
    sent_then_edited(&w, &b, 20).await;
    drain(&w).await;
    assert_eq!(w.tg.edited().len(), 5);
    // One continuation a minute (retry after 30 s), 50 of them.
    for _ in 0..55 {
        later(&w, 1).await;
    }
    assert_eq!(w.tg.edited().len(), 5);
    assert_eq!(report(&w).await, ["5/20/15"]);
    assert_eq!(
        count(
            &w,
            "SELECT count(*) FROM job WHERE kind = 'announcements.edit.chunk' \
             AND state <> 'done'"
        )
        .await,
        0,
        "the chain ended without a failed Job"
    );
    assert_eq!(
        count(
            &w,
            "SELECT count(*) FROM job WHERE kind = 'announcements.edit.chunk'"
        )
        .await,
        51,
        "the chunk and its 50 continuations"
    );
}

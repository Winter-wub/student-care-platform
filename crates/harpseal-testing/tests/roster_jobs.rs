//! The Job `kernel.roster.import` at the Deployment seam (#24): 500-row
//! chunks, idempotent re-runs, a chunk that fails part-way, and the left
//! transition revoking Links.

mod roster_support;

use harpseal::{Job, Timestamp};
use harpseal_channel_fake::FakeChannel;
use roster_support::*;

const ACTIVE: &str = "SELECT count(*) FROM student WHERE status = 'active'";

#[tokio::test]
async fn twelve_hundred_rows_run_as_three_chunk_jobs() {
    let (h, app) = boot().await;
    let b = signed_in(&h, &app, "Officer").await;
    let id = previewed(&b, "big.csv", &csv(0..1200), "full", false).await;
    confirm(&b, &id).await;
    let mut applied = Vec::new();
    for _ in 0..3 {
        let r = app
            .deployment()
            .tick(Timestamp(h.clock().now_ms()))
            .await
            .unwrap();
        assert_eq!(r.claimed, 1, "one chunk per Job");
        applied.push(
            int(
                &h,
                "SELECT applied_rows FROM roster_import WHERE id = ?",
                &[&id],
            )
            .await,
        );
    }
    assert_eq!(applied, vec![500, 1000, 1200]);
    assert_eq!(
        import_text(&h, &id, "state").await,
        "running",
        "the left phase is next"
    );
    let r = app
        .deployment()
        .tick(Timestamp(h.clock().now_ms()))
        .await
        .unwrap();
    assert_eq!(r.claimed, 1, "one left batch");
    assert_eq!(import_text(&h, &id, "state").await, "done");
    let keys = "SELECT group_concat(unique_key, ' ') FROM (SELECT unique_key FROM job \
                WHERE kind = 'kernel.roster.import' ORDER BY created_at, id)";
    let rows = h.db().query(harpseal::Statement::new(keys)).await.unwrap();
    assert_eq!(
        rows.rows[0][0].as_text().unwrap(),
        format!("roster:{id} roster:{id}:500 roster:{id}:1000 roster:{id}:left:0")
    );
    assert_eq!(int(&h, ACTIVE, &[]).await, 1200);
    assert_eq!(counts(&h, &id).await["result"]["new"], 1200);
}

fn chunk_job(id: &str, chunk: i64) -> Job {
    Job {
        id: "rerun".into(),
        kind: "kernel.roster.import".into(),
        payload: serde_json::json!({ "import_id": id, "chunk": chunk }).to_string(),
        attempt: 1,
    }
}

/// What a re-run must not change.
async fn snapshot(h: &harpseal_testing::Harness, id: &str) -> (String, i64, i64, i64, i64, i64) {
    (
        import_text(h, id, "counts").await,
        int(
            h,
            "SELECT applied_rows FROM roster_import WHERE id = ?",
            &[id],
        )
        .await,
        int(h, "SELECT count(*) FROM audit_log", &[]).await,
        int(h, "SELECT max(updated_at) FROM student", &[]).await,
        int(h, "SELECT count(*) FROM student_tag", &[]).await,
        int(h, "SELECT count(*) FROM job", &[]).await,
    )
}

#[tokio::test]
async fn running_a_finished_chunk_again_changes_nothing() {
    let (h, app) = boot().await;
    let b = signed_in(&h, &app, "Officer").await;
    let id = previewed(&b, "big.csv", &csv(0..600), "full", false).await;
    confirm(&b, &id).await;
    // Mid-import: chunk 0 is applied, chunk 500 is queued.
    app.deployment()
        .tick(Timestamp(h.clock().now_ms()))
        .await
        .unwrap();
    let mid = snapshot(&h, &id).await;
    assert_eq!(mid.1, 500);
    h.clock().advance_ms(10);
    app.deployment().run_job(chunk_job(&id, 0)).await.unwrap();
    assert_eq!(
        snapshot(&h, &id).await,
        mid,
        "a repeated chunk 0 is a no-op"
    );

    run_jobs(&h, &app).await;
    assert_eq!(import_text(&h, &id, "state").await, "done");
    let done = snapshot(&h, &id).await;
    h.clock().advance_minutes(5);
    for chunk in [0, 500] {
        app.deployment()
            .run_job(chunk_job(&id, chunk))
            .await
            .unwrap();
    }
    assert_eq!(snapshot(&h, &id).await, done);
    assert_eq!(int(&h, "SELECT count(*) FROM student", &[]).await, 600);
}

#[tokio::test]
async fn a_chunk_that_fails_part_way_writes_nothing_and_is_retried() {
    let (h, app) = boot().await;
    let b = signed_in(&h, &app, "Officer").await;
    let id = previewed(&b, "big.csv", &csv(0..1200), "full", false).await;
    confirm(&b, &id).await;
    // The 700th row breaks the second chunk's batch at its 200th upsert.
    run(
        &h,
        "CREATE TRIGGER boom BEFORE INSERT ON student WHEN NEW.student_code = '68000700' \
         BEGIN SELECT RAISE(ABORT, 'injected failure'); END",
    )
    .await;
    let tick = || app.deployment().tick(Timestamp(h.clock().now_ms()));
    assert_eq!(tick().await.unwrap().done, 1, "first chunk");
    let r = tick().await.unwrap();
    assert_eq!((r.claimed, r.retried), (1, 1), "second chunk fails");
    assert_eq!(
        int(&h, "SELECT count(*) FROM student", &[]).await,
        500,
        "no partial rows"
    );
    assert_eq!(
        int(
            &h,
            "SELECT applied_rows FROM roster_import WHERE id = ?",
            &[&id]
        )
        .await,
        500
    );
    assert_eq!(import_text(&h, &id, "state").await, "running");
    run(&h, "DROP TRIGGER boom").await;
    h.clock().advance_minutes(2);
    run_jobs(&h, &app).await;
    assert_eq!(import_text(&h, &id, "state").await, "done");
    assert_eq!(int(&h, "SELECT count(*) FROM student", &[]).await, 1200);
}

#[tokio::test]
async fn a_chunk_that_keeps_failing_marks_the_import_failed_without_file_contents() {
    let (h, app) = boot().await;
    let b = signed_in(&h, &app, "Officer").await;
    let id = previewed(&b, "a.csv", &csv(0..5), "full", false).await;
    confirm(&b, &id).await;
    run(
        &h,
        "CREATE TRIGGER boom BEFORE INSERT ON student \
         BEGIN SELECT RAISE(ABORT, 'นักศึกษา 3 s68000003@student.sealbay.example'); END",
    )
    .await;
    for _ in 0..4 {
        app.deployment()
            .tick(Timestamp(h.clock().now_ms()))
            .await
            .unwrap();
        h.clock().advance_minutes(31);
    }
    assert_eq!(import_text(&h, &id, "state").await, "failed");
    let error = import_text(&h, &id, "error").await;
    assert!(error.contains("database error"), "{error}");
    assert!(!error.contains("sealbay"), "no cell values: {error}");
    let page = b.get(&format!("/admin/roster/{id}")).await.body;
    assert!(page.contains("การนำเข้าหยุดกลางทาง"), "{page}");
    // The slot is free again.
    previewed(&b, "b.csv", &csv(0..5), "full", false).await;
}

#[tokio::test]
async fn students_who_leave_lose_their_links_and_are_told() {
    let (h, app) = boot().await;
    let b = signed_in(&h, &app, "Officer").await;
    // On the roster before: two linked active Students and one left one.
    linked_student(&h, "gone", "68000900", "active").await;
    linked_student(&h, "kept", "68000001", "active").await;
    run(
        &h,
        "INSERT INTO student (id, student_code, display_name, locale, status, created_at, \
         updated_at) VALUES ('back', '68000002', 'Back', 'th', 'left', 1, 1)",
    )
    .await;
    let id = previewed(&b, "a.csv", &csv(0..3), "full", true).await;
    let c = counts(&h, &id).await;
    assert_eq!(c["preview"]["will_become_left"], 1);
    assert_eq!(c["preview"]["returning"], 1);
    assert_eq!(c["preview"]["left_names"], serde_json::json!(["Old gone"]));
    confirm(&b, &id).await;
    run_jobs(&h, &app).await;
    assert_eq!(import_text(&h, &id, "state").await, "done");

    let status = |sid: &str| {
        let sql = format!("SELECT count(*) FROM student WHERE id = '{sid}' AND status = ?");
        (sql, sid.to_string())
    };
    let (sql, _) = status("gone");
    assert_eq!(int(&h, &sql, &["left"]).await, 1);
    let (sql, _) = status("back");
    assert_eq!(int(&h, &sql, &["active"]).await, 1, "returning");
    let link = "SELECT count(*) FROM link WHERE id = ? AND status = 'revoked'";
    assert_eq!(int(&h, link, &["l-gone"]).await, 1);
    assert_eq!(int(&h, link, &["l-kept"]).await, 0);
    let flow = "SELECT count(*) FROM channel_account WHERE id = 'a-gone' \
                AND flow_state = 'awaiting_student_id'";
    assert_eq!(int(&h, flow, &[]).await, 1, "flow state reset");
    let told = h.fake_channel().sent_to(&FakeChannel::account("u-gone"));
    assert_eq!(told.len(), 1, "the revoked notice");
    assert!(
        h.fake_channel()
            .sent_to(&FakeChannel::account("u-kept"))
            .is_empty()
    );
    let c = counts(&h, &id).await;
    assert_eq!(
        (c["left"].as_i64(), c["links_revoked"].as_i64()),
        (Some(1), Some(1))
    );
    assert_eq!(c["notices_failed"], 0);
    let audit = "SELECT count(*) FROM audit_log WHERE actor_kind = 'job' AND target_id = ? \
                 AND action = ?";
    assert_eq!(int(&h, audit, &["gone", "student.status"]).await, 1);
    assert_eq!(int(&h, audit, &["l-gone", "link.revoke"]).await, 1);
}

#[tokio::test]
async fn a_status_set_by_hand_is_not_changed_by_a_full_import() {
    let (h, app) = boot().await;
    let b = signed_in(&h, &app, "Officer").await;
    linked_student(&h, "stay", "68000900", "active").await;
    run(
        &h,
        "UPDATE student SET manual_overrides = '[\"status\"]' WHERE id = 'stay'",
    )
    .await;
    let id = previewed(&b, "a.csv", &csv(0..2), "full", false).await;
    let c = counts(&h, &id).await;
    assert_eq!(c["preview"]["will_become_left"], 0);
    // A Student kept active, not a field kept: counted apart (#46).
    assert_eq!(c["preview"]["skipped_by_override"], 0);
    assert_eq!(c["preview"]["kept_by_status"], 1);
    confirm(&b, &id).await;
    run_jobs(&h, &app).await;
    let c = counts(&h, &id).await;
    assert_eq!(
        (c["kept_by_status"].as_i64(), c["left"].as_i64()),
        (Some(1), Some(0))
    );
    let page = b.get(&format!("/admin/roster/{id}")).await.body;
    for (count, both) in [("kept_by_status", "1"), ("left", "0"), ("new", "2")] {
        let row = page
            .split(&format!("data-count=\"{count}\""))
            .nth(1)
            .and_then(|r| r.split("</tr>").next())
            .unwrap_or_else(|| panic!("no {count} row: {page}"));
        let shown = |cell: &str| {
            row.split(cell)
                .nth(1)
                .and_then(|c| c.split('<').next())
                .map(|c| c.trim_start_matches('>').to_string())
        };
        assert_eq!(
            shown("data-preview").as_deref(),
            Some(both),
            "{count}: {row}"
        );
        assert_eq!(
            shown("data-result").as_deref(),
            Some(both),
            "{count}: {row}"
        );
    }
    assert_eq!(int(&h, ACTIVE, &[]).await, 3);
    assert_eq!(
        int(&h, "SELECT count(*) FROM link WHERE status = 'active'", &[]).await,
        1
    );
}

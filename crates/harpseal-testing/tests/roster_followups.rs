//! Roster import follow-ups (#46) at the Deployment seam: chunks read the
//! parsed rows, stored files are pruned after a year, and no object
//! outlives a failed upload. Body limits are in `roster_limits.rs`.

mod roster_support;

use harpseal::{DeploymentConfig, FileStore, Timestamp};
use roster_support::*;

const DAY_MS: i64 = 86_400_000;

#[tokio::test]
async fn chunks_read_the_parsed_rows_not_the_original_file() {
    let (h, app) = boot().await;
    let b = signed_in(&h, &app, "Officer").await;
    let id = previewed(&b, "big.csv", &csv(0..1200), "upsert", false).await;
    let rows_key = format!("roster-rows/{id}.jsonl");
    assert!(
        h.files().keys().contains(&rows_key),
        "{:?}",
        h.files().keys()
    );
    assert_eq!(
        counts(&h, &id).await["rows_sha256"].as_str().map(str::len),
        Some(64)
    );
    // With the original gone, only the parsed rows can feed the chunks.
    let file_key = import_text(&h, &id, "file_key").await;
    h.files().delete(&file_key).await.unwrap();
    confirm(&b, &id).await;
    run_jobs(&h, &app).await;
    assert_eq!(import_text(&h, &id, "state").await, "done");
    assert_eq!(int(&h, "SELECT count(*) FROM student", &[]).await, 1200);
    assert_eq!(counts(&h, &id).await["result"]["new"], 1200);
}

#[tokio::test]
async fn without_the_parsed_rows_a_chunk_reads_the_original_file() {
    let (h, app) = boot().await;
    let b = signed_in(&h, &app, "Officer").await;
    let id = previewed(&b, "a.xlsx", XLSX_50, "upsert", false).await;
    // A Deployment restored from a backup has only the original.
    h.files()
        .delete(&format!("roster-rows/{id}.jsonl"))
        .await
        .unwrap();
    confirm(&b, &id).await;
    run_jobs(&h, &app).await;
    assert_eq!(import_text(&h, &id, "state").await, "done");
    assert_eq!(int(&h, "SELECT count(*) FROM student", &[]).await, 50);
}

#[tokio::test]
async fn changed_parsed_rows_fail_the_import() {
    let (h, app) = boot().await;
    let b = signed_in(&h, &app, "Officer").await;
    let id = previewed(&b, "a.csv", &csv(0..3), "upsert", false).await;
    let forged = String::from_utf8(csv(0..3))
        .unwrap()
        .replace("นักศึกษา 1", "x");
    let rows_key = format!("roster-rows/{id}.jsonl");
    h.files()
        .put(&rows_key, forged.into_bytes(), "application/jsonl")
        .await
        .unwrap();
    confirm(&b, &id).await;
    for _ in 0..5 {
        app.deployment()
            .tick(Timestamp(h.clock().now_ms()))
            .await
            .unwrap();
        h.clock().advance_minutes(31);
    }
    assert_eq!(import_text(&h, &id, "state").await, "failed");
    let error = import_text(&h, &id, "error").await;
    assert!(error.contains("the stored roster rows changed"), "{error}");
    assert_eq!(int(&h, "SELECT count(*) FROM student", &[]).await, 0);
}

#[tokio::test]
async fn stored_files_are_deleted_a_year_after_upload_and_audited() {
    let (h, app) = boot().await;
    let b = signed_in(&h, &app, "Officer").await;
    let uploaded_at = h.clock().now_ms();
    let id = previewed(&b, "a.csv", &csv(0..3), "full", false).await;
    confirm(&b, &id).await;
    run_jobs(&h, &app).await;
    // A second import still running a year later keeps its file.
    let waiting = previewed(&b, "b.csv", &csv(0..2), "upsert", false).await;
    confirm(&b, &waiting).await;
    run(&h, "DELETE FROM job WHERE state = 'queued'").await;
    run(
        &h,
        "UPDATE roster_import SET state = 'running' WHERE state = 'queued'",
    )
    .await;
    let file_key = import_text(&h, &id, "file_key").await;
    let rows_key = format!("roster-rows/{id}.jsonl");
    // Its changes (#134): the whole list and the page's head.
    let diff_keys = [
        format!("roster-diff/{id}.jsonl"),
        format!("roster-diff/{id}.head.json"),
    ];
    assert!(
        diff_keys.iter().all(|k| h.files().keys().contains(k)),
        "stored at upload"
    );
    let tick = || app.deployment().tick(Timestamp(h.clock().now_ms()));

    h.clock().set_ms(uploaded_at + 365 * DAY_MS - 3_600_000);
    assert_eq!(tick().await.unwrap().roster_files_pruned, 0);
    assert!(
        h.files().keys().contains(&file_key),
        "kept until the year ends"
    );

    h.clock().advance_ms(2 * 3_600_000);
    let r = tick().await.unwrap();
    assert_eq!(r.roster_files_pruned, 1, "{:?}", r.errors);
    let keys = h.files().keys();
    assert!(
        !keys.contains(&file_key) && !keys.contains(&rows_key),
        "{keys:?}"
    );
    assert!(diff_keys.iter().all(|k| !keys.contains(k)), "{keys:?}");
    assert_eq!(import_text(&h, &id, "file_key").await, "");
    assert_eq!(import_text(&h, &id, "state").await, "done", "the row stays");
    let audit = "SELECT count(*) FROM audit_log WHERE action = 'roster.file.prune' \
                 AND actor_kind = 'system' AND target_id = ?";
    assert_eq!(int(&h, audit, &[&id]).await, 1);
    assert_ne!(import_text(&h, &waiting, "file_key").await, "");
    assert_eq!(tick().await.unwrap().roster_files_pruned, 0, "once");

    let v = signed_in(&h, &app, "Viewer").await;
    let page = v.get(&format!("/admin/roster/{id}")).await;
    assert!(page.body.contains("ลบไฟล์แล้ว"), "{}", page.body);
    assert!(!page.body.contains(&format!("/admin/roster/{id}/file\"")));
    assert_eq!(v.get(&format!("/admin/roster/{id}/file")).await.status, 404);
    // The changes are gone too: 410 with a Thai page, never part of them.
    let gone = v.get(&format!("/admin/roster/{id}/changes.csv")).await;
    assert_eq!(gone.status, 410);
    assert!(
        gone.body
            .contains("รายการเปลี่ยนแปลงของการนำเข้านี้ไม่มีให้ดาวน์โหลดแล้ว"),
        "{}",
        gone.body
    );
}

#[tokio::test]
async fn a_failed_upload_leaves_no_stored_object() {
    let (h, app) = boot().await;
    let b = signed_in(&h, &app, "Officer").await;
    run(
        &h,
        "CREATE TRIGGER boom BEFORE INSERT ON roster_import \
         BEGIN SELECT RAISE(ABORT, 'injected failure'); END",
    )
    .await;
    for (name, bytes) in [("a.csv", csv(0..3)), ("bad.csv", b"nope\r\n".to_vec())] {
        let r = upload(&b, name, &bytes, "upsert").await;
        assert_eq!(r.status, 500, "{name}: {}", r.body);
    }
    assert_eq!(h.files().keys(), Vec::<String>::new());
}

#[tokio::test]
async fn an_upload_that_loses_the_slot_leaves_no_stored_object() {
    let (h, app) = boot().await;
    let b = signed_in(&h, &app, "Officer").await;
    // Between the slot check and the insert, another import takes the
    // slot: the insert writes nothing and `Busy` names the other one.
    run(
        &h,
        "CREATE TRIGGER race BEFORE INSERT ON roster_import WHEN NEW.id <> 'other' \
         BEGIN INSERT INTO roster_import (id, staff_id, mode, state, file_key, file_name, \
         created_at) VALUES ('other', NEW.staff_id, 'upsert', 'queued', 'roster/other/x.csv', \
         'x.csv', NEW.created_at); SELECT RAISE(IGNORE); END",
    )
    .await;
    let r = upload(&b, "a.csv", &csv(0..3), "upsert").await;
    assert_eq!(r.status, 303, "{}", r.body);
    assert_eq!(flash(&b), "err:kernel.roster.busy");
    assert_eq!(r.header("location"), Some("/admin/roster/other"));
    assert_eq!(h.files().keys(), Vec::<String>::new());
}

#[tokio::test]
async fn the_fallback_refuses_a_file_that_now_reads_differently() {
    let (h, app) = boot().await;
    let b = signed_in(&h, &app, "Officer").await;
    let id = previewed(&b, "a.csv", &csv(0..3), "upsert", false).await;
    confirm(&b, &id).await;
    // Restored without the parsed rows, under a `[roster]` that still
    // accepts the file but reads `ชั้นปี` as a number (1.0, not 1).
    h.files()
        .delete(&format!("roster-rows/{id}.jsonl"))
        .await
        .unwrap();
    let changed = CONFIG.replace(
        "column = \"ชั้นปี\"\ntype = \"integer\"",
        "column = \"ชั้นปี\"\ntype = \"number\"",
    );
    assert_ne!(changed, CONFIG);
    let config = DeploymentConfig::from_toml(&changed).unwrap();
    let later = h.boot(h.builder().config(config).build().unwrap()).await;
    for _ in 0..5 {
        later
            .deployment()
            .tick(Timestamp(h.clock().now_ms()))
            .await
            .unwrap();
        h.clock().advance_minutes(31);
    }
    assert_eq!(import_text(&h, &id, "state").await, "failed");
    let error = import_text(&h, &id, "error").await;
    assert!(error.contains("no longer matches the preview"), "{error}");
    assert_eq!(int(&h, "SELECT count(*) FROM student", &[]).await, 0);
}

#[tokio::test]
async fn one_failing_object_does_not_hold_back_the_other_prunes() {
    let (h, app) = boot().await;
    let b = signed_in(&h, &app, "Officer").await;
    let start = h.clock().now_ms();
    let mut ids = Vec::new();
    for (name, rows) in [("a.csv", 0..3), ("b.csv", 0..4)] {
        let id = previewed(&b, name, &csv(rows), "upsert", false).await;
        confirm(&b, &id).await;
        run_jobs(&h, &app).await;
        ids.push(id);
    }
    // The oldest import's file cannot be deleted (ordered first).
    let stuck = import_text(&h, &ids[0], "file_key").await;
    h.files().fail_on(&stuck);
    h.clock().set_ms(start + 366 * DAY_MS);
    let r = app
        .deployment()
        .tick(Timestamp(h.clock().now_ms()))
        .await
        .unwrap();
    assert_eq!(r.roster_files_pruned, 1);
    assert_eq!(import_text(&h, &ids[1], "file_key").await, "");
    assert_eq!(import_text(&h, &ids[0], "file_key").await, stuck);
    assert!(
        r.errors.iter().any(|e| e.contains(&ids[0])),
        "{:?}",
        r.errors
    );
    let keys = "SELECT count(*) FROM audit_log WHERE action = 'roster.file.prune' \
                AND json_extract(after, '$.keys[0]') = ? AND json_extract(after, '$.keys[1]') = ?";
    let pruned_key = format!("roster/{}/b.csv", ids[1]);
    let rows_key = format!("roster-rows/{}.jsonl", ids[1]);
    assert_eq!(int(&h, keys, &[&pruned_key, &rows_key]).await, 1);
    assert!(r.to_string().ends_with("and 1 roster files"), "{r}");
}

#[tokio::test]
async fn a_preview_whose_file_was_pruned_cannot_be_confirmed() {
    let (h, app) = boot().await;
    let b = signed_in(&h, &app, "Officer").await;
    let id = previewed(&b, "a.csv", &csv(0..3), "upsert", false).await;
    h.clock().advance_ms(366 * DAY_MS);
    let r = app
        .deployment()
        .tick(Timestamp(h.clock().now_ms()))
        .await
        .unwrap();
    assert_eq!(r.roster_files_pruned, 1, "an abandoned preview too");
    h.create_staff("late@sealbay.example", PASSWORD, &["Officer"])
        .await;
    let late = app.browser();
    assert_eq!(
        late.login("late@sealbay.example", PASSWORD).await.status,
        303
    );
    confirm(&late, &id).await;
    assert_eq!(flash(&late), "err:kernel.roster.file_pruned");
    assert_eq!(import_text(&h, &id, "state").await, "previewed");
    assert_eq!(int(&h, "SELECT count(*) FROM job", &[]).await, 0);
}

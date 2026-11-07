//! Roster import follow-ups (#46) at the Deployment seam: chunks read the
//! parsed rows, stored files are pruned after a year, only the upload
//! route reads a large multipart body, and no object outlives a failed
//! upload.

mod roster_support;

use axum::body::Body;
use harpseal::{FileStore, Timestamp};
use harpseal_testing::{Browser, TestResponse};
use http::{Request, header};
use roster_support::*;

const DAY_MS: i64 = 86_400_000;

#[tokio::test]
async fn chunks_read_the_parsed_rows_not_the_original_file() {
    let (h, app) = boot().await;
    let b = signed_in(&h, &app, "Officer").await;
    let id = previewed(&b, "big.csv", &csv(0..1200), "upsert").await;
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
    let id = previewed(&b, "a.xlsx", XLSX_50, "upsert").await;
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
    let id = previewed(&b, "a.csv", &csv(0..3), "upsert").await;
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
    let id = previewed(&b, "a.csv", &csv(0..3), "full").await;
    confirm(&b, &id).await;
    run_jobs(&h, &app).await;
    // A second import still running a year later keeps its file.
    let waiting = previewed(&b, "b.csv", &csv(0..2), "upsert").await;
    confirm(&b, &waiting).await;
    run(&h, "DELETE FROM job WHERE state = 'queued'").await;
    run(
        &h,
        "UPDATE roster_import SET state = 'running' WHERE state = 'queued'",
    )
    .await;
    let file_key = import_text(&h, &id, "file_key").await;
    let rows_key = format!("roster-rows/{id}.jsonl");
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
}

/// A multipart POST to `path` with these `(name, value)` parts.
async fn multipart(b: &Browser<'_>, path: &str, parts: &[(&str, &[u8])]) -> TestResponse {
    let boundary = "----sealbay-boundary";
    let mut body = Vec::new();
    for (name, value) in parts {
        let head = format!("--{boundary}\r\nContent-Disposition: form-data; name=\"{name}\"");
        body.extend_from_slice(format!("{head}\r\n\r\n").as_bytes());
        body.extend_from_slice(value);
        body.extend_from_slice(b"\r\n");
    }
    body.extend_from_slice(format!("--{boundary}--\r\n").as_bytes());
    let req = Request::post(path)
        .header(
            header::CONTENT_TYPE,
            format!("multipart/form-data; boundary={boundary}"),
        )
        .body(Body::from(body))
        .unwrap();
    b.send(req).await
}

#[tokio::test]
async fn only_the_upload_route_reads_a_large_multipart_body() {
    let (h, app) = boot().await;
    let b = signed_in(&h, &app, "Officer").await;
    let token = b.csrf().unwrap();
    let padding = vec![b'x'; 200 * 1024];
    // Another back-office POST: 200 KiB is past the small generic limit,
    // so its `_csrf` part is never reached.
    let other = multipart(
        &b,
        "/admin/roster/none/cancel",
        &[("_csrf", token.as_bytes()), ("pad", &padding)],
    )
    .await;
    assert_eq!(other.status, 403, "{}", other.body);
    // The same size is fine on the upload route.
    let big = csv(0..3000);
    assert!(big.len() > 200 * 1024);
    let r = upload(&b, "big.csv", &big, "upsert").await;
    assert_eq!(r.status, 303, "{}", r.body);
    assert_eq!(flash(&b), "ok:kernel.roster.previewed");
}

#[tokio::test]
async fn staff_without_the_permission_are_refused_before_the_body_is_read() {
    let (h, app) = boot().await;
    let viewer = signed_in(&h, &app, "Viewer").await;
    // No `_csrf` part: a guard that read the body first would answer with
    // the CSRF refusal; the Permission check answers before that.
    let r = multipart(
        &viewer,
        "/admin/roster/upload",
        &[("mode", b"full"), ("file", &vec![b'x'; 1024 * 1024])],
    )
    .await;
    assert_eq!(r.status, 403);
    assert!(r.body.contains("kernel.roster.import"), "{}", r.body);
    assert_eq!(int(&h, "SELECT count(*) FROM roster_import", &[]).await, 0);
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

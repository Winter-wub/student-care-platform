//! A Module's back-office spreadsheet upload (#74): the roster's size
//! limit with 413 above it, the Permission and CSRF checks, the shared
//! reader on CSV and multi-sheet XLSX, and the kept file's life: confirm
//! (hash checked), cancel and the expiry sweep.

mod module_upload_support;

use harpseal::{CONFIRMED_KEEP_MS, MAX_UPLOAD_BYTES, Services, UPLOAD_KEEP_MS};
use module_upload_support::{
    Csrf, FEES_CSV, FEES_XLSX, PASSWORD, UPLOAD, audited, boot, job_sees, kept_id, kept_keys, send,
    signed_in, tick, upload_file, uploads,
};

/// A CSV of `n` bytes that reads (one header, padded rows).
fn big_csv(n: usize) -> Vec<u8> {
    let mut out = b"a,b\n".to_vec();
    while out.len() < n {
        out.extend_from_slice(b"1,xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx\n");
    }
    out.truncate(n);
    out
}

#[tokio::test]
async fn the_roster_size_is_accepted_and_more_is_413() {
    let (h, app) = boot().await;
    let b = signed_in(&h, &app, "Uploader").await;
    // Over the 64 KiB of other back-office forms, under 4 MiB: accepted.
    let r = upload_file(&b, "big.csv", &big_csv(200 * 1024)).await;
    kept_id(&r);
    // Just over the file limit, inside the body limit: the extractor.
    let r = upload_file(&b, "big.csv", &big_csv(MAX_UPLOAD_BYTES + 1)).await;
    assert_eq!(r.status, 413, "{}", r.body);
    // Over the body limit: the guard, before the handler runs.
    let r = upload_file(&b, "big.csv", &big_csv(MAX_UPLOAD_BYTES + 128 * 1024)).await;
    assert_eq!(r.status, 413, "{}", r.body);
    assert_eq!(uploads(&h).await, 1, "only the first one was kept");
    assert_eq!(kept_keys(&h).len(), 1);
}

#[tokio::test]
async fn staff_without_the_permission_get_403_and_nothing_is_kept() {
    let (h, app) = boot().await;
    let b = signed_in(&h, &app, "Viewer").await;
    let r = upload_file(&b, "fees.csv", FEES_CSV.as_bytes()).await;
    assert_eq!(r.status, 403, "{}", r.body);
    // Even a body over the limit: the Permission is checked first.
    let r = upload_file(&b, "big.csv", &big_csv(MAX_UPLOAD_BYTES + 128 * 1024)).await;
    assert_eq!(r.status, 403);
    let signed_out = app.browser();
    let r = upload_file(&signed_out, "fees.csv", FEES_CSV.as_bytes()).await;
    assert_eq!(r.status, 303, "to the sign-in page");
    assert_eq!(uploads(&h).await, 0);
    assert!(kept_keys(&h).is_empty());
}

#[tokio::test]
async fn the_csrf_token_is_required_as_a_part_or_a_header() {
    let (h, app) = boot().await;
    let b = signed_in(&h, &app, "Uploader").await;
    let r = send(&b, UPLOAD, "fees.csv", FEES_CSV.as_bytes(), Csrf::Missing).await;
    assert_eq!(r.status, 403, "{}", r.body);
    assert_eq!(uploads(&h).await, 0);
    let r = send(&b, UPLOAD, "fees.csv", FEES_CSV.as_bytes(), Csrf::Header).await;
    kept_id(&r);
    let r = send(&b, UPLOAD, "fees.csv", FEES_CSV.as_bytes(), Csrf::Part).await;
    kept_id(&r);
    assert!(r.body.ends_with("note=ภาคเรียน 2"), "{}", r.body);
}

#[tokio::test]
async fn csv_and_multi_sheet_xlsx_are_read_with_the_shared_reader() {
    let (h, app) = boot().await;
    let b = signed_in(&h, &app, "Uploader").await;
    let r = upload_file(&b, "fees.csv", FEES_CSV.as_bytes()).await;
    kept_id(&r);
    assert!(r.body.contains(" csv sheets= rows=2 "), "{}", r.body);
    let r = upload_file(&b, "fees_terms.xlsx", FEES_XLSX).await;
    kept_id(&r);
    assert!(
        r.body
            .contains(" xlsx sheets=ภาคเรียน 1|ภาคเรียน 2|lists(hidden) rows=2|2 "),
        "{}",
        r.body
    );
    let audited = h
        .db()
        .query(harpseal::Statement::new(
            "SELECT COUNT(*) AS n FROM audit_log WHERE action = 'module.upload'",
        ))
        .await
        .unwrap();
    assert_eq!(audited.get(0, "n").and_then(|v| v.as_i64()), Some(2));
}

#[tokio::test]
async fn a_file_that_is_not_csv_or_xlsx_by_its_bytes_is_refused() {
    let (h, app) = boot().await;
    let b = signed_in(&h, &app, "Uploader").await;
    let r = upload_file(&b, "fees.csv", FEES_XLSX).await;
    assert_eq!(r.status, 415, "a workbook renamed .csv");
    let r = upload_file(&b, "fees.xlsx", FEES_CSV.as_bytes()).await;
    assert_eq!(r.status, 415, "text renamed .xlsx");
    let r = upload_file(&b, "fees.pdf", b"%PDF-1.7").await;
    assert_eq!(r.status, 415);
    let r = upload_file(&b, "fees.csv", b"").await;
    assert_eq!(r.status, 400, "an empty file");
    assert_eq!(uploads(&h).await, 0);
}

#[tokio::test]
async fn confirm_reads_the_kept_file_and_refuses_one_whose_hash_changed() {
    let (h, app) = boot().await;
    let b = signed_in(&h, &app, "Uploader").await;
    let id = kept_id(&upload_file(&b, "fees_terms.xlsx", FEES_XLSX).await);
    let r = b.post(&format!("/admin/probe/{id}/confirm"), "").await;
    assert_eq!(r.status, 200, "{}", r.body);
    let keys = kept_keys(&h);
    assert_eq!(keys.len(), 1);
    assert!(keys[0].starts_with("module-upload/probe/"), "{keys:?}");
    // The object is swapped behind the record's back.
    let files: Services = h.services();
    let other = FEES_CSV.as_bytes().to_vec();
    files.files.put(&keys[0], other, "text/csv").await.unwrap();
    let r = b.post(&format!("/admin/probe/{id}/confirm"), "").await;
    assert_eq!(r.status, 409, "a changed file is never imported");
}

#[tokio::test]
async fn only_the_uploader_or_a_shared_permission_reaches_an_upload() {
    let (h, app) = boot().await;
    let mine = signed_in(&h, &app, "Uploader").await;
    let id = kept_id(&upload_file(&mine, "fees.csv", FEES_CSV.as_bytes()).await);
    // Same Permission, another Staff member: as if the id did not exist.
    let clerk = signed_in(&h, &app, "Clerk").await;
    assert_eq!(clerk.get("/admin").await.status, 200);
    let r = clerk.post(&format!("/admin/probe/{id}/confirm"), "").await;
    assert_eq!(r.status, 404);
    let r = clerk.post(&format!("/admin/probe/{id}/cancel"), "").await;
    assert_eq!(r.status, 404);
    assert_eq!(uploads(&h).await, 1, "still there");
    // Not confirmed: the import Job does not see it either.
    assert!(!job_sees(&h, &id).await);
    // The Module's override Permission (`Uploads::shared_with`).
    let boss = signed_in(&h, &app, "Supervisor").await;
    let r = boss.post(&format!("/admin/probe/{id}/confirm"), "").await;
    assert_eq!(r.body, "confirmed true");
    assert!(job_sees(&h, &id).await);
    let other = harpseal::KeptUpload::open_confirmed(&h.services(), "fees", &id).await;
    assert!(other.unwrap().is_none(), "another Module's name");
}

#[tokio::test]
async fn a_confirm_just_before_expiry_pins_the_upload_for_the_job() {
    let (h, app) = boot().await;
    let b = signed_in(&h, &app, "Uploader").await;
    let id = kept_id(&upload_file(&b, "fees.csv", FEES_CSV.as_bytes()).await);
    h.clock().advance_ms(UPLOAD_KEEP_MS - 60_000);
    let r = b.login("uploader@sealbay.example", PASSWORD).await;
    assert_eq!(r.status, 303);
    assert_eq!(b.get("/admin").await.status, 200);
    let r = b.post(&format!("/admin/probe/{id}/confirm"), "").await;
    assert_eq!(r.body, "confirmed true");
    let r = b.post(&format!("/admin/probe/{id}/confirm"), "").await;
    assert_eq!(r.body, "confirmed false", "pinned once");
    let r = b.post(&format!("/admin/probe/{id}/cancel"), "").await;
    assert_eq!(r.status, 404, "a confirmed upload belongs to its Job");
    assert_eq!(audited(&h, "module.upload.cancel").await, 0);
    assert_eq!(audited(&h, "module.upload.confirm").await, 1);
    // Past the 24 hours, and a sweep: the Job still finds it.
    h.clock().advance_ms(120_000);
    assert_eq!(tick(&h, &app).await.module_uploads_swept, 0);
    assert!(job_sees(&h, &id).await);
    assert!(
        harpseal::KeptUpload::finish(&h.services(), "probe", &id)
            .await
            .unwrap()
    );
    assert_eq!(uploads(&h).await, 0);
    assert!(kept_keys(&h).is_empty());
    // A confirmed upload the Job never finished goes after the week.
    let id = kept_id(&upload_file(&b, "fees.csv", FEES_CSV.as_bytes()).await);
    b.post(&format!("/admin/probe/{id}/confirm"), "").await;
    h.clock().advance_ms(CONFIRMED_KEEP_MS - 1);
    assert_eq!(tick(&h, &app).await.module_uploads_swept, 0);
    h.clock().advance_ms(1);
    assert_eq!(tick(&h, &app).await.module_uploads_swept, 1);
}

#[tokio::test]
async fn a_backup_leaves_kept_uploads_out() {
    let (h, app) = boot().await;
    let b = signed_in(&h, &app, "Uploader").await;
    let id = kept_id(&upload_file(&b, "fees.csv", FEES_CSV.as_bytes()).await);
    b.post(&format!("/admin/probe/{id}/confirm"), "").await;
    kept_id(&upload_file(&b, "fees_terms.xlsx", FEES_XLSX).await);
    let mut sink = harpseal::backup::MemoryBackup::new();
    let dumped = harpseal::backup::dump(app.deployment(), &mut sink)
        .await
        .unwrap();
    assert!(
        dumped
            .manifest
            .files
            .iter()
            .all(|f| !f.key.starts_with("module-upload/")),
        "{:?}",
        dumped.manifest.files
    );
    let rows = sink.entries.get("tables/module_upload.ndjson").unwrap();
    assert!(rows.is_empty(), "no kept upload row");
    assert_eq!(uploads(&h).await, 2, "the live ones are untouched");
}

#[tokio::test]
async fn the_header_token_and_a_non_utf8_csv_get_clear_refusals() {
    let (h, app) = boot().await;
    let b = signed_in(&h, &app, "Uploader").await;
    let big = big_csv(MAX_UPLOAD_BYTES + 128 * 1024);
    let r = send(&b, UPLOAD, "big.csv", &big, Csrf::Header).await;
    assert_eq!(r.status, 413, "{}", r.body);
    // "รหัส" in TIS-620.
    let r = upload_file(&b, "fees.csv", b"\xc3\xcb\xd1\xca,a\n1,2\n").await;
    assert_eq!(r.status, 415);
    assert!(r.body.contains("UTF-8"), "{}", r.body);
    assert_eq!(uploads(&h).await, 0);
}

#[tokio::test]
async fn cancel_deletes_the_file_and_its_record() {
    let (h, app) = boot().await;
    let b = signed_in(&h, &app, "Uploader").await;
    let id = kept_id(&upload_file(&b, "fees.csv", FEES_CSV.as_bytes()).await);
    assert_eq!(kept_keys(&h).len(), 1);
    let r = b.post(&format!("/admin/probe/{id}/cancel"), "").await;
    assert_eq!(r.status, 200);
    assert_eq!(audited(&h, "module.upload.cancel").await, 1);
    assert!(kept_keys(&h).is_empty());
    assert_eq!(uploads(&h).await, 0);
    let r = b.post(&format!("/admin/probe/{id}/confirm"), "").await;
    assert_eq!(r.status, 404, "nothing left to confirm");
    let r = b.post(&format!("/admin/probe/{id}/cancel"), "").await;
    assert_eq!(r.status, 404);
}

#[tokio::test]
async fn the_tick_deletes_expired_uploads_only() {
    let (h, app) = boot().await;
    let b = signed_in(&h, &app, "Uploader").await;
    let old = kept_id(&upload_file(&b, "fees.csv", FEES_CSV.as_bytes()).await);
    assert_eq!(tick(&h, &app).await.module_uploads_swept, 0);
    h.clock().advance_ms(UPLOAD_KEEP_MS - 60_000);
    // The Staff session ended meanwhile.
    let r = b.login("uploader@sealbay.example", PASSWORD).await;
    assert_eq!(r.status, 303);
    assert_eq!(b.get("/admin").await.status, 200, "a fresh CSRF token");
    let fresh = kept_id(&upload_file(&b, "fees_terms.xlsx", FEES_XLSX).await);
    assert_eq!(tick(&h, &app).await.module_uploads_swept, 0, "not yet");
    h.clock().advance_ms(60_000);
    // Expired: no longer opens, even before the sweep.
    let r = b.post(&format!("/admin/probe/{old}/confirm"), "").await;
    assert_eq!(r.status, 404);
    let report = tick(&h, &app).await;
    assert_eq!(report.module_uploads_swept, 1, "{:?}", report.errors);
    assert_eq!(uploads(&h).await, 1);
    let keys = kept_keys(&h);
    assert_eq!(keys.len(), 1);
    assert!(keys[0].contains(&fresh), "{keys:?}");
    assert_eq!(tick(&h, &app).await.module_uploads_swept, 0, "once");
    h.clock().advance_ms(UPLOAD_KEEP_MS);
    assert_eq!(tick(&h, &app).await.module_uploads_swept, 1);
    assert!(kept_keys(&h).is_empty());
}

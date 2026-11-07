//! The example's "Fees" Module (#74): a back-office spreadsheet upload
//! with a preview built by the shared reader, imported on confirm by a Job.

use axum::body::Body;
use axum::http::{Request, header};
use example_deployment::{Secrets, builder, fees};
use harpseal::{Statement, Timestamp};
use harpseal_testing::{Browser, Harness, TestApp, TestResponse};

const PASSWORD: &str = "kelp forest at dawn";
const FEES_XLSX: &[u8] =
    include_bytes!("../../../../crates/harpseal/tests/fixtures/roster/fees_terms.xlsx");
const FEES_CSV: &str = "ค่าธรรมเนียม 2/2569\n\
    รหัสนักศึกษา,ชื่อ-สกุล,ยอดค้างชำระ\n\
    68010001,นกน้อย ทะเลใส,\"12,500\"\n\
    68010002,ปลาทอง อ่าวคราม,800.50\n\
    รวม,,\"13,300.50\"\n";

fn secrets() -> Secrets {
    Secrets::from_lookup(|k| match k {
        "LINE_LIFF_ID" => Some("1234567890-AbCdEfGh".into()),
        "TELEGRAM_WEB_APP_URL" => Some("https://app.seal-bay.invalid/tg".into()),
        _ => None,
    })
}

async fn boot(h: &Harness) -> TestApp {
    let d = builder(h.services(), secrets())
        .channel(h.fake_channel())
        .build()
        .expect("the example Deployment builds");
    h.boot(d).await
}

async fn signed_in<'a>(h: &Harness, app: &'a TestApp, role: &str) -> Browser<'a> {
    let email = format!("{}@sealbay.example", role.to_lowercase());
    h.create_staff(&email, PASSWORD, &[role]).await;
    let b = app.browser();
    assert_eq!(b.login(&email, PASSWORD).await.status, 303);
    b
}

/// Opens the Fees page (for its CSRF token), then posts the file as the
/// form does.
async fn upload(b: &Browser<'_>, name: &str, bytes: &[u8]) -> TestResponse {
    let page = b.get(fees::BASE).await;
    assert_eq!(page.status, 200, "{}", page.body);
    let boundary = "----sealbay-fees";
    let mut body = format!(
        "--{boundary}\r\nContent-Disposition: form-data; name=\"_csrf\"\r\n\r\n{}\r\n\
         --{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"{name}\"\r\n\
         Content-Type: application/octet-stream\r\n\r\n",
        b.csrf().unwrap_or_default()
    )
    .into_bytes();
    body.extend_from_slice(bytes);
    body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());
    let req = Request::post(format!("{}/upload", fees::BASE))
        .header(
            header::CONTENT_TYPE,
            format!("multipart/form-data; boundary={boundary}"),
        )
        .body(Body::from(body))
        .unwrap();
    b.send(req).await
}

/// The preview path an upload redirected to.
fn preview_path(r: &TestResponse) -> String {
    assert_eq!(r.status, 303, "{}", r.body);
    r.header("location").expect("a Location").to_string()
}

async fn count(h: &Harness, sql: &str) -> i64 {
    let rows = h.db().query(Statement::new(sql.to_string())).await.unwrap();
    rows.get(0, "n").and_then(|v| v.as_i64()).unwrap_or(-1)
}

#[tokio::test]
async fn a_csv_is_previewed_then_imported_by_the_job_on_confirm() {
    let h = Harness::new();
    let app = boot(&h).await;
    let b = signed_in(&h, &app, "Administrator").await;
    let path = preview_path(&upload(&b, "fees.csv", FEES_CSV.as_bytes()).await);
    let page = b.get(&path).await;
    assert_eq!(page.status, 200);
    assert!(page.body.contains("12,500.00") && page.body.contains("800.50"));
    assert!(
        page.body.contains("แถว 5: รวม"),
        "the totals row is listed as skipped"
    );
    assert!(page.body.contains("/confirm"), "nothing blocks the import");
    let r = b.post(&format!("{path}/confirm"), "").await;
    assert_eq!(r.status, 303, "{}", r.body);
    assert_eq!(
        count(&h, "SELECT COUNT(*) AS n FROM fees_balance").await,
        0,
        "the Job does it"
    );
    let report = app
        .deployment()
        .tick(Timestamp(h.clock().now_ms()))
        .await
        .unwrap();
    assert_eq!(report.failed, 0, "{:?}", report.errors);
    let n = "SELECT COUNT(*) AS n FROM fees_balance WHERE \
             (student_code = '68010001' AND amount_satang = 1250000) OR \
             (student_code = '68010002' AND amount_satang = 80050)";
    assert_eq!(count(&h, n).await, 2);
    assert_eq!(
        count(&h, "SELECT COUNT(*) AS n FROM module_upload").await,
        0,
        "discarded"
    );
    assert!(
        h.files()
            .keys()
            .iter()
            .all(|k| !k.starts_with("module-upload/"))
    );
    let page = b.get(fees::BASE).await;
    assert!(
        page.body.contains("<strong id=\"stored\">2</strong>"),
        "{}",
        page.body
    );
    let audited = "SELECT COUNT(*) AS n FROM audit_log WHERE action = 'fees.import'";
    assert_eq!(count(&h, audited).await, 1);
}

#[tokio::test]
async fn a_multi_sheet_xlsx_preview_names_each_problem_and_cannot_be_confirmed() {
    let h = Harness::new();
    let app = boot(&h).await;
    let b = signed_in(&h, &app, "Administrator").await;
    let path = preview_path(&upload(&b, "fees_terms.xlsx", FEES_XLSX).await);
    let page = b.get(&path).await;
    assert_eq!(page.status, 200);
    let body = &page.body;
    // Rows from both visible sheets, named.
    assert!(
        body.contains("ภาคเรียน 1") && body.contains("ภาคเรียน 2"),
        "{body}"
    );
    assert!(body.contains("8,000.50") && body.contains("9,000.00"));
    assert!(body.contains("<span id=\"row-count\">3</span>"), "{body}");
    // The bad amount on sheet 2, row 3.
    assert!(
        body.contains("<td>ภาคเรียน 2</td><td>3</td><td>ยอดไม่ใช่ตัวเลข: &quot;12,5&quot;</td>"),
        "{body}"
    );
    assert!(body.contains("ภาคเรียน 1 แถว 5: รวม"), "the totals row");
    assert!(body.contains("ชีตที่ซ่อน: lists"));
    assert!(
        !body.contains("/confirm"),
        "a file with problems is not offered"
    );
    let r = b.post(&format!("{path}/confirm"), "").await;
    assert_eq!(r.status, 303, "back to the preview");
    let jobs = "SELECT COUNT(*) AS n FROM job WHERE kind = 'fees.import'";
    assert_eq!(count(&h, jobs).await, 0, "no Job was enqueued");
    assert_eq!(count(&h, "SELECT COUNT(*) AS n FROM fees_balance").await, 0);
    // Cancel deletes the kept file.
    assert_eq!(b.post(&format!("{path}/cancel"), "").await.status, 303);
    assert_eq!(
        count(&h, "SELECT COUNT(*) AS n FROM module_upload").await,
        0
    );
    assert_eq!(b.get(&path).await.status, 404);
}

#[tokio::test]
async fn the_page_needs_its_permission_and_the_upload_its_size() {
    let h = Harness::new();
    let app = boot(&h).await;
    let admin = signed_in(&h, &app, "Administrator").await;
    let big = vec![b'1'; harpseal::MAX_UPLOAD_BYTES + 1];
    let r = upload(&admin, "fees.csv", &big).await;
    assert_eq!(r.status, 413, "{}", r.body);
    let r = upload(&admin, "fees.pdf", b"%PDF-1.7").await;
    assert_eq!(r.status, 415);
    // A Staff member without `fees.import` (the Role exists only here).
    h.create_staff("viewer@sealbay.example", PASSWORD, &[])
        .await;
    let other = app.browser();
    assert_eq!(
        other.login("viewer@sealbay.example", PASSWORD).await.status,
        303
    );
    assert_eq!(other.get(fees::BASE).await.status, 403);
    assert_eq!(
        count(&h, "SELECT COUNT(*) AS n FROM module_upload").await,
        0
    );
}

#[tokio::test]
async fn a_confirm_a_minute_before_expiry_is_still_imported_after_it() {
    let h = Harness::new();
    let app = boot(&h).await;
    let b = signed_in(&h, &app, "Administrator").await;
    let path = preview_path(&upload(&b, "fees.csv", FEES_CSV.as_bytes()).await);
    h.clock().advance_ms(harpseal::UPLOAD_KEEP_MS - 60_000);
    assert_eq!(
        b.login("administrator@sealbay.example", PASSWORD)
            .await
            .status,
        303
    );
    assert_eq!(b.get(&path).await.status, 200, "a fresh token, still kept");
    assert_eq!(b.post(&format!("{path}/confirm"), "").await.status, 303);
    let page = b.get(&path).await;
    assert!(page.body.contains("id=\"queued\""), "confirmed once");
    // The Job runs only after the 24 hours are over.
    h.clock().advance_ms(120_000);
    let report = app
        .deployment()
        .tick(Timestamp(h.clock().now_ms()))
        .await
        .unwrap();
    assert_eq!(report.failed + report.retried, 0, "{:?}", report.errors);
    assert_eq!(count(&h, "SELECT COUNT(*) AS n FROM fees_balance").await, 2);
    assert_eq!(
        count(&h, "SELECT COUNT(*) AS n FROM module_upload").await,
        0
    );
}

#[tokio::test]
async fn a_confirmed_upload_lost_before_the_import_fails_the_job() {
    let h = Harness::new();
    let app = boot(&h).await;
    let b = signed_in(&h, &app, "Administrator").await;
    let path = preview_path(&upload(&b, "fees.csv", FEES_CSV.as_bytes()).await);
    assert_eq!(b.post(&format!("{path}/confirm"), "").await.status, 303);
    let id = path.rsplit('/').next().unwrap();
    // Lost behind the Job's back (an operator, a failed object store).
    assert!(
        harpseal::KeptUpload::finish(&h.services(), fees::NAME, id)
            .await
            .unwrap()
    );
    let report = app
        .deployment()
        .tick(Timestamp(h.clock().now_ms()))
        .await
        .unwrap();
    assert_eq!(report.retried, 1, "never reported as done");
    let failed = "SELECT COUNT(*) AS n FROM job WHERE kind = 'fees.import' \
                  AND state <> 'done' AND last_error LIKE '%nothing was imported%'";
    assert_eq!(count(&h, failed).await, 1);
    assert_eq!(count(&h, "SELECT COUNT(*) AS n FROM fees_balance").await, 0);
}

const NEWER_CSV: &str = "รหัสนักศึกษา,ชื่อ-สกุล,ยอดค้างชำระ\n68010001,นกน้อย ทะเลใส,999\n";

async fn tick(h: &Harness, app: &TestApp) -> harpseal::TickReport {
    app.deployment()
        .tick(Timestamp(h.clock().now_ms()))
        .await
        .unwrap()
}

#[tokio::test]
async fn cancel_after_confirm_leaves_the_import_to_its_job() {
    let h = Harness::new();
    let app = boot(&h).await;
    let b = signed_in(&h, &app, "Administrator").await;
    let path = preview_path(&upload(&b, "fees.csv", FEES_CSV.as_bytes()).await);
    // A stale tab still shows Cancel.
    let token = b.csrf().unwrap();
    assert_eq!(b.post(&format!("{path}/confirm"), "").await.status, 303);
    let r = b
        .post_raw(&format!("{path}/cancel"), &format!("_csrf={token}"))
        .await;
    assert_eq!(r.status, 303);
    assert_eq!(
        count(&h, "SELECT COUNT(*) AS n FROM module_upload").await,
        1
    );
    let report = tick(&h, &app).await;
    assert_eq!(report.failed + report.retried, 0, "{:?}", report.errors);
    assert_eq!(count(&h, "SELECT COUNT(*) AS n FROM fees_balance").await, 2);
}

#[tokio::test]
async fn a_retry_of_an_older_import_never_overwrites_a_newer_one() {
    let h = Harness::new();
    let app = boot(&h).await;
    let b = signed_in(&h, &app, "Administrator").await;
    // A: imported, but its file was not deleted (its `finish` failed).
    let a = preview_path(&upload(&b, "fees.csv", FEES_CSV.as_bytes()).await);
    assert_eq!(b.post(&format!("{a}/confirm"), "").await.status, 303);
    let a_id = a.rsplit('/').next().unwrap().to_string();
    let key = h
        .files()
        .keys()
        .into_iter()
        .find(|k| k.contains(&a_id))
        .unwrap();
    let bytes = harpseal::FileStore::get(h.files(), &key)
        .await
        .unwrap()
        .unwrap();
    let snap = "CREATE TABLE snap AS SELECT * FROM module_upload";
    h.db().execute(Statement::new(snap)).await.unwrap();
    assert_eq!(tick(&h, &app).await.failed, 0);
    let back = "INSERT INTO module_upload SELECT * FROM snap";
    h.db().execute(Statement::new(back)).await.unwrap();
    harpseal::FileStore::put(h.files(), &key, bytes, "text/csv")
        .await
        .unwrap();
    // B: newer balances, imported.
    let newer = preview_path(&upload(&b, "newer.csv", NEWER_CSV.as_bytes()).await);
    assert_eq!(b.post(&format!("{newer}/confirm"), "").await.status, 303);
    assert_eq!(tick(&h, &app).await.failed, 0);
    let b_amount = "SELECT COUNT(*) AS n FROM fees_balance \
                    WHERE student_code = '68010001' AND amount_satang = 99900";
    assert_eq!(count(&h, b_amount).await, 1);
    // A's retry runs after B: it only deletes A's file.
    let jobs = app.deployment().jobs();
    let payload = serde_json::json!({ "upload": a_id });
    jobs.enqueue(fees::JOB, payload, None).await.unwrap();
    let report = tick(&h, &app).await;
    assert_eq!(report.failed + report.retried, 0, "{:?}", report.errors);
    assert_eq!(count(&h, b_amount).await, 1, "B's balance stands");
    let left = "SELECT COUNT(*) AS n FROM module_upload";
    assert_eq!(count(&h, left).await, 0, "A's file is deleted");
}

#[tokio::test]
async fn a_failed_import_shows_on_the_preview() {
    let h = Harness::new();
    let app = boot(&h).await;
    let b = signed_in(&h, &app, "Administrator").await;
    let path = preview_path(&upload(&b, "fees.csv", FEES_CSV.as_bytes()).await);
    assert_eq!(b.post(&format!("{path}/confirm"), "").await.status, 303);
    assert!(b.get(&path).await.body.contains("id=\"queued\""));
    // Every write fails: the Job runs out of retries.
    let broken = "DROP TABLE fees_import";
    h.db().execute(Statement::new(broken)).await.unwrap();
    let mut failed = 0;
    for _ in 0..6 {
        failed += tick(&h, &app).await.failed;
        h.clock().advance_ms(31 * 60_000);
    }
    assert_eq!(failed, 1);
    assert_eq!(
        b.login("administrator@sealbay.example", PASSWORD)
            .await
            .status,
        303
    );
    let page = b.get(&path).await;
    assert!(page.body.contains("id=\"job-failed\""), "{}", page.body);
    assert!(!page.body.contains("id=\"queued\""));
}

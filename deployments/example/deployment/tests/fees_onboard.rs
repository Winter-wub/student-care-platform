//! The Fees page's help and next step (#127): the columns the Module
//! declares once give the panel beside the file input and the template;
//! after a confirm the result page offers "แจ้งนักศึกษา", a draft to
//! exactly the Students in the file.

use axum::body::Body;
use axum::http::{Request, header};
use example_deployment::{Secrets, builder, fees};
use harpseal::{Statement, Timestamp};
use harpseal_testing::{Browser, Harness, TestApp, TestResponse};

const PASSWORD: &str = "kelp forest at dawn";
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
        .unwrap()
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
async fn the_upload_form_has_the_declared_columns_and_template_beside_it() {
    let h = Harness::new();
    let app = boot(&h).await;
    let b = signed_in(&h, &app, "Finance").await;
    let page = b.get(fees::BASE).await.body;
    let at = page
        .find("<div class=\"cols\">")
        .expect("form and help side by side");
    let pair = &page[at..];
    assert!(
        pair.find("name=\"file\"") < pair.find("ไฟล์ที่ต้องเตรียม"),
        "{pair}"
    );
    for head in ["รหัสนักศึกษา", "ชื่อ-สกุล", "ยอดค้างชำระ"]
    {
        let span = format!("<span class=\"name code-head\">{head}</span>");
        assert!(pair.contains(&span), "{head}: {pair}");
    }
    assert!(pair.contains("12,500.00") && pair.contains("ทศนิยมไม่เกิน 2 ตำแหน่ง"));
    assert!(pair.contains("href=\"/admin/fees/upload/template.csv\" download"));
    let t = b.get("/admin/fees/upload/template.csv").await;
    assert_eq!(t.status, 200);
    assert_eq!(t.body, "\u{feff}รหัสนักศึกษา,ชื่อ-สกุล,ยอดค้างชำระ\r\n");
}

/// Runs the queued Jobs once.
async fn tick(h: &Harness, app: &TestApp) {
    let report = app
        .deployment()
        .tick(Timestamp(h.clock().now_ms()))
        .await
        .unwrap();
    assert_eq!(report.failed, 0, "{:?}", report.errors);
}

/// The next-step card, if any.
fn next_step(page: &str) -> Option<String> {
    let at = page.find("id=\"next-step\"")?;
    Some(page[at..at + page[at..].find("</section>").unwrap()].to_string())
}

#[tokio::test]
async fn the_result_survives_the_job_and_offers_the_next_step_only_once_imported() {
    let h = Harness::new();
    let app = boot(&h).await;
    let b = signed_in(&h, &app, "Administrator").await;
    let path = preview_path(&upload(&b, "fees.csv", FEES_CSV.as_bytes()).await);
    assert_eq!(
        next_step(&b.get(&path).await.body),
        None,
        "not before the confirm"
    );
    let r = b.post(&format!("{path}/confirm"), "").await;
    assert_eq!(r.status, 303, "{}", r.body);
    assert_eq!(r.header("location"), Some(path.as_str()), "the result page");
    // Queued: it says so, and offers nothing before the balances exist.
    let page = b.get(&path).await.body;
    assert!(
        page.contains("id=\"queued\"") && page.contains("id=\"pending\""),
        "{page}"
    );
    assert_eq!(next_step(&page), None, "{page}");
    // The Job imports and deletes the kept upload; the page still answers.
    tick(&h, &app).await;
    assert_eq!(
        count(&h, "SELECT COUNT(*) AS n FROM module_upload").await,
        0
    );
    let r = b.get(&path).await;
    assert_eq!(r.status, 200, "{}", r.body);
    assert!(r.body.contains("นำเข้ายอดค้างชำระ 2 รายการแล้ว"), "{}", r.body);
    let next = next_step(&r.body).expect("the next step once imported");
    let href = "/admin/announcements/new?scope=students&amp;students=68010001+68010002";
    assert!(
        next.contains(&format!("href=\"{href}\">แจ้งนักศึกษา</a>")),
        "{next}"
    );
    assert!(next.contains("นักศึกษา 2 คนในไฟล์นี้"), "{next}");
    assert!(
        next.contains(&format!("href=\"{}\">กลับไปหน้ายอดค้างชำระ</a>", fees::BASE)),
        "back: {next}"
    );
    // A reload later still answers the same.
    assert_eq!(b.get(&path).await.status, 200);
    // The link opens a draft with exactly those Students; nothing is sent.
    let form = b.get(&href.replace("&amp;", "&")).await.body;
    assert!(form.contains(">68010001\n68010002</textarea>"), "{form}");
    let none = "SELECT COUNT(*) AS n FROM announcement";
    assert_eq!(count(&h, none).await, 0);
}

#[tokio::test]
async fn a_later_file_replacing_some_balances_leaves_the_audience_to_staff() {
    let h = Harness::new();
    let app = boot(&h).await;
    let b = signed_in(&h, &app, "Administrator").await;
    let first = preview_path(&upload(&b, "fees.csv", FEES_CSV.as_bytes()).await);
    b.post(&format!("{first}/confirm"), "").await;
    tick(&h, &app).await;
    let later = "รหัสนักศึกษา,ชื่อ-สกุล,ยอดค้างชำระ\n68010002,ปลาทอง อ่าวคราม,100\n";
    let second = preview_path(&upload(&b, "later.csv", later.as_bytes()).await);
    b.post(&format!("{second}/confirm"), "").await;
    tick(&h, &app).await;
    let page = b.get(&first).await.body;
    assert!(page.contains("id=\"superseded\""), "{page}");
    let next = next_step(&page).expect("the next step");
    assert!(next.contains("href=\"/admin/announcements/new\""), "{next}");
}

#[tokio::test]
async fn finance_without_announcements_gets_only_the_way_back() {
    let h = Harness::new();
    let app = boot(&h).await;
    let b = signed_in(&h, &app, "Finance").await;
    let path = preview_path(&upload(&b, "fees.csv", FEES_CSV.as_bytes()).await);
    b.post(&format!("{path}/confirm"), "").await;
    tick(&h, &app).await;
    let next = next_step(&b.get(&path).await.body).expect("the next step");
    assert!(!next.contains("/admin/announcements/new"), "{next}");
    assert!(next.contains(&format!("href=\"{}\"", fees::BASE)), "{next}");
}

#[tokio::test]
async fn the_result_follows_the_uploader_rule() {
    let h = Harness::new();
    let app = boot(&h).await;
    let admin = signed_in(&h, &app, "Administrator").await;
    let path = preview_path(&upload(&admin, "fees.csv", FEES_CSV.as_bytes()).await);
    admin.post(&format!("{path}/confirm"), "").await;
    tick(&h, &app).await;
    assert_eq!(admin.get(&path).await.status, 200);
    // Another Staff member with the same Permission: as before the Job,
    // the upload is not theirs to see.
    let finance = signed_in(&h, &app, "Finance").await;
    let r = finance.get(&path).await;
    assert_eq!(r.status, 404, "{}", r.body);
    assert_eq!(next_step(&r.body), None);
}

#[tokio::test]
async fn a_failed_job_whose_upload_is_gone_says_so_to_its_uploader_only() {
    let h = Harness::new();
    let app = boot(&h).await;
    let admin = signed_in(&h, &app, "Administrator").await;
    let path = preview_path(&upload(&admin, "fees.csv", FEES_CSV.as_bytes()).await);
    assert_eq!(admin.post(&format!("{path}/confirm"), "").await.status, 303);
    let id = path.rsplit('/').next().unwrap();
    // Lost behind the Job's back: it retries, then fails for good.
    assert!(
        harpseal::KeptUpload::finish(&h.services(), fees::NAME, id)
            .await
            .unwrap()
    );
    let mut failed = 0;
    for _ in 0..6 {
        let report = app
            .deployment()
            .tick(Timestamp(h.clock().now_ms()))
            .await
            .unwrap();
        failed += report.failed;
        h.clock().advance_ms(31 * 60_000);
    }
    assert_eq!(failed, 1, "failed for good");
    // Sessions outlive the retries: sign in again.
    assert_eq!(
        admin
            .login("administrator@sealbay.example", PASSWORD)
            .await
            .status,
        303
    );
    let r = admin.get(&path).await;
    assert_eq!(r.status, 409, "{}", r.body);
    assert!(r.body.contains("นำเข้าไม่สำเร็จ"), "{}", r.body);
    assert_eq!(next_step(&r.body), None);
    let finance = signed_in(&h, &app, "Finance").await;
    let r = finance.get(&path).await;
    assert_eq!(r.status, 404, "not theirs: {}", r.body);
}

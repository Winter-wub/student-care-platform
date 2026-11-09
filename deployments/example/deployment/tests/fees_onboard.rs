//! The Fees page's help and next step (#127): the columns the Module
//! declares once give the panel beside the file input and the template;
//! after a confirm the result page offers "แจ้งนักศึกษา", a draft to
//! exactly the Students in the file.

use axum::body::Body;
use axum::http::{Request, header};
use example_deployment::{Secrets, builder, fees};
use harpseal::Statement;
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

#[tokio::test]
async fn a_confirmed_upload_offers_a_draft_to_the_students_in_the_file() {
    let h = Harness::new();
    let app = boot(&h).await;
    let b = signed_in(&h, &app, "Administrator").await;
    let path = preview_path(&upload(&b, "fees.csv", FEES_CSV.as_bytes()).await);
    let before = b.get(&path).await.body;
    assert!(
        !before.contains("id=\"next-step\""),
        "not before the confirm"
    );
    let r = b.post(&format!("{path}/confirm"), "").await;
    assert_eq!(r.status, 303, "{}", r.body);
    assert_eq!(r.header("location"), Some(path.as_str()), "the result page");
    let page = b.get(&path).await.body;
    assert!(page.contains("id=\"queued\""), "{page}");
    let at = page.find("id=\"next-step\"").expect("the next step");
    let next = &page[at..at + page[at..].find("</section>").unwrap()];
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
    // The link opens a draft with exactly those Students; nothing is sent.
    let form = b.get(&href.replace("&amp;", "&")).await.body;
    assert!(form.contains(">68010001\n68010002</textarea>"), "{form}");
    let none = "SELECT COUNT(*) AS n FROM announcement";
    assert_eq!(count(&h, none).await, 0);
}

#[tokio::test]
async fn finance_without_announcements_gets_only_the_way_back() {
    let h = Harness::new();
    let app = boot(&h).await;
    let b = signed_in(&h, &app, "Finance").await;
    let path = preview_path(&upload(&b, "fees.csv", FEES_CSV.as_bytes()).await);
    b.post(&format!("{path}/confirm"), "").await;
    let page = b.get(&path).await.body;
    let at = page.find("id=\"next-step\"").expect("the next step");
    let next = &page[at..at + page[at..].find("</section>").unwrap()];
    assert!(!next.contains("/admin/announcements/new"), "{next}");
    assert!(next.contains(&format!("href=\"{}\"", fees::BASE)), "{next}");
}

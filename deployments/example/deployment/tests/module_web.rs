//! What a Module outside the Core writes its pages with (#162), through
//! the example's Fees Module: `harpseal::web::axum` instead of an `axum`
//! dependency (this crate has none), the Core's one-shot flash after a
//! POST, and failures the runtime logs as it logs the Core's own.

// axum at the Core's version (#162): this crate has no axum dependency.
use harpseal::web::axum;

use example_deployment::{Secrets, builder, fees};
use harpseal::Statement;
use harpseal_testing::{Browser, Harness, TestApp, TestResponse};

use axum::body::Body;
use axum::http::{Request, header};

const PASSWORD: &str = "kelp forest at dawn";
const FEES_CSV: &str = "รหัสนักศึกษา,ชื่อ-สกุล,ยอดค้างชำระ\n68010001,นกน้อย ทะเลใส,\"12,500\"\n";
const CANCELLED: &str = "ยกเลิกแล้ว ลบไฟล์ที่อัปโหลดออกแล้ว";

fn secrets() -> Secrets {
    Secrets::from_lookup(|k| match k {
        "LINE_LIFF_ID" => Some("1234567890-AbCdEfGh".into()),
        "TELEGRAM_WEB_APP_URL" => Some("https://app.seal-bay.invalid/tg".into()),
        _ => None,
    })
}

async fn run(h: &Harness, sql: &str) {
    h.db()
        .execute(Statement::new(sql.to_string()))
        .await
        .unwrap();
}

async fn boot(h: &Harness) -> TestApp {
    let d = builder(h.services(), secrets())
        .unwrap()
        .dev(true)
        .channel(h.fake_channel())
        .build()
        .expect("the example Deployment builds");
    h.boot(d).await
}

async fn signed_in<'a>(h: &Harness, app: &'a TestApp) -> Browser<'a> {
    let email = "finance@sealbay.example";
    h.create_staff(email, PASSWORD, &["Administrator"]).await;
    let b = app.browser();
    assert_eq!(b.login(email, PASSWORD).await.status, 303);
    b
}

/// Uploads the CSV as the form does and opens its preview (for the
/// CSRF token); answers the preview's path.
async fn preview(b: &Browser<'_>) -> String {
    assert_eq!(b.get(fees::BASE).await.status, 200);
    let boundary = "----sealbay-web";
    let body = format!(
        "--{boundary}\r\nContent-Disposition: form-data; name=\"_csrf\"\r\n\r\n{}\r\n\
         --{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"fees.csv\"\r\n\
         Content-Type: text/csv\r\n\r\n{FEES_CSV}\r\n--{boundary}--\r\n",
        b.csrf().unwrap_or_default()
    );
    let req = Request::post(format!("{}/upload", fees::BASE))
        .header(
            header::CONTENT_TYPE,
            format!("multipart/form-data; boundary={boundary}"),
        )
        .body(Body::from(body))
        .unwrap();
    let r = b.send(req).await;
    assert_eq!(r.status, 303, "{}", r.body);
    let path = r.header("location").expect("a Location").to_string();
    assert_eq!(b.get(&path).await.status, 200);
    path
}

/// The flash cookie the response sets, if any.
fn flash_cookie(r: &TestResponse) -> Option<String> {
    r.headers
        .get_all(header::SET_COOKIE)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .find(|v| v.starts_with("hs_flash=ok:"))
        .map(str::to_string)
}

#[tokio::test]
async fn a_module_handler_sets_a_flash_the_next_page_shows_once() {
    let h = Harness::new();
    let app = boot(&h).await;
    let b = signed_in(&h, &app).await;
    let path = preview(&b).await;
    let r = b.post(&format!("{path}/cancel"), "").await;
    assert_eq!(r.status, 303, "{}", r.body);
    assert_eq!(r.header("location"), Some(fees::BASE));
    let cookie = flash_cookie(&r).expect("the Core's flash cookie, not a ?done=");
    assert!(
        cookie.starts_with("hs_flash=ok:fees.cancelled;"),
        "{cookie}"
    );
    for flag in ["HttpOnly", "SameSite=Lax", "Path=/admin"] {
        assert!(cookie.contains(flag), "{flag}: {cookie}");
    }
    let next = b.get(fees::BASE).await;
    assert!(
        next.body.contains(&format!(
            "<div class=\"flash ok\" role=\"status\">{CANCELLED}</div>"
        )),
        "{}",
        next.body
    );
    let again = b.get(fees::BASE).await;
    assert!(!again.body.contains(CANCELLED), "shown once");
    // Cancelling it a second time: nothing left to cancel, a red flash.
    let r = b.post(&format!("{path}/cancel"), "").await;
    assert_eq!(r.status, 303);
    let next = b.get(fees::BASE).await.body;
    assert!(next.contains("class=\"flash err\""), "{next}");
}

#[tokio::test]
async fn an_htmx_post_gets_hx_redirect_with_the_same_flash() {
    let h = Harness::new();
    let app = boot(&h).await;
    let b = signed_in(&h, &app).await;
    let path = preview(&b).await;
    let req = Request::post(format!("{path}/cancel"))
        .header("hx-request", "true")
        .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
        .body(Body::from(format!("_csrf={}", b.csrf().unwrap())))
        .unwrap();
    let r = b.send(req).await;
    assert_eq!(r.status, 200, "{}", r.body);
    assert_eq!(r.header("hx-redirect"), Some(fees::BASE));
    assert_eq!(r.header("location"), None);
    assert!(flash_cookie(&r).is_some());
    let next = b.get(fees::BASE).await.body;
    assert!(next.contains(CANCELLED), "{next}");
}

#[tokio::test]
async fn a_module_back_office_failure_is_logged_like_a_core_one() {
    let h = Harness::new();
    let app = boot(&h).await;
    let b = signed_in(&h, &app).await;
    let path = preview(&b).await;
    run(&h, "DROP TABLE module_upload").await;
    let r = b.post(&format!("{path}/cancel"), "").await;
    assert_eq!(r.status, 500, "{}", r.body);
    let report = r.error_report.expect("a report the runtime logs");
    assert_eq!(
        (report.area, report.kind, report.route),
        ("fees", "db", Some("/fees/{id}/cancel"))
    );
    assert_eq!(report.to_string(), "fees: db at /admin/fees/{id}/cancel");
    assert!(r.header("x-request-id").is_some(), "the log line's request");
    assert!(
        !r.body.contains("module_upload"),
        "the page never shows the error"
    );
}

#[tokio::test]
async fn a_module_mini_app_failure_is_logged_too() {
    let h = Harness::new();
    let app = boot(&h).await;
    for sql in [
        "INSERT INTO student (id, student_code, display_name, locale, attributes, created_at, \
         updated_at) VALUES ('s1', '68010001', 'Karn Seal', 'th', '{}', 1, 1)",
        "INSERT INTO channel_account (id, channel, external_id, first_seen_at, flow_state, \
         updated_at) VALUES ('a-s1', 'fake', 'u-s1', 1, 'linked', 1)",
        "INSERT INTO link (id, student_id, channel_account_id, channel, status, method, \
         linked_at, updated_at) VALUES ('l-s1', 's1', 'a-s1', 'fake', 'active', 'email_otp', 1, 1)",
    ] {
        run(&h, sql).await;
    }
    let student = app.browser();
    let r = student
        .open_mini_app(&h.fake_channel(), "u-s1", fees::student::PATH)
        .await;
    assert_eq!(r.status, 303, "{}", r.body);
    run(&h, "DROP TABLE fees_balance").await;
    let r = student.get(fees::student::PATH).await;
    assert_eq!(r.status, 500);
    let report = r.error_report.expect("a report the runtime logs");
    // The logged line is the area and the kind only: neither the
    // database's message nor the Student's code.
    let line = report.to_string();
    assert_eq!(line, "fees.student: db");
    let all = format!("{line} {report:?}");
    assert!(!all.contains("fees_balance") && !all.contains("68010001"));
}

//! Mini App (#28) exchange with the real adapters at the HTTP seam: a LIFF
//! ID token checked by LINE's verify endpoint (canned answers on the
//! harness's `RecordingHttp`) and a Telegram `initData` checked by HMAC
//! against the harness clock. The vectors are the adapters' own:
//! `harpseal-channel-line/tests/liff.rs` and `web_app.rs`'s known vector.

use harpseal::{Deployment, EmailOtp, Statement};
use harpseal_channel_line::{LineChannel, LineConfig};
use harpseal_channel_telegram::{TelegramChannel, TelegramConfig};
use harpseal_testing::{Harness, TestApp};

const LIFF: &str = "2000000000-SealBay0";
const LINE_USER: &str = "U5ea1ba7000000000000000000000c0de";
const ID_TOKEN: &str = "eyJhbGciOiJFUzI1NiJ9.eyJzdWIiOiJVNWVhMWJhNyJ9.c2lnbmF0dXJl";
/// From `web_app.rs`: valid at the harness start (auth_date 1793836800).
const BOT_TOKEN: &str = "8000000001:AAFsealBayTestTokenNotReal0000000000";
const INIT_DATA: &str = "auth_date=1793836800&query_id=AAHdF6IQAAAAAN0XohDhrOrc\
    &user=%7B%22id%22%3A7012345678%2C%22first_name%22%3A%22Karn%22%2C%22language_code%22%3A%22th%22%7D\
    &hash=2e9f10864e5d25e3ed0571951dab659b75948576b076b8d92fc2b059b003b1b1";
const TG_USER: &str = "7012345678";

async fn run(h: &Harness, sql: &str) {
    h.db()
        .execute(Statement::new(sql.to_string()))
        .await
        .unwrap();
}

async fn boot() -> (Harness, TestApp) {
    let h = Harness::new();
    let s = h.services();
    let line = LineChannel::new(
        LineConfig::new("line-secret", "line-token", LIFF),
        s.http.clone(),
    );
    let tg = TelegramChannel::new(
        TelegramConfig::new(BOT_TOKEN, "tg-secret", "https://sealbay.example"),
        s.http.clone(),
    )
    .with_clock(s.clock.clone());
    let d = Deployment::builder()
        .config(Harness::config())
        .runtime(s)
        .channel(line)
        .channel(tg)
        .verification(EmailOtp::new(Harness::config().otp))
        .build()
        .unwrap();
    let app = h.boot(d).await;
    run(
        &h,
        "INSERT INTO student (id, student_code, display_name, locale, created_at, updated_at) \
         VALUES ('s1', '6801001', 'Karn Seal', 'th', 1, 1)",
    )
    .await;
    for (kind, ext) in [("line", LINE_USER), ("telegram", TG_USER)] {
        run(
            &h,
            &format!(
                "INSERT INTO channel_account (id, channel, external_id, first_seen_at, \
                 flow_state, updated_at) VALUES ('a-{kind}', '{kind}', '{ext}', 1, 'linked', 1)"
            ),
        )
        .await;
        run(
            &h,
            &format!(
                "INSERT INTO link (id, student_id, channel_account_id, channel, status, method, \
                 linked_at, updated_at) VALUES ('l-{kind}', 's1', 'a-{kind}', '{kind}', 'active', \
                 'email_otp', 1, 1)"
            ),
        )
        .await;
    }
    (h, app)
}

fn claims(aud: &str) -> String {
    format!(
        r#"{{"iss":"https://access.line.me","sub":"{LINE_USER}","aud":"{aud}","exp":1793840400,"iat":1793836800}}"#
    )
}

async fn refused_details(app: &TestApp) -> String {
    let rows = app
        .query("SELECT group_concat(details, ' ') FROM security_event WHERE kind = 'app.session_refused'")
        .await;
    rows.get(0, "group_concat(details, ' ')")
        .and_then(|v| v.as_text())
        .unwrap_or_default()
        .to_string()
}

#[tokio::test]
async fn the_entry_page_loads_both_platform_scripts_with_the_liff_id() {
    let (_h, app) = boot().await;
    let page = app.get("/app").await.body;
    assert!(page.contains("src=\"https://static.line-scdn.net/liff/edge/2/sdk.js\""));
    assert!(page.contains("src=\"https://telegram.org/js/telegram-web-app.js\""));
    assert!(page.contains(&format!("data-liff=\"{LIFF}\"")));
    assert!(page.contains("data-line=\"line\""));
    assert!(page.contains("data-tg=\"telegram\""));
    // No other external script than these and the pinned HTMX.
    assert_eq!(page.matches("<script src=").count(), 3, "{page}");
}

#[tokio::test]
async fn a_liff_id_token_line_accepts_opens_the_students_page() {
    let (h, app) = boot().await;
    h.http().respond_with(200, claims("2000000000"));
    let b = app.browser();
    let r = b.open_mini_app("line", ID_TOKEN, "/app").await;
    assert_eq!(r.status, 303, "{}", r.body);
    let reqs = h.http_requests();
    assert_eq!(reqs.len(), 1);
    assert_eq!(reqs[0].uri, "https://api.line.me/oauth2/v2.1/verify");
    let body = String::from_utf8_lossy(&reqs[0].body).to_string();
    assert!(body.contains("client_id=2000000000"), "{body}");
    let page = b.get("/app").await.body;
    assert!(page.contains("Karn Seal") && page.contains("6801001"));
    assert!(page.contains("เชื่อมต่อผ่าน LINE"));
}

#[tokio::test]
async fn a_liff_id_token_line_refuses_is_a_403_and_never_stored() {
    let (h, app) = boot().await;
    h.http().respond_with(
        400,
        r#"{"error":"invalid_request","error_description":"IdToken expired."}"#,
    );
    let r = app.browser().open_mini_app("line", ID_TOKEN, "/app").await;
    assert_eq!(r.status, 403);
    assert!(!r.body.contains(ID_TOKEN));
    let details = refused_details(&app).await;
    assert!(details.contains("\"channel\":\"line\""), "{details}");
    assert!(!details.contains(ID_TOKEN));
    // LINE down is a 500 without details, and no Security Event.
    h.http().respond_with(503, "upstream down");
    let down = app.browser().open_mini_app("line", ID_TOKEN, "/app").await;
    assert_eq!(down.status, 500);
    assert!(!down.body.contains("upstream"));
}

#[tokio::test]
async fn telegram_init_data_is_checked_by_hash_and_age() {
    let (h, app) = boot().await;
    let b = app.browser();
    let r = b.open_mini_app("telegram", INIT_DATA, "/app").await;
    assert_eq!(r.status, 303, "{}", r.body);
    let page = b.get("/app").await.body;
    assert!(page.contains("Karn Seal") && page.contains("เชื่อมต่อผ่าน Telegram"));

    let tampered = INIT_DATA.replace("7012345678", "7012345679");
    let r = app
        .browser()
        .open_mini_app("telegram", &tampered, "/app")
        .await;
    assert_eq!(r.status, 403);
    // An hour and a bit later the same initData is stale.
    h.clock().advance_minutes(61);
    let r = app
        .browser()
        .open_mini_app("telegram", INIT_DATA, "/app")
        .await;
    assert_eq!(r.status, 403);
    let details = refused_details(&app).await;
    assert_eq!(
        details.matches("\"channel\":\"telegram\"").count(),
        2,
        "{details}"
    );
    assert!(!details.contains("hash") && !details.contains("auth_date"));
    assert!(h.http_requests().is_empty());
}

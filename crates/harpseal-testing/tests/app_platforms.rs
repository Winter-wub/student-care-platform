//! Mini App (#28) exchange with the real adapters at the HTTP seam: a LIFF
//! ID token checked by LINE's verify endpoint (canned answers on the
//! harness's `RecordingHttp`) and a Telegram `initData` checked by HMAC
//! against the harness clock. The vectors are the adapters' own:
//! `harpseal-channel-line/tests/liff.rs` and `web_app.rs`'s known vector.
//! The scripts come from each adapter's `MiniApp::web_client` (#52), so a
//! LINE MINI App URL loads LIFF too.

use harpseal::{Deployment, EmailOtp, Statement};
use harpseal_channel_line::{LINE_MINI_APP_BASE, LineChannel, LineConfig};
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
    boot_with(LineConfig::new("line-secret", "line-token", LIFF)).await
}

async fn boot_with(line: LineConfig) -> (Harness, TestApp) {
    let h = Harness::new();
    let s = h.services();
    let line = LineChannel::new(line, s.http.clone());
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
async fn a_line_mini_app_url_loads_the_liff_sdk_with_its_id() {
    let cfg =
        LineConfig::new("line-secret", "line-token", LIFF).with_launch_base(LINE_MINI_APP_BASE);
    let (h, app) = boot_with(cfg).await;
    let line = app.deployment().channel("line").unwrap();
    let launch = line.mini_app().unwrap().launch_url("/app");
    assert_eq!(launch, format!("https://miniapp.line.me/{LIFF}/app"));
    let page = app.get("/app").await.body;
    assert!(page.contains("src=\"https://static.line-scdn.net/liff/edge/2/sdk.js\""));
    assert!(page.contains(&format!("data-liff=\"{LIFF}\"")));
    assert!(page.contains("data-line=\"line\""));
    // The LINE Login button for a browser outside LINE.
    assert!(page.contains("id=\"xchg-line\""));
    // And its exchange is the LIFF one: limited and verified with LINE.
    h.http().respond_with(200, claims("2000000000"));
    let r = app
        .browser()
        .open_mini_app_raw("line", ID_TOKEN, "/app")
        .await;
    assert_eq!(r.status, 303, "{}", r.body);
    assert_eq!(h.http_requests().len(), 1);
}

#[tokio::test]
async fn telegram_web_in_an_iframe_gets_a_partitioned_cookie() {
    let (h, app) = boot().await;
    let post = |channel: &str, token: &str, framed: &str| {
        http::Request::post("/app/session")
            .header("content-type", "application/x-www-form-urlencoded")
            .body(axum::body::Body::from(format!(
                "channel={channel}&token={}&next=%2Fapp&framed={framed}",
                token
                    .replace('%', "%25")
                    .replace('&', "%26")
                    .replace('=', "%3D")
            )))
            .unwrap()
    };
    let framed = app.request(post("telegram", INIT_DATA, "1")).await;
    assert_eq!(framed.status, 303, "{}", framed.body);
    let cookie = framed.header("set-cookie").unwrap();
    assert!(
        cookie.contains("SameSite=None; Secure; Partitioned"),
        "{cookie}"
    );
    // In the app (top-level webview): Lax.
    let top = app.request(post("telegram", INIT_DATA, "")).await;
    let cookie = top.header("set-cookie").unwrap();
    assert!(cookie.contains("SameSite=Lax; Secure") && !cookie.contains("Partitioned"));
    // Only a Telegram Web App Channel gets it: LIFF never runs framed.
    h.http().respond_with(200, claims("2000000000"));
    let line = app.request(post("line", ID_TOKEN, "1")).await;
    assert_eq!(line.status, 303, "{}", line.body);
    let cookie = line.header("set-cookie").unwrap();
    assert!(cookie.contains("SameSite=Lax") && !cookie.contains("Partitioned"));
    // Without a session the entry page deletes both forms of the cookie.
    let req = http::Request::get("/app")
        .header("cookie", "app_session=gone")
        .body(axum::body::Body::empty())
        .unwrap();
    let cleared = app.request(req).await;
    let all: Vec<_> = cleared.headers.get_all("set-cookie").iter().collect();
    assert_eq!(all.len(), 2, "{all:?}");
    assert!(
        all.iter()
            .all(|c| c.to_str().unwrap().contains("Max-Age=0"))
    );
    assert!(
        all.iter()
            .any(|c| c.to_str().unwrap().contains("Partitioned"))
    );
}

#[tokio::test]
async fn a_liff_id_token_line_accepts_opens_the_students_page() {
    let (h, app) = boot().await;
    h.http().respond_with(200, claims("2000000000"));
    let b = app.browser();
    let r = b.open_mini_app_raw("line", ID_TOKEN, "/app").await;
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
    let r = app
        .browser()
        .open_mini_app_raw("line", ID_TOKEN, "/app")
        .await;
    assert_eq!(r.status, 403);
    assert!(!r.body.contains(ID_TOKEN));
    let details = refused_details(&app).await;
    assert!(details.contains("\"channel\":\"line\""), "{details}");
    assert!(!details.contains(ID_TOKEN));
    // LINE down is a 500 without details, and no Security Event.
    h.http().respond_with(503, "upstream down");
    let down = app
        .browser()
        .open_mini_app_raw("line", ID_TOKEN, "/app")
        .await;
    assert_eq!(down.status, 500);
    assert!(!down.body.contains("upstream"));
    // The runtime logs the kind only, never the message or the token.
    let report = down.error_report.expect("the 500 is reported").to_string();
    assert_eq!(report, "app.exchange: platform");
}

#[tokio::test]
async fn telegram_init_data_is_checked_by_hash_and_age() {
    let (h, app) = boot().await;
    let b = app.browser();
    let r = b.open_mini_app_raw("telegram", INIT_DATA, "/app").await;
    assert_eq!(r.status, 303, "{}", r.body);
    let page = b.get("/app").await.body;
    assert!(page.contains("Karn Seal") && page.contains("เชื่อมต่อผ่าน Telegram"));

    let tampered = INIT_DATA.replace("7012345678", "7012345679");
    let r = app
        .browser()
        .open_mini_app_raw("telegram", &tampered, "/app")
        .await;
    assert_eq!(r.status, 403);
    let details = refused_details(&app).await;
    assert_eq!(
        details.matches("\"channel\":\"telegram\"").count(),
        1,
        "{details}"
    );
    assert!(!details.contains("hash") && !details.contains("auth_date"));

    // An hour and a bit later the same (correctly signed) initData is
    // stale, as it is in a webview left open: when the session has expired
    // too, the entry page's exchange lands on "open it again from the
    // chat", with no retry button and no Security Event.
    h.clock().advance_minutes(61);
    assert!(b.get("/app").await.body.contains("id=\"xchg\""));
    let r = b.open_mini_app_raw("telegram", INIT_DATA, "/app").await;
    assert_eq!(r.status, 401);
    assert!(r.body.contains("กรุณาปิดแล้วเปิดใหม่จากปุ่มในแชท"), "{}", r.body);
    assert!(!r.body.contains("ลองอีกครั้ง"));
    assert_eq!(refused_details(&app).await, details);
    assert!(h.http_requests().is_empty());
}

#[tokio::test]
async fn line_verify_calls_are_limited_per_ip_group_per_minute() {
    let (h, app) = boot().await;
    let from = |ip: &'static str| {
        http::Request::post("/app/session")
            .header("content-type", "application/x-www-form-urlencoded")
            .header("cf-connecting-ip", ip)
            .body(axum::body::Body::from(format!(
                "channel=line&token={ID_TOKEN}&next=%2Fapp"
            )))
            .unwrap()
    };
    // The canned `200 {}` is no valid claim: each call is a refusal, and
    // every call counts.
    for i in 0..30 {
        let ip = if i % 2 == 0 {
            "2001:db8:aa:bb::1"
        } else {
            "2001:db8:aa:bb::2"
        };
        assert_eq!(app.request(from(ip)).await.status, 403, "call {i}");
    }
    assert_eq!(h.http_requests().len(), 30);
    // Same /64: limited, and LINE is not called.
    let limited = app.request(from("2001:db8:aa:bb::3")).await;
    assert_eq!(limited.status, 429);
    assert_eq!(h.http_requests().len(), 30);
    // Another group is not.
    assert_eq!(app.request(from("203.0.113.9")).await.status, 403);
    // One refusal event per group per minute.
    let details = refused_details(&app).await;
    assert_eq!(details.matches("\"ip\"").count(), 2, "{details}");
    // The next minute the group may try again.
    h.clock().advance_ms(60_000);
    assert_eq!(app.request(from("2001:db8:aa:bb::1")).await.status, 403);
}

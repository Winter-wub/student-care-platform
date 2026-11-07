//! A Module's own Mini App page (#72) at the HTTP seam, with the real
//! LINE (LIFF) and Telegram (Web App) adapters: the page's handler takes
//! `AppStudent`, the Module lists it on the Mini App home with
//! `mini_app_link`. Vectors as in `app_platforms.rs`.
//!
//! Two Students: Karn (`s1`) opens through LINE, Mali (`s2`) through
//! Telegram.

use axum::extract::State;
use axum::response::Response;
use axum::routing::get;
use harpseal::{
    AppState, AppStudent, BuildError, CatalogEntry, Deployment, EmailOtp, Module, ModuleRegistrar,
    Statement,
};
use harpseal_channel_line::{LineChannel, LineConfig};
use harpseal_channel_telegram::{TelegramChannel, TelegramConfig};
use harpseal_testing::{Browser, Harness, TestApp, TestResponse};

const LIFF: &str = "2000000000-SealBay0";
const LINE_USER: &str = "U5ea1ba7000000000000000000000c0de";
const ID_TOKEN: &str = "eyJhbGciOiJFUzI1NiJ9.eyJzdWIiOiJVNWVhMWJhNyJ9.c2lnbmF0dXJl";
const BOT_TOKEN: &str = "8000000001:AAFsealBayTestTokenNotReal0000000000";
const INIT_DATA: &str = "auth_date=1793836800&query_id=AAHdF6IQAAAAAN0XohDhrOrc\
    &user=%7B%22id%22%3A7012345678%2C%22first_name%22%3A%22Karn%22%2C%22language_code%22%3A%22th%22%7D\
    &hash=2e9f10864e5d25e3ed0571951dab659b75948576b076b8d92fc2b059b003b1b1";
const TG_USER: &str = "7012345678";

/// The page under test.
const PAGE: &str = "/app/probe";

/// A Module with one Mini App page that prints what `AppStudent` gives,
/// and two home links registered out of order.
struct Probe;

const TEXTS: &[CatalogEntry] = &[
    CatalogEntry::new("probe.page", "th", "หน้าทดสอบ"),
    CatalogEntry::new("probe.page", "en", "Probe page"),
    CatalogEntry::new("probe.first", "th", "ลิงก์แรก"),
    CatalogEntry::new("probe.first", "en", "First link"),
];

impl Module for Probe {
    fn name(&self) -> &'static str {
        "probe"
    }
    fn register(&self, r: &mut ModuleRegistrar) {
        r.messages(TEXTS)
            .public(PAGE, get(probe))
            .public("/app/probe/first", get(probe))
            .mini_app_link("probe.page", PAGE, 30)
            .mini_app_link("probe.first", "/app/probe/first", 20);
    }
}

async fn probe(State(s): State<AppState>, me: AppStudent) -> Response {
    let year = me.attributes.get("year").map(ToString::to_string);
    let body = maud::html! {
        p id="who" {
            (me.id.0) "|" (me.student_code) "|" (me.display_name) "|"
            (me.locale.as_str()) "|" (me.channel) "|" (year.unwrap_or_default())
        }
        p id="t" { (me.t("probe.page")) }
    };
    me.page(&s, &me.t("probe.page"), body)
}

async fn run(h: &Harness, sql: &str) {
    h.db()
        .execute(Statement::new(sql.to_string()))
        .await
        .unwrap();
}

fn builder(h: &Harness) -> harpseal::DeploymentBuilder {
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
    Deployment::builder()
        .config(Harness::config())
        .runtime(s)
        .channel(line)
        .channel(tg)
        .verification(EmailOtp::new(Harness::config().otp))
}

/// Karn (`s1`, Thai) linked on LINE, Mali (`s2`, English) on Telegram.
async fn boot() -> (Harness, TestApp) {
    let h = Harness::new();
    let app = h.boot(builder(&h).module(Probe).build().unwrap()).await;
    for (id, code, name, locale, year) in [
        ("s1", "6801001", "Karn Seal", "th", 1),
        ("s2", "6801002", "Mali Tern", "en", 3),
    ] {
        run(
            &h,
            &format!(
                "INSERT INTO student (id, student_code, display_name, locale, attributes, \
                 created_at, updated_at) VALUES ('{id}', '{code}', '{name}', '{locale}', \
                 '{{\"year\":{year}}}', 1, 1)"
            ),
        )
        .await;
    }
    for (kind, ext, student) in [("line", LINE_USER, "s1"), ("telegram", TG_USER, "s2")] {
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
                 linked_at, updated_at) VALUES ('l-{kind}', '{student}', 'a-{kind}', '{kind}', \
                 'active', 'email_otp', 1, 1)"
            ),
        )
        .await;
    }
    (h, app)
}

/// A browser signed in through `channel`'s real platform token, landing
/// on [`PAGE`].
async fn signed_in<'a>(h: &Harness, app: &'a TestApp, channel: &str) -> Browser<'a> {
    let b = app.browser();
    let token = if channel == "line" {
        h.http().respond_with(
            200,
            format!(
                r#"{{"iss":"https://access.line.me","sub":"{LINE_USER}","aud":"2000000000","exp":1793840400,"iat":1793836800}}"#
            ),
        );
        ID_TOKEN
    } else {
        INIT_DATA
    };
    let r = b.open_mini_app_raw(channel, token, PAGE).await;
    assert_eq!(r.status, 303, "{}", r.body);
    assert_eq!(r.header("location"), Some(PAGE));
    b
}

fn who(r: &TestResponse) -> &str {
    let at = r.body.find("<p id=\"who\">").expect("the probe page") + 12;
    let end = r.body[at..].find("</p>").unwrap();
    &r.body[at..at + end]
}

fn is_entry(r: &TestResponse) -> bool {
    r.status == 200 && r.body.contains("id=\"xchg\"")
}

#[tokio::test]
async fn line_and_telegram_sessions_reach_their_own_student() {
    let (h, app) = boot().await;
    let line = signed_in(&h, &app, "line").await;
    let r = line.get(PAGE).await;
    assert_eq!(r.status, 200);
    assert_eq!(who(&r), "s1|6801001|Karn Seal|th|line|1");
    assert!(r.body.contains("หน้าทดสอบ"), "Thai texts for Karn");
    // The Mini App's headers: CSP, never cached, no referrer.
    assert_eq!(
        r.header("content-security-policy"),
        Some("frame-ancestors 'self' https://web.telegram.org")
    );
    assert_eq!(r.header("cache-control"), Some("no-store"));
    assert_eq!(r.header("referrer-policy"), Some("no-referrer"));

    let tg = signed_in(&h, &app, "telegram").await;
    let r = tg.get(PAGE).await;
    assert_eq!(who(&r), "s2|6801002|Mali Tern|en|telegram|3");
    assert!(r.body.contains("Probe page"), "English texts for Mali");
}

#[tokio::test]
async fn telegram_web_s_partitioned_cookie_reaches_the_module_page() {
    let (_h, app) = boot().await;
    let req = http::Request::post("/app/session")
        .header("content-type", "application/x-www-form-urlencoded")
        .body(axum::body::Body::from(format!(
            "channel=telegram&token={}&next=%2Fapp%2Fprobe&framed=1",
            harpseal_testing::urlencode(INIT_DATA)
        )))
        .unwrap();
    let r = app.request(req).await;
    let cookie = r.header("set-cookie").unwrap().to_string();
    assert!(cookie.contains("Path=/app;") && cookie.contains("Partitioned"));
    let token = cookie.split(';').next().unwrap();
    let req = http::Request::get(PAGE)
        .header("cookie", token)
        .body(axum::body::Body::empty())
        .unwrap();
    assert!(who(&app.request(req).await).starts_with("s2|"));
}

#[tokio::test]
async fn without_a_session_the_page_is_the_entry_page_back_to_itself() {
    let (_h, app) = boot().await;
    let r = app.get(PAGE).await;
    assert!(is_entry(&r), "{}", r.body);
    assert!(r.body.contains("name=\"next\" value=\"/app/probe\""));
    assert!(r.header("content-security-policy").is_some());
    // A forged cookie: the same, and the cookie is cleared.
    let req = http::Request::get(PAGE)
        .header("cookie", "app_session=00ff00ff")
        .body(axum::body::Body::empty())
        .unwrap();
    let r = app.request(req).await;
    assert!(is_entry(&r));
    assert!(r.header("set-cookie").unwrap().contains("Max-Age=0"));
}

#[tokio::test]
async fn an_expired_session_goes_back_through_the_sign_in() {
    let (h, app) = boot().await;
    for channel in ["line", "telegram"] {
        let b = signed_in(&h, &app, channel).await;
        h.clock().advance_minutes(61);
        let r = b.get(PAGE).await;
        assert!(is_entry(&r), "{channel}: {}", r.body);
        assert!(!r.body.contains("id=\"who\""));
        h.clock().advance_minutes(-61);
    }
}

#[tokio::test]
async fn a_revoked_link_gets_the_not_linked_page() {
    let (h, app) = boot().await;
    let b = signed_in(&h, &app, "line").await;
    run(&h, "UPDATE link SET status = 'revoked' WHERE id = 'l-line'").await;
    let r = b.get(PAGE).await;
    assert_eq!(r.status, 200);
    assert!(r.body.contains("ยังไม่ได้เชื่อมกับรหัสนักศึกษา"), "{}", r.body);
    assert!(!r.body.contains("Karn Seal"));
}

#[tokio::test]
async fn no_url_or_query_picks_another_student_only_the_cookie_does() {
    let (h, app) = boot().await;
    let karn = signed_in(&h, &app, "line").await;
    for path in [
        "/app/probe?student=s2",
        "/app/probe?id=s2&student_code=6801002",
        "/app/probe/first?student=s2",
    ] {
        let r = karn.get(path).await;
        assert!(who(&r).starts_with("s1|"), "{path}");
        assert!(!r.body.contains("Mali Tern"), "{path}");
    }
    assert_eq!(karn.get("/app/probe/s2").await.status, 404);
    // Mali's cookie is Mali: the page follows the session, never mixes.
    let mali = signed_in(&h, &app, "telegram").await;
    let cookie = mali.cookie("app_session").unwrap();
    let req = http::Request::get("/app/probe?student=s1")
        .header("cookie", format!("app_session={cookie}"))
        .body(axum::body::Body::empty())
        .unwrap();
    let r = app.request(req).await;
    assert!(who(&r).starts_with("s2|") && !r.body.contains("Karn Seal"));
}

#[tokio::test]
async fn the_home_lists_module_links_in_order() {
    let (h, app) = boot().await;
    let b = signed_in(&h, &app, "line").await;
    let home = b.get("/app").await.body;
    let first = home.find("href=\"/app/probe/first\"").expect("first link");
    let page = home.find("href=\"/app/probe\"").expect("page link");
    assert!(first < page, "order 20 before 30");
    assert!(home.contains(">ลิงก์แรก</a>") && home.contains(">หน้าทดสอบ</a>"));
    // Mali reads them in English.
    let tg = signed_in(&h, &app, "telegram").await;
    assert!(tg.get("/app").await.body.contains(">First link</a>"));
}

struct Stray(&'static str, &'static str);

impl Module for Stray {
    fn name(&self) -> &'static str {
        "stray"
    }
    fn register(&self, r: &mut ModuleRegistrar) {
        r.messages(TEXTS)
            .public(self.0, get(probe))
            .mini_app_link("probe.page", self.1, 1);
    }
}

#[test]
fn build_refuses_a_link_outside_app_or_to_a_page_it_does_not_own() {
    for (route, link) in [
        ("/my-info", "/my-info"),
        ("/app/stray", "/app/elsewhere"),
        ("/app/stray", PAGE),
    ] {
        let h = Harness::new();
        let err = builder(&h)
            .module(Probe)
            .module(Stray(route, link))
            .build()
            .unwrap_err();
        assert!(
            matches!(
                err,
                BuildError::InvalidRoute {
                    module: "stray",
                    ..
                }
            ),
            "{link}: {err}"
        );
    }
}

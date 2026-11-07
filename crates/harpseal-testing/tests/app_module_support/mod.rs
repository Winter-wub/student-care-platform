//! Setup for a Module's own Mini App page (#72) at the HTTP seam, with the
//! real LINE (LIFF) and Telegram (Web App) adapters. Vectors as in
//! `app_platforms.rs`.
//!
//! Two Students: Karn (`s1`) opens through LINE, Mali (`s2`) through
//! Telegram. `[roster.attributes]`: `year` is `student_visible`, `advisor`
//! is not; the stored `legacy` key is no longer configured.

#![allow(dead_code)]

use axum::extract::State;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use harpseal::{
    AppForm, AppState, AppStudent, CatalogEntry, Deployment, DeploymentConfig, EmailOtp, Module,
    ModuleRegistrar, Statement,
};
use harpseal_channel_line::{LineChannel, LineConfig};
use harpseal_channel_telegram::{TelegramChannel, TelegramConfig};
use harpseal_testing::{Browser, Harness, TEST_CONFIG, TestApp, TestResponse};

pub const LIFF: &str = "2000000000-SealBay0";
pub const LINE_USER: &str = "U5ea1ba7000000000000000000000c0de";
pub const ID_TOKEN: &str = "eyJhbGciOiJFUzI1NiJ9.eyJzdWIiOiJVNWVhMWJhNyJ9.c2lnbmF0dXJl";
pub const BOT_TOKEN: &str = "8000000001:AAFsealBayTestTokenNotReal0000000000";
pub const INIT_DATA: &str = "auth_date=1793836800&query_id=AAHdF6IQAAAAAN0XohDhrOrc\
    &user=%7B%22id%22%3A7012345678%2C%22first_name%22%3A%22Karn%22%2C%22language_code%22%3A%22th%22%7D\
    &hash=2e9f10864e5d25e3ed0571951dab659b75948576b076b8d92fc2b059b003b1b1";
pub const TG_USER: &str = "7012345678";

/// The page under test.
pub const PAGE: &str = "/app/probe";
/// `POST`: takes `AppStudent` alone (the token must come as a header).
pub const NOTE: &str = "/app/probe/note";
/// `POST`: takes `AppForm` (the token may come as the `_csrf` field).
pub const FORM: &str = "/app/probe/form";
/// `GET`: a bare text response, not `AppStudent::page`.
pub const RAW: &str = "/app/probe/raw";

const ROSTER: &str = r#"
[roster.attributes.year]
column = "ชั้นปี"
type = "integer"
student_visible = true

[roster.attributes.advisor]
column = "อาจารย์ที่ปรึกษา"
type = "text"
optional = true
"#;

/// A Module with one Mini App page that prints what `AppStudent` gives,
/// two home links registered out of order, and two `POST` routes.
pub struct Probe;

pub const TEXTS: &[CatalogEntry] = &[
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
            .public(NOTE, post(note))
            .public(FORM, post(form))
            .public(RAW, get(raw))
            .mini_app_link("probe.page", PAGE, 30)
            .mini_app_link("probe.first", "/app/probe/first", 20);
    }
}

pub async fn probe(State(s): State<AppState>, me: AppStudent) -> Response {
    let attrs = serde_json::to_string(&me.attributes).unwrap();
    let body = maud::html! {
        p id="who" {
            (me.id.0) "|" (me.student_code) "|" (me.display_name) "|"
            (me.locale.as_str()) "|" (me.channel) "|" (attrs)
        }
        p id="t" { (me.t("probe.page")) }
        form method="post" action=(FORM) {
            input type="hidden" name="_csrf" value=(me.csrf_token());
            input name="text";
        }
    };
    me.page(&s, &me.t("probe.page"), body)
}

async fn note(me: AppStudent) -> String {
    format!("saved:{}", me.id.0)
}

#[derive(serde::Deserialize)]
struct Note {
    text: String,
}

async fn form(f: AppForm<Note>) -> String {
    format!("form:{}:{}", f.student.id.0, f.form.text)
}

async fn raw(me: AppStudent) -> Response {
    me.id.0.into_response()
}

pub async fn run(h: &Harness, sql: &str) {
    h.db()
        .execute(Statement::new(sql.to_string()))
        .await
        .unwrap();
}

pub fn config() -> DeploymentConfig {
    DeploymentConfig::from_toml(&format!("{TEST_CONFIG}{ROSTER}")).unwrap()
}

pub fn builder(h: &Harness) -> harpseal::DeploymentBuilder {
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
        .config(config())
        .runtime(s)
        .channel(line)
        .channel(tg)
        .verification(EmailOtp::new(config().otp))
}

/// Karn (`s1`, Thai) linked on LINE, Mali (`s2`, English) on Telegram.
pub async fn boot() -> (Harness, TestApp) {
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
                 '{{\"year\":{year},\"advisor\":\"Dr. Kelp\",\"legacy\":\"x\"}}', 1, 1)"
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
pub async fn signed_in<'a>(h: &Harness, app: &'a TestApp, channel: &str) -> Browser<'a> {
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

pub fn who(r: &TestResponse) -> &str {
    let at = r.body.find("<p id=\"who\">").expect("the probe page") + 12;
    let end = r.body[at..].find("</p>").unwrap();
    &r.body[at..at + end]
}

pub fn is_entry(r: &TestResponse) -> bool {
    r.status == 200 && r.body.contains("id=\"xchg\"")
}

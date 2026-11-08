//! Setup for the Announcements tests (#25): a Deployment with two Fake
//! Channels (`fake` LineLike, with multicast; `tg` TelegramLike, without),
//! the Announcements Module, a roster schema and Roles, linked Students,
//! signed-in browsers and a tick loop.

#![allow(dead_code)]

pub mod notice;

use axum::body::Body;
use harpseal::modules::announcements::Announcements;
use harpseal::{
    ActionId, Deployment, DeploymentConfig, EmailOtp, Inbound, Message, ReplyToken, Statement,
    Timestamp,
};
use harpseal_channel_fake::{FakeChannel, FakeProfile, SIGNATURE_HEADER};
use harpseal_testing::{Browser, Harness, TestApp, urlencode};
use http::Request;

pub const PASSWORD: &str = "kelp forest at dawn";

pub const CONFIG: &str = r#"
name = "Seal Bay University (announcements)"
default_locale = "th"
utc_offset = "+07:00"

[roles]
Officer = ["kernel.student.view", "announcements.view", "announcements.send", "announcements.retract"]
Viewer = ["announcements.view"]
Sender = ["announcements.view", "announcements.send"]

[roster.attributes.programme]
column = "หลักสูตร"
type = "enum"
values = ["MSC", "MSC-Intl", "Fisheries"]
label = { th = "หลักสูตร", en = "Programme" }

[roster.attributes.year]
column = "ชั้นปี"
type = "integer"
label = { th = "ชั้นปี", en = "Year" }
"#;

/// The Telegram-like Channel's kind and secret.
pub const TG: &str = "tg";
const TG_SECRET: &str = "tg-secret";

pub struct World {
    pub h: Harness,
    pub app: TestApp,
    /// LineLike, kind `fake` (the harness's), multicast.
    pub line: FakeChannel,
    /// TelegramLike, kind `tg`, edit and delete.
    pub tg: FakeChannel,
}

/// Boots the Deployment; `max_recipients` sets the LineLike multicast
/// group size.
pub async fn boot_with(max_recipients: usize) -> World {
    boot_config(CONFIG, max_recipients).await
}

/// [`boot_with`] on the `deployment.toml` `src` (e.g. [`CONFIG`] plus a
/// section).
pub async fn boot_config(src: &str, max_recipients: usize) -> World {
    boot_line(src, max_recipients, |line| line).await
}

/// [`boot_config`] with the LineLike Channel registered as `wrap(line)`
/// (e.g. a wrapper answering a repeated key as LINE does); `World::line`
/// stays the inner Fake.
pub async fn boot_line<C: harpseal::Channel + 'static>(
    src: &str,
    max_recipients: usize,
    wrap: impl FnOnce(FakeChannel) -> C,
) -> World {
    let h = Harness::new().with_fake_profile(FakeProfile::LineLike);
    let line = h.fake_channel().with_max_recipients(max_recipients);
    let tg = FakeChannel::new(TG_SECRET)
        .unwrap()
        .telegram_like()
        .with_kind(TG);
    let config = DeploymentConfig::from_toml(src).unwrap();
    let d = Deployment::builder()
        .config(config.clone())
        .runtime(h.services())
        .channel(wrap(line.clone()))
        .channel(tg.clone())
        .verification(EmailOtp::new(config.otp))
        .module(Announcements)
        .build()
        .unwrap();
    let app = h.boot(d).await;
    World { h, app, line, tg }
}

pub async fn boot() -> World {
    boot_with(500).await
}

/// Runs one SQL statement on the harness database.
pub async fn run(h: &Harness, sql: &str) {
    h.db()
        .execute(Statement::new(sql.to_string()))
        .await
        .unwrap();
}

/// A Student: `status` active|left, `locale`, attributes JSON.
pub async fn student(h: &Harness, id: &str, locale: &str, status: &str, attrs: &str) {
    run(
        h,
        &format!(
            "INSERT INTO student (id, student_code, display_name, email, locale, status, \
             attributes, created_at, updated_at) VALUES ('{id}', 'code-{id}', 'Name {id}', NULL, \
             '{locale}', '{status}', '{attrs}', 1, 1)"
        ),
    )
    .await;
}

/// A Tag on a Student.
pub async fn tag(h: &Harness, student: &str, name: &str) {
    run(
        h,
        &format!(
            "INSERT OR IGNORE INTO tag (id, name, created_at) VALUES ('t-{name}', '{name}', 1)"
        ),
    )
    .await;
    run(
        h,
        &format!(
            "INSERT INTO student_tag (student_id, tag_id, source, added_at) \
             VALUES ('{student}', 't-{name}', 'roster', 1)"
        ),
    )
    .await;
}

/// The external id of `student` on `channel`.
pub fn ext(channel: &str, student: &str) -> String {
    format!("{channel}-{student}")
}

/// A linked account of `student` on `channel` with Consent; `status`
/// active|unreachable|revoked.
pub async fn link(h: &Harness, student: &str, channel: &str, status: &str) {
    let ext = ext(channel, student);
    run(
        h,
        &format!(
            "INSERT INTO channel_account (id, channel, external_id, first_seen_at, flow_state, \
             updated_at) VALUES ('a-{ext}', '{channel}', '{ext}', 1, 'linked', 1)"
        ),
    )
    .await;
    run(
        h,
        &format!(
            "INSERT OR IGNORE INTO consent (id, student_id, channel_account_id, notice_version, \
             locale, accepted_at) VALUES ('c-{ext}', '{student}', 'a-{ext}', '1', 'th', 1)"
        ),
    )
    .await;
    run(
        h,
        &format!(
            "INSERT INTO link (id, student_id, channel_account_id, channel, status, method, \
             linked_at, updated_at) VALUES ('l-{ext}', '{student}', 'a-{ext}', '{channel}', \
             '{status}', 'email_otp', 1, 1)"
        ),
    )
    .await;
}

/// The standard roster: s1 (th) on both Channels, s3 (th) on `fake`,
/// s4 (en) on `tg`, s2 left (its old Link still active, to prove `left`
/// is excluded).
pub async fn roster(w: &World) {
    let h = &w.h;
    student(h, "s1", "th", "active", r#"{"programme":"MSC","year":2}"#).await;
    student(h, "s2", "th", "left", r#"{"programme":"MSC","year":2}"#).await;
    student(
        h,
        "s3",
        "th",
        "active",
        r#"{"programme":"MSC-Intl","year":2}"#,
    )
    .await;
    student(h, "s4", "en", "active", r#"{"programme":"MSC","year":3}"#).await;
    link(h, "s1", "fake", "active").await;
    link(h, "s1", TG, "active").await;
    link(h, "s2", "fake", "active").await;
    link(h, "s3", "fake", "active").await;
    link(h, "s4", TG, "active").await;
}

/// A signed-in browser holding `role` (the CSRF token loaded).
pub async fn signed_in<'a>(w: &'a World, role: &str) -> Browser<'a> {
    let email = format!("{}@sealbay.example", role.to_lowercase());
    w.h.create_staff(&email, PASSWORD, &[role]).await;
    let b = w.app.browser();
    let r = b.login(&email, PASSWORD).await;
    assert_eq!(r.status, 303, "{}", r.body);
    assert_eq!(b.get("/admin/announcements").await.status, 200);
    b
}

/// The create form, urlencoded, plus `extra` (e.g. `&do=confirm`).
pub fn form(title: &str, body: &str, extra: &str) -> String {
    format!(
        "title_th={}&body_th={}{extra}",
        urlencode(title),
        urlencode(body)
    )
}

/// `form` with the Audience choice (#120) its filters imply, unless it
/// names one: `groups` with an attribute or Tag, `students` with a list,
/// else `everyone`. Groups and a list together stay without a choice, as
/// an older page posts them (the Audience ANDs them; a choice would keep
/// only its own filters).
pub fn with_scope(form: &str) -> String {
    if form.split('&').any(|p| p.starts_with("scope=")) {
        return form.to_string();
    }
    let filled = |p: &str| p.split_once('=').is_some_and(|(_, v)| !v.is_empty());
    let has = |k: &str| form.split('&').any(|p| p.starts_with(k) && filled(p));
    let groups = has("attr.") || has("tags=");
    if groups && has("students=") {
        return form.to_string();
    }
    let scope = if groups {
        "groups"
    } else if has("students=") {
        "students"
    } else {
        "everyone"
    };
    format!("{form}&scope={scope}")
}

/// The hidden fields of the review step's confirm form (#120),
/// form-encoded: version, `reviewed` and, to everyone, `reach`.
pub async fn review_fields(b: &Browser<'_>, id: &str) -> String {
    let page = b.get(&format!("/admin/announcements/{id}/review")).await;
    assert_eq!(page.status, 200, "{}", page.body);
    let form = page
        .body
        .split("<form")
        .find(|f| f.contains("/confirm\""))
        .unwrap_or_else(|| panic!("no confirm form: {}", page.body));
    let form = &form[..form.find("</form>").unwrap_or(form.len())];
    form.split("<input type=\"hidden\" ")
        .skip(1)
        .filter_map(|i| {
            let attr = |a: &str| {
                let at = i.find(&format!("{a}=\""))? + a.len() + 2;
                Some(i[at..at + i[at..].find('"')?].to_string())
            };
            Some(format!("{}={}", attr("name")?, attr("value")?))
        })
        .filter(|p| !p.starts_with("_csrf="))
        .collect::<Vec<_>>()
        .join("&")
}

/// Confirms draft `id` through its review step, as Staff do.
pub async fn confirm_reviewed(b: &Browser<'_>, id: &str) -> harpseal_testing::TestResponse {
    let fields = review_fields(b, id).await;
    b.post(&format!("/admin/announcements/{id}/confirm"), &fields)
        .await
}

/// Posts the create form and returns the new announcement's id. With
/// `do=confirm` it confirms on the review step too (#120).
pub async fn create(b: &Browser<'_>, form: &str) -> String {
    let form = with_scope(form);
    let r = b.post("/admin/announcements/new", &form).await;
    assert_eq!(r.status, 303, "{}", r.body);
    let loc = r.header("location").unwrap().to_string();
    let id = loc.split('/').nth(3).unwrap().to_string();
    if loc.ends_with("/review") {
        let r = confirm_reviewed(b, &id).await;
        assert_eq!(r.status, 303, "{}", r.body);
    }
    // Load the detail page: its CSRF token and version for later posts.
    assert_eq!(
        b.get(&format!("/admin/announcements/{id}")).await.status,
        200
    );
    id
}

/// Ticks at the current time until no Job is claimed.
pub async fn drain(w: &World) -> usize {
    let mut runs = 0;
    for _ in 0..20 {
        let now = Timestamp(w.h.clock().now_ms());
        let r = w.app.deployment().tick(now).await.unwrap();
        assert!(r.errors.is_empty(), "{:?}", r.errors);
        if r.claimed == 0 {
            return runs;
        }
        runs += r.claimed;
    }
    panic!("jobs never settled");
}

/// One column of every row of `sql`, as text.
pub async fn texts(w: &World, sql: &str) -> Vec<String> {
    let rows =
        w.h.db()
            .query(Statement::new(sql.to_string()))
            .await
            .unwrap();
    rows.rows
        .iter()
        .map(|r| match &r[0] {
            harpseal::Value::Text(t) => t.clone(),
            harpseal::Value::Integer(i) => i.to_string(),
            harpseal::Value::Null => "NULL".into(),
            other => format!("{other:?}"),
        })
        .collect()
}

/// One integer from `sql`.
pub async fn count(w: &World, sql: &str) -> i64 {
    texts(w, sql).await[0].parse().unwrap()
}

/// The `version` of announcement `id`.
pub async fn version(w: &World, id: &str) -> i64 {
    count(
        w,
        &format!("SELECT version FROM announcement WHERE id = '{id}'"),
    )
    .await
}

/// Student `student` presses `action` on `channel`; returns what they saw.
pub async fn press(w: &World, channel: &str, student: &str, action: &str) -> Vec<Message> {
    let fake = if channel == TG { &w.tg } else { &w.line };
    let who = fake_account(channel, student);
    fake.take_sent();
    let ev = Inbound::action(
        who.clone(),
        ActionId::new(action).unwrap(),
        (channel != TG).then(|| ReplyToken {
            token: format!("r-{}", w.h.clock().now_ms()),
            expires_at: Timestamp(w.h.clock().now_ms() + 60_000),
        }),
    );
    let body = FakeChannel::encode(&[ev]);
    let req = Request::post(format!("/webhook/{channel}"))
        .header(SIGNATURE_HEADER, fake.sign(&body))
        .body(Body::from(body))
        .unwrap();
    let r = w.app.request(req).await;
    assert_eq!(r.status, 200, "{}", r.body);
    fake.sent_to(&who)
}

pub fn fake_account(channel: &str, student: &str) -> harpseal::ChannelAccountId {
    let kind = if channel == TG {
        harpseal::ChannelKind(TG)
    } else {
        harpseal_channel_fake::KIND
    };
    harpseal::ChannelAccountId::new(kind, ext(channel, student))
}

/// The text of a Text message.
pub fn text(m: &Message) -> String {
    match m {
        Message::Text(t) => t.clone(),
        other => format!("{other:?}"),
    }
}

/// "ส่งซ้ำ" of `id` seen at `version`, confirmed as Staff do: the ask page
/// first, then `confirm=yes` with the reach it shows to everyone (#120).
pub async fn resend_confirmed(
    b: &Browser<'_>,
    id: &str,
    version: i64,
) -> harpseal_testing::TestResponse {
    let path = format!("/admin/announcements/{id}/resend");
    let ask = b.post(&path, &format!("version={version}")).await;
    assert_eq!(ask.status, 200, "{}", ask.body);
    let reach = ask
        .body
        .split("name=\"reach\" value=\"")
        .nth(1)
        .and_then(|r| r.split('"').next())
        .map(|n| format!("&reach={n}"))
        .unwrap_or_default();
    b.post(&path, &format!("version={version}&confirm=yes{reach}"))
        .await
}

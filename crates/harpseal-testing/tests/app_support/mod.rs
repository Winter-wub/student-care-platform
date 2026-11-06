//! Setup for the Mini App tests (#28), on top of the Announcements world
//! (`fake` LineLike and `tg` TelegramLike Fake Channels, both with a Mini
//! App, whose `verify_session` takes the external id as the token).

#![allow(dead_code)]

use harpseal_testing::{Browser, TestResponse};

use crate::announcements_support::{World, ext, run};

/// A browser holding an App Session of `student`'s account on `channel`.
pub async fn open<'a>(w: &'a World, channel: &str, student: &str) -> Browser<'a> {
    let b = w.app.browser();
    let r = b
        .open_mini_app(channel, &ext(channel, student), "/app")
        .await;
    assert_eq!(r.status, 303, "{}", r.body);
    b
}

/// The CSRF token of a Mini App page (from the `hx-headers` on `<body>`).
pub fn csrf_of(body: &str) -> String {
    let key = "X-CSRF-Token&quot;:&quot;";
    let at = body.find(key).expect("the page carries a CSRF token") + key.len();
    body[at..]
        .chars()
        .take_while(char::is_ascii_hexdigit)
        .collect()
}

/// An HTMX `POST` with `form` exactly as given.
pub async fn hx_post(b: &Browser<'_>, path: &str, form: &str) -> TestResponse {
    let req = http::Request::post(path)
        .header("content-type", "application/x-www-form-urlencoded")
        .header("hx-request", "true")
        .body(axum::body::Body::from(form.to_string()))
        .unwrap();
    b.send(req).await
}

/// A sent announcement `id` with a Thai text, delivered to `student` on
/// `channel`; `at` is its `sent_at`, `expires_at` optional.
pub async fn seeded(
    w: &World,
    id: &str,
    student: &str,
    channel: &str,
    at: i64,
    ack: bool,
    expires_at: Option<i64>,
) {
    let exp = expires_at.map_or("NULL".to_string(), |e| e.to_string());
    run(
        &w.h,
        &format!(
            "INSERT OR IGNORE INTO announcement (id, status, require_ack, created_at, updated_at, \
             sent_at, expires_at) VALUES ('{id}', 'sent', {}, {at}, {at}, {at}, {exp})",
            i64::from(ack)
        ),
    )
    .await;
    run(
        &w.h,
        &format!(
            "INSERT OR IGNORE INTO announcement_text (announcement_id, locale, title, body) \
             VALUES ('{id}', 'th', 'ข่าว {id}', 'รายละเอียด {id}')"
        ),
    )
    .await;
    let e = ext(channel, student);
    run(
        &w.h,
        &format!(
            "INSERT INTO announcement_delivery (id, announcement_id, round, student_id, link_id, \
             channel, channel_account_id, status) VALUES ('d-{id}-{e}', '{id}', 0, '{student}', \
             'l-{e}', '{channel}', 'a-{e}', 'sent')"
        ),
    )
    .await;
}

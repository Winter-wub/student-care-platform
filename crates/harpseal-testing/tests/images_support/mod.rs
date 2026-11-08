//! Setup for the announcement image tests (#109): fictional PNG and JPEG
//! bytes made in code, the create / edit form as `multipart/form-data`,
//! and a Deployment with `public_url` set.

#![allow(dead_code)]

use axum::body::Body;
use harpseal::{Card, Message};
use harpseal_testing::{Browser, TestResponse};
use http::{Request, header};

use crate::announcements_support::*;

/// The `public_url` of [`boot_public`].
pub const PUBLIC: &str = "https://students.seal-bay.invalid";

/// The Announcements world with `public_url` set.
pub async fn boot_public() -> World {
    boot_config(&format!("public_url = \"{PUBLIC}/\"\n{CONFIG}"), 500).await
}

/// A PNG header of `w` x `h`: what the sniff reads (fictional bytes).
pub fn png(w: u32, h: u32) -> Vec<u8> {
    let mut b = b"\x89PNG\r\n\x1a\n".to_vec();
    b.extend_from_slice(&13u32.to_be_bytes());
    b.extend_from_slice(b"IHDR");
    b.extend_from_slice(&w.to_be_bytes());
    b.extend_from_slice(&h.to_be_bytes());
    b.extend_from_slice(&[8, 2, 0, 0, 0, 0, 0, 0, 0]);
    b
}

/// A JPEG with a SOF0 of `w` x `h`.
pub fn jpeg(w: u16, h: u16) -> Vec<u8> {
    let mut b = vec![0xFF, 0xD8, 0xFF, 0xC0, 0x00, 0x11, 0x08];
    b.extend_from_slice(&h.to_be_bytes());
    b.extend_from_slice(&w.to_be_bytes());
    b.extend_from_slice(&[3, 1, 0x22, 0, 2, 0x11, 1, 3, 0x11, 1, 0xFF, 0xD9]);
    b
}

/// One part of a form.
pub enum Part {
    Text(&'static str, String),
    File(String, String, Vec<u8>),
}

/// A text part.
pub fn t(name: &'static str, value: impl Into<String>) -> Part {
    Part::Text(name, value.into())
}

/// A file part: field, file name, bytes.
pub fn f(field: &str, name: &str, bytes: Vec<u8>) -> Part {
    Part::File(field.into(), name.into(), bytes)
}

/// `POST path` as `multipart/form-data`: the CSRF token first, then
/// `parts` (an empty file input is sent as browsers do).
pub async fn post_parts(b: &Browser<'_>, path: &str, parts: &[Part]) -> TestResponse {
    let boundary = "----sealbay-109";
    let mut body = Vec::new();
    let mut add = |head: String, value: &[u8]| {
        body.extend_from_slice(format!("--{boundary}\r\n{head}\r\n\r\n").as_bytes());
        body.extend_from_slice(value);
        body.extend_from_slice(b"\r\n");
    };
    let token = b.csrf().unwrap_or_default();
    add(
        "Content-Disposition: form-data; name=\"_csrf\"".into(),
        token.as_bytes(),
    );
    add(
        "Content-Disposition: form-data; name=\"image4\"; filename=\"\"\r\n\
         Content-Type: application/octet-stream"
            .into(),
        b"",
    );
    for p in parts {
        match p {
            Part::Text(name, v) => add(
                format!("Content-Disposition: form-data; name=\"{name}\""),
                v.as_bytes(),
            ),
            Part::File(field, name, bytes) => add(
                format!(
                    "Content-Disposition: form-data; name=\"{field}\"; filename=\"{name}\"\r\n\
                     Content-Type: image/png"
                ),
                bytes,
            ),
        }
    }
    body.extend_from_slice(format!("--{boundary}--\r\n").as_bytes());
    let req = Request::post(path)
        .header(
            header::CONTENT_TYPE,
            format!("multipart/form-data; boundary={boundary}"),
        )
        .body(Body::from(body))
        .unwrap();
    b.send(req).await
}

/// Creates an announcement from `parts` (title and body added); returns
/// its id.
pub async fn create_parts(b: &Browser<'_>, body: &str, extra: Vec<Part>) -> String {
    let mut parts = vec![t("title_th", "ย้ายห้องสอบ"), t("body_th", body)];
    parts.extend(extra);
    let r = post_parts(b, "/admin/announcements/new", &parts).await;
    assert_eq!(r.status, 303, "{}", r.body);
    let loc = r.header("location").unwrap().to_string();
    assert_eq!(b.get(&loc).await.status, 200);
    loc.rsplit('/').next().unwrap().to_string()
}

/// Posts the edit form of `id` with `body` and `extra` parts.
pub async fn edit_parts(
    w: &World,
    b: &Browser<'_>,
    id: &str,
    body: &str,
    extra: Vec<Part>,
) -> TestResponse {
    let path = format!("/admin/announcements/{id}/edit");
    assert_eq!(b.get(&path).await.status, 200);
    let mut parts = vec![
        t("title_th", "ย้ายห้องสอบ"),
        t("body_th", body),
        t("version", version(w, id).await.to_string()),
    ];
    parts.extend(extra);
    post_parts(b, &path, &parts).await
}

/// `(n, token, file_key)` of every image of `id`, by slot.
pub async fn stored(w: &World, id: &str) -> Vec<(String, String, String)> {
    let rows = texts(
        w,
        &format!(
            "SELECT n || '|' || token || '|' || file_key FROM announcement_file \
             WHERE announcement_id = '{id}' ORDER BY n"
        ),
    )
    .await;
    rows.into_iter()
        .map(|r| {
            let mut it = r.split('|').map(str::to_string);
            (it.next().unwrap(), it.next().unwrap(), it.next().unwrap())
        })
        .collect()
}

/// The token of slot `n` of `id`.
pub async fn token(w: &World, id: &str, n: &str) -> String {
    stored(w, id)
        .await
        .into_iter()
        .find(|(k, _, _)| k == n)
        .map(|(_, t, _)| t)
        .unwrap_or_else(|| panic!("no image {n}"))
}

/// Confirms (sends) announcement `id` from its detail page, then runs the
/// Jobs.
pub async fn send_now(w: &World, b: &Browser<'_>, id: &str) {
    let v = version(w, id).await;
    let r = b
        .post(
            &format!("/admin/announcements/{id}/confirm"),
            &format!("version={v}"),
        )
        .await;
    assert_eq!(r.status, 303, "{}", r.body);
    drain(w).await;
}

/// The first Card in `msgs`.
pub fn card_of(msgs: Vec<Message>) -> Card {
    msgs.into_iter()
        .find_map(|m| match m {
            Message::Card(c) => Some(c),
            _ => None,
        })
        .expect("a Card was pushed")
}

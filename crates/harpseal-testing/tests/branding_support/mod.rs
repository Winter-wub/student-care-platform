//! Shared by the `/admin/branding` tests (#114): a Deployment, signed-in
//! browsers, multipart posts with the CSRF token and the form's `seen`
//! version, and readers of the settings and audit rows.

#![allow(dead_code)]

use axum::body::Body;
use harpseal::{DeploymentConfig, Statement, Value};
use harpseal_testing::{Browser, Harness, TestApp, TestResponse};
use http::{Request, header};

pub const PASSWORD: &str = "kelp forest at dawn";
pub const PAGE: &str = "/admin/branding";
pub const SAVE: &str = "/admin/branding/save";
pub const BOUNDARY: &str = "----sealbay-branding";

pub const CONFIG: &str = r##"
name = "Seal Bay University (branding)"
default_locale = "th"
utc_offset = "+07:00"

[branding]
primary_color = "#0b5cad"

[roles]
Administrator = ["kernel.setting.edit", "kernel.audit.view"]
Viewer = ["kernel.student.view"]
"##;

pub fn config() -> DeploymentConfig {
    DeploymentConfig::from_toml(CONFIG).unwrap()
}

pub async fn boot(h: &Harness) -> TestApp {
    h.boot(h.builder().config(config()).build().unwrap()).await
}

pub async fn signed_in<'a>(h: &Harness, app: &'a TestApp, role: &str) -> Browser<'a> {
    let email = format!("{}@sealbay.example", role.to_lowercase());
    h.create_staff(&email, PASSWORD, &[role]).await;
    let b = app.browser();
    assert_eq!(b.login(&email, PASSWORD).await.status, 303);
    b
}

/// A PNG of `w` x `h` as the checks read it: the signature and `IHDR`,
/// then the `IEND` chunk.
pub fn png(w: u32, h: u32) -> Vec<u8> {
    let mut v = b"\x89PNG\r\n\x1a\n\0\0\0\x0dIHDR".to_vec();
    v.extend(w.to_be_bytes());
    v.extend(h.to_be_bytes());
    v.extend([8, 6, 0, 0, 0]);
    v.extend(b"\0\0\0\0IEND\xaeB`\x82");
    v
}

/// A multipart body: `(name, value)` parts; a part named `logo` is a file.
pub fn multipart(parts: &[(&str, &[u8])]) -> Vec<u8> {
    let mut body = Vec::new();
    for (name, value) in parts {
        let file = if *name == "logo" {
            "; filename=\"logo.png\"\r\nContent-Type: application/octet-stream"
        } else {
            ""
        };
        let head = format!("--{BOUNDARY}\r\nContent-Disposition: form-data; name=\"{name}\"{file}");
        body.extend_from_slice(format!("{head}\r\n\r\n").as_bytes());
        body.extend_from_slice(value);
        body.extend_from_slice(b"\r\n");
    }
    body.extend_from_slice(format!("--{BOUNDARY}--\r\n").as_bytes());
    body
}

/// Posts `parts` to `path` with the browser's CSRF token first.
/// The `seen` version in a rendered branding page.
pub fn seen_in(body: &str) -> Option<String> {
    let mark = "name=\"seen\" value=\"";
    let at = body.find(mark)? + mark.len();
    Some(body[at..at + body[at..].find('"')?].to_string())
}

/// Posts `parts` to `path` with the browser's CSRF token first and, for the
/// name form, the `seen` version of the page as it is now (as a browser
/// that just opened it), unless `parts` brings its own.
pub async fn post(b: &Browser<'_>, path: &str, parts: &[(&str, &[u8])]) -> TestResponse {
    let seen = if path == SAVE && !parts.iter().any(|(n, _)| *n == "seen") {
        seen_in(&b.get(PAGE).await.body)
    } else {
        None
    };
    let token = b.csrf().unwrap_or_default();
    let mut all: Vec<(&str, &[u8])> = vec![("_csrf", token.as_bytes())];
    if let Some(v) = &seen {
        all.push(("seen", v.as_bytes()));
    }
    all.extend_from_slice(parts);
    post_raw(b, path, &all).await
}

pub async fn post_raw(b: &Browser<'_>, path: &str, parts: &[(&str, &[u8])]) -> TestResponse {
    let req = Request::post(path)
        .header(
            header::CONTENT_TYPE,
            format!("multipart/form-data; boundary={BOUNDARY}"),
        )
        .body(Body::from(multipart(parts)))
        .unwrap();
    b.send(req).await
}

pub async fn settings(h: &Harness) -> Vec<(String, String)> {
    let rows = h
        .db()
        .query(Statement::new(
            "SELECT key, value FROM setting WHERE key LIKE 'kernel.branding.%' ORDER BY key",
        ))
        .await
        .unwrap();
    (0..rows.len())
        .map(|i| {
            let t = |c| rows.get(i, c).and_then(Value::as_text).unwrap().to_string();
            (t("key"), t("value"))
        })
        .collect()
}

pub async fn audits(h: &Harness, action: &str) -> Vec<(String, String)> {
    let rows = h
        .db()
        .query(
            Statement::new(
                "SELECT coalesce(before, '') AS b, coalesce(after, '') AS a \
                 FROM audit_log WHERE action = ? ORDER BY at",
            )
            .bind(action),
        )
        .await
        .unwrap();
    (0..rows.len())
        .map(|i| {
            let t = |c| rows.get(i, c).and_then(Value::as_text).unwrap().to_string();
            (t("b"), t("a"))
        })
        .collect()
}

/// A setting row written behind the page's back (a restore, a hand edit, a
/// Module's own setting).
pub async fn put_setting(h: &Harness, key: &str, locale: &str, value: &str) {
    h.db()
        .execute(
            Statement::new(
                "INSERT INTO setting (key, locale, value, updated_at) VALUES (?, ?, ?, 1) \
                 ON CONFLICT (key, locale) DO UPDATE SET value = excluded.value",
            )
            .bind(key)
            .bind(locale)
            .bind(value),
        )
        .await
        .unwrap();
}

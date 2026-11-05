//! Issue #5 prototype pages: Staff login, the Students list with HTMX search
//! and a "send announcement" form (back office), plus the Mini App page and
//! the public notice. Platform-neutral: the same code runs on Workers (D1)
//! and on the native binary (SQLite), through `Services`.

mod auth;
mod mini_app;
mod students;

use axum::Extension;
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use harpseal::manifest::Permission;
use harpseal::port_types::Value;
use harpseal::{ModuleRegistrar, Services};
use sha2::{Digest, Sha256};

pub use auth::{SESSION_COOKIE, SecureCookies, Staff};

const SCHEMA_SQL: &str = include_str!("../../sql/0002_pages.sql");
const SEED_SQL: &str = include_str!("../../sql/0003_seed.sql");

const STUDENT_VIEW: Permission = Permission {
    id: "kernel.student.view",
    description: "See Students",
};
const ANNOUNCE_SEND: Permission = Permission {
    id: "announcements.send",
    description: "Create and send announcements",
};

/// The prototype pages as one Module.
pub struct Pages {
    /// `Secure` on cookies. Off only for plain-HTTP localhost.
    pub secure_cookies: bool,
}

impl harpseal::Module for Pages {
    fn name(&self) -> &'static str {
        "pages"
    }

    fn register(&self, r: &mut ModuleRegistrar) {
        let secure = self.secure_cookies;
        let admin = axum::Router::new()
            .route("/login", get(auth::login_form).post(auth::login))
            .route("/logout", post(auth::logout))
            .route("/students", get(students::page))
            .route("/students/rows", get(students::rows))
            .route("/announcements", post(students::announce))
            .route("/announcements/audience", get(students::audience_count))
            .layer(Extension(SecureCookies(secure)));
        let public = axum::Router::new()
            .route("/", get(mini_app::index))
            .route("/app", get(mini_app::page))
            .route("/app/ack/{id}", post(mini_app::ack))
            .route("/notice", get(mini_app::notice))
            .layer(Extension(SecureCookies(secure)));
        r.permission(STUDENT_VIEW)
            .permission(ANNOUNCE_SEND)
            .migration("0002_pages", SCHEMA_SQL)
            .migration("0003_seed", SEED_SQL)
            .back_office(STUDENT_VIEW.id, admin)
            .public(public);
    }
}

/// A Core error turned into a plain 500 page (prototype).
pub(crate) struct Fail(harpseal::Error);

impl From<harpseal::Error> for Fail {
    fn from(e: harpseal::Error) -> Self {
        Fail(e)
    }
}

impl IntoResponse for Fail {
    fn into_response(self) -> Response {
        let body = format!("เกิดข้อผิดพลาด: {}", self.0);
        (StatusCode::INTERNAL_SERVER_ERROR, body).into_response()
    }
}

pub(crate) type Page = Result<Response, Fail>;

pub(crate) fn text(row: &[Value], i: usize) -> String {
    match row.get(i) {
        Some(Value::Text(s)) => s.clone(),
        Some(Value::Integer(n)) => n.to_string(),
        Some(Value::Real(f)) => f.to_string(),
        _ => String::new(),
    }
}

pub(crate) fn int(row: &[Value], i: usize) -> i64 {
    match row.get(i) {
        Some(Value::Integer(n)) => *n,
        Some(Value::Real(f)) => *f as i64,
        _ => 0,
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

pub(crate) fn sha256_hex(s: &str) -> String {
    hex(&Sha256::digest(s.as_bytes()))
}

pub(crate) fn random_hex(s: &Services, n: usize) -> harpseal::Result<String> {
    let mut buf = vec![0u8; n];
    s.random.fill(&mut buf)?;
    Ok(hex(&buf))
}

/// UUIDv7 as text (ADR-0007): 48-bit ms timestamp, version, random bits.
pub(crate) fn uuid7(s: &Services) -> harpseal::Result<String> {
    let mut b = [0u8; 16];
    s.random.fill(&mut b[6..])?;
    let ms = s.clock.now().0 as u64;
    b[..6].copy_from_slice(&ms.to_be_bytes()[2..]);
    b[6] = 0x70 | (b[6] & 0x0f);
    b[8] = 0x80 | (b[8] & 0x3f);
    let h = hex(&b);
    Ok(format!(
        "{}-{}-{}-{}-{}",
        &h[..8],
        &h[8..12],
        &h[12..16],
        &h[16..20],
        &h[20..]
    ))
}

pub(crate) fn cookie<'a>(headers: &'a HeaderMap, name: &str) -> Option<&'a str> {
    headers
        .get_all(header::COOKIE)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|v| v.split(';'))
        .filter_map(|kv| kv.trim().split_once('='))
        .find(|(k, _)| *k == name)
        .map(|(_, v)| v)
}

pub(crate) fn is_htmx(headers: &HeaderMap) -> bool {
    headers.contains_key("hx-request")
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderValue;

    #[test]
    fn reads_one_cookie_among_many() {
        let mut h = HeaderMap::new();
        h.insert(
            header::COOKIE,
            HeaderValue::from_static("a=1; hs_session=xyz; b=2"),
        );
        assert_eq!(cookie(&h, "hs_session"), Some("xyz"));
        assert_eq!(cookie(&h, "c"), None);
    }

    #[test]
    fn sha256_is_hex() {
        assert_eq!(
            sha256_hex("abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }
}

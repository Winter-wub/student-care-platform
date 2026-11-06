//! Dev mode (`HARPSEAL_DEV_ROUTES=1`), for demos and `just e2e` only:
//!
//! - email is captured in memory instead of sent ([`CapturingEmail`]), so
//!   the OTP flow works without an SMTP relay;
//! - three routes show what a Student would have seen on a phone:
//!
//! | Route | Answer |
//! |---|---|
//! | `GET /dev/outbox?to=<external id>` | JSON: what the Fake Channel sent that account, and its Menu |
//! | `GET /dev/email?to=<address>` | JSON: the last email to that address |
//! | `POST /dev/tick` | runs `Deployment::tick` now (the ticker waits a minute) |
//!
//! Every route needs header `x-dev-secret: <FAKE_SECRET>` (compared in
//! constant time) and answers 404 without it, so the routes look absent.
//! The OTP itself is unchanged: same length, expiry and attempt limit; dev
//! mode only changes where the email goes. Never enable it in production.

use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use axum::extract::RawQuery;
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use harpseal::{Button, Deployment, Email, EmailSender, Message};
use harpseal_channel_fake::FakeChannel;
use serde_json::{Value, json};

/// Header carrying `FAKE_SECRET`.
pub const SECRET_HEADER: &str = "x-dev-secret";

/// Keeps every email instead of sending it.
#[derive(Debug, Default, Clone)]
pub struct CapturingEmail(Arc<Mutex<Vec<Email>>>);

impl CapturingEmail {
    fn last_to(&self, to: &str) -> Option<Email> {
        let all = self.0.lock().unwrap_or_else(|p| p.into_inner());
        all.iter()
            .rev()
            .find(|e| e.to.eq_ignore_ascii_case(to))
            .cloned()
    }
}

#[async_trait]
impl EmailSender for CapturingEmail {
    async fn send(&self, email: Email) -> harpseal::Result<()> {
        eprintln!("dev: email to {} captured: {}", email.to, email.subject);
        self.0.lock().unwrap_or_else(|p| p.into_inner()).push(email);
        Ok(())
    }
}

#[derive(Clone)]
struct Dev {
    deployment: Arc<Deployment>,
    fake: FakeChannel,
    email: CapturingEmail,
    secret: String,
}

/// The `/dev/*` routes.
pub fn router(
    deployment: Arc<Deployment>,
    fake: FakeChannel,
    email: CapturingEmail,
    secret: String,
) -> axum::Router {
    let dev = Dev {
        deployment,
        fake,
        email,
        secret,
    };
    let (a, b, c) = (dev.clone(), dev.clone(), dev);
    axum::Router::new()
        .route(
            "/dev/outbox",
            get(move |h: HeaderMap, q: RawQuery| async move { a.outbox(&h, q) }),
        )
        .route(
            "/dev/email",
            get(move |h: HeaderMap, q: RawQuery| async move { b.email(&h, q) }),
        )
        .route(
            "/dev/tick",
            post(move |h: HeaderMap| async move { c.tick(&h).await }),
        )
}

/// Whether `a` equals `b`, in time independent of where they differ.
fn same(a: &str, b: &str) -> bool {
    a.len() == b.len()
        && a.bytes()
            .zip(b.bytes())
            .fold(0u8, |acc, (x, y)| acc | (x ^ y))
            == 0
}

/// Value of `key` in a query string (`+` and `%XX` decoded).
pub fn query_param(query: &str, key: &str) -> Option<String> {
    query.split('&').find_map(|pair| {
        let (k, v) = pair.split_once('=')?;
        (k == key).then(|| decode(v))
    })
}

fn decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let hex = |b: u8| (b as char).to_digit(16).map(|d| d as u8);
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let pair = bytes
            .get(i + 1)
            .copied()
            .and_then(hex)
            .zip(bytes.get(i + 2).copied().and_then(hex));
        match (bytes[i], pair) {
            (b'+', _) => out.push(b' '),
            (b'%', Some((hi, lo))) => {
                out.push(hi * 16 + lo);
                i += 2;
            }
            (b, _) => out.push(b),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn json_response(v: &Value) -> Response {
    ([(header::CONTENT_TYPE, "application/json")], v.to_string()).into_response()
}

fn buttons(bs: &[Button]) -> Value {
    bs.iter()
        .map(|b| match b {
            Button::Action { label, action } => {
                json!({ "label": label, "action": action.as_str() })
            }
            Button::MiniApp { label, path } => json!({ "label": label, "mini_app": path }),
            other => json!({ "label": other.label() }),
        })
        .collect()
}

/// A Message as JSON: `kind`, `text` (Text, Card title + body, Choices
/// prompt) and `buttons`.
pub fn message_json(m: &Message) -> Value {
    let (text, bs): (String, Value) = match m {
        Message::Text(t) => (t.clone(), json!([])),
        Message::Card(c) => (format!("{}\n{}", c.title, c.body), buttons(&c.buttons)),
        Message::CardList(l) => {
            let text =
                l.0.iter()
                    .map(|c| c.title.as_str())
                    .collect::<Vec<_>>()
                    .join("\n");
            let all: Vec<Button> = l.0.iter().flat_map(|c| c.buttons.clone()).collect();
            (text, buttons(&all))
        }
        Message::Choices(c) => (c.text.clone(), buttons(&c.options)),
        other => (
            String::new(),
            buttons(&other.buttons().into_iter().cloned().collect::<Vec<_>>()),
        ),
    };
    json!({ "kind": m.kind(), "text": text, "buttons": bs })
}

impl Dev {
    fn allowed(&self, h: &HeaderMap) -> bool {
        let got = h
            .get(SECRET_HEADER)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");
        !got.is_empty() && same(got, &self.secret)
    }

    fn outbox(&self, h: &HeaderMap, RawQuery(q): RawQuery) -> Response {
        if !self.allowed(h) {
            return StatusCode::NOT_FOUND.into_response();
        }
        let Some(to) = query_param(q.as_deref().unwrap_or(""), "to") else {
            return StatusCode::BAD_REQUEST.into_response();
        };
        let who = FakeChannel::account(to);
        let messages: Vec<Value> = self.fake.sent_to(&who).iter().map(message_json).collect();
        let menu = match self.fake.menu(&who) {
            None => json!("unset"),
            Some(None) => json!("locked"),
            Some(Some(m)) => m
                .entries
                .iter()
                .map(|e| json!({ "label": e.label, "action": e.action.as_str() }))
                .collect(),
        };
        json_response(&json!({ "messages": messages, "menu": menu }))
    }

    fn email(&self, h: &HeaderMap, RawQuery(q): RawQuery) -> Response {
        if !self.allowed(h) {
            return StatusCode::NOT_FOUND.into_response();
        }
        let Some(to) = query_param(q.as_deref().unwrap_or(""), "to") else {
            return StatusCode::BAD_REQUEST.into_response();
        };
        match self.email.last_to(&to) {
            Some(e) => json_response(&json!({ "to": e.to, "subject": e.subject, "text": e.text })),
            None => StatusCode::NOT_FOUND.into_response(),
        }
    }

    async fn tick(&self, h: &HeaderMap) -> Response {
        if !self.allowed(h) {
            return StatusCode::NOT_FOUND.into_response();
        }
        let now = self.deployment.services().clock.now();
        match self.deployment.tick(now).await {
            Ok(r) => json_response(&json!({
                "claimed": r.claimed, "done": r.done, "retried": r.retried,
                "failed": r.failed, "errors": r.errors,
            })),
            Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn query_params_are_decoded() {
        let q = "to=s68010001%40sealbay.example&x=a+b";
        assert_eq!(query_param(q, "to").unwrap(), "s68010001@sealbay.example");
        assert_eq!(query_param(q, "x").unwrap(), "a b");
        assert_eq!(query_param(q, "y"), None);
        assert_eq!(query_param("to=%E0%B8%81", "to").unwrap(), "ก");
        assert_eq!(query_param("to=50%", "to").unwrap(), "50%");
    }

    #[test]
    fn the_secret_must_match_exactly() {
        assert!(same("abc", "abc"));
        assert!(!same("abc", "abd"));
        assert!(!same("abc", "ab"));
        assert!(!same("", "abc"));
    }
}

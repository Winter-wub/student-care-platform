//! The audit writer (ADR-0007): one append-only `audit_log` row per Staff
//! write, with actor, action, target, before/after JSON and the request it
//! came from. Secrets are redacted before anything is written.
//!
//! Module pages take a [`RequestCtx`] (an axum extractor) and the signed-in
//! [`crate::CurrentStaff`], then call [`record`], or put [`entry`] into the
//! same `Db::batch` as the write itself so both commit or neither does:
//!
//! ```no_run
//! # async fn demo(s: harpseal::AppState, staff: harpseal::CurrentStaff,
//! #               ctx: harpseal::RequestCtx) -> harpseal::Result<()> {
//! use harpseal::audit::{self, Actor, Target};
//! let before = serde_json::json!({ "display_name": "Old" });
//! let after = serde_json::json!({ "display_name": "New" });
//! audit::record(&s.services, &Actor::staff(&staff), "student.update",
//!     Some(&Target::new("student", "0190...")), Some(before), Some(after), &ctx).await
//! # }
//! ```

use axum::extract::FromRequestParts;
use axum::http::request::Parts;
use axum::http::{HeaderMap, header};
use serde_json::Value as Json;

use crate::port_types::Statement;
use crate::{Result, Services};

/// What replaces a redacted value.
pub const REDACTED: &str = "[redacted]";

/// Longest `user_agent` kept, in characters.
const MAX_USER_AGENT: usize = 512;

/// Who did it.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Actor {
    /// A Staff member, by `staff.id`.
    Staff(String),
    /// A Student, by `student.id` (e.g. `link.create` after Verification).
    Student(String),
    /// The Core itself, the CLI, or an anonymous request (a failed login
    /// for an unknown email).
    System,
    /// A Job, by `job.id`.
    Job(String),
}

impl Actor {
    /// The signed-in Staff member.
    pub fn staff(s: &crate::CurrentStaff) -> Self {
        Actor::Staff(s.id.clone())
    }

    fn parts(&self) -> (&'static str, Option<&str>) {
        match self {
            Actor::Staff(id) => ("staff", Some(id)),
            Actor::Student(id) => ("student", Some(id)),
            Actor::System => ("system", None),
            Actor::Job(id) => ("job", Some(id)),
        }
    }
}

/// What it was done to: a table and a row id.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Target {
    /// Table (or kind of thing), e.g. `student`, `staff`, `role`.
    pub kind: String,
    /// Row id.
    pub id: String,
}

impl Target {
    /// A target row.
    pub fn new(kind: impl Into<String>, id: impl Into<String>) -> Self {
        Self {
            kind: kind.into(),
            id: id.into(),
        }
    }
}

/// Where a request came from, for audit rows. The Deployment's Router makes
/// one per request (and answers with its id in `x-request-id`); extract it
/// in a handler with `ctx: RequestCtx`. `Default` is "no request" (CLI,
/// Jobs).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct RequestCtx {
    /// The client address: `CF-Connecting-IP` on Workers, the right-most
    /// public `X-Forwarded-For` hop only with `[staff] trust_forwarded_for`,
    /// else the [`PeerIp`] the runtime recorded. Headers a client could
    /// forge are never trusted on their own.
    pub ip: Option<String>,
    /// `User-Agent`, cut to 512 characters.
    pub user_agent: Option<String>,
    /// The id the Router gave this request.
    pub request_id: Option<String>,
}

/// Response header carrying [`RequestCtx::request_id`].
pub const REQUEST_ID_HEADER: &str = "x-request-id";

impl RequestCtx {
    /// Reads ip and user agent from the request; `request_id` as given.
    pub(crate) fn from_parts(
        headers: &HeaderMap,
        peer: Option<String>,
        trust_forwarded_for: bool,
        request_id: Option<String>,
    ) -> Self {
        let ip = crate::client_ip::resolve(headers, peer, trust_forwarded_for);
        let user_agent = headers
            .get(header::USER_AGENT)
            .and_then(|v| v.to_str().ok())
            .map(|v| v.chars().take(MAX_USER_AGENT).collect());
        Self {
            ip,
            user_agent,
            request_id,
        }
    }
}

impl<S: Send + Sync> FromRequestParts<S> for RequestCtx {
    type Rejection = std::convert::Infallible;

    async fn from_request_parts(parts: &mut Parts, _: &S) -> Result<Self, Self::Rejection> {
        if let Some(ctx) = parts.extensions.get::<RequestCtx>() {
            return Ok(ctx.clone());
        }
        Ok(RequestCtx::from_parts(
            &parts.headers,
            peer(parts),
            false,
            None,
        ))
    }
}

/// The TCP peer's IP address, as a request extension. A runtime that
/// knows it inserts one (the native runtime's `serve` does); the Core
/// cannot read the server's connection itself on every platform.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PeerIp(pub String);

/// The peer address, when the runtime recorded it.
pub(crate) fn peer(parts: &Parts) -> Option<String> {
    parts.extensions.get::<PeerIp>().map(|p| p.0.clone())
}

/// Whether a JSON key names a secret: anything containing `password`,
/// `secret` or `token`, and `hash` / `*_hash`.
fn is_secret_key(key: &str) -> bool {
    let k = key.to_ascii_lowercase();
    k.contains("password")
        || k.contains("secret")
        || k.contains("token")
        || k == "hash"
        || k.ends_with("_hash")
}

/// Replaces the value of every secret-named key (see the module docs),
/// at any depth, with [`REDACTED`].
pub fn redact(v: Json) -> Json {
    match v {
        Json::Object(map) => Json::Object(
            map.into_iter()
                .map(|(k, v)| {
                    let v = if is_secret_key(&k) {
                        Json::String(REDACTED.into())
                    } else {
                        redact(v)
                    };
                    (k, v)
                })
                .collect(),
        ),
        Json::Array(items) => Json::Array(items.into_iter().map(redact).collect()),
        other => other,
    }
}

/// The `INSERT` for one audit row, redacted, for a caller's `Db::batch`.
pub fn entry(
    services: &Services,
    actor: &Actor,
    action: &str,
    target: Option<&Target>,
    before: Option<Json>,
    after: Option<Json>,
    ctx: &RequestCtx,
) -> Result<Statement> {
    let (stmt, _) = row(services, actor, action, target, before, after, ctx, None)?;
    Ok(stmt)
}

/// [`entry`] that inserts its row only when `condition` (an SQL boolean
/// expression with its own params, e.g. `EXISTS (SELECT 1 FROM link WHERE
/// id = ? AND status <> 'revoked')`) holds when the statement runs. Also
/// returns the row's id, so later statements of the same batch can run
/// only if the row was written (`EXISTS (SELECT 1 FROM audit_log WHERE
/// id = ?)`): a compare-and-set whose audit row is the gate.
#[allow(clippy::too_many_arguments)]
pub fn entry_if(
    services: &Services,
    actor: &Actor,
    action: &str,
    target: Option<&Target>,
    before: Option<Json>,
    after: Option<Json>,
    ctx: &RequestCtx,
    condition: Statement,
) -> Result<(Statement, String)> {
    row(
        services,
        actor,
        action,
        target,
        before,
        after,
        ctx,
        Some(condition),
    )
}

#[allow(clippy::too_many_arguments)]
fn row(
    services: &Services,
    actor: &Actor,
    action: &str,
    target: Option<&Target>,
    before: Option<Json>,
    after: Option<Json>,
    ctx: &RequestCtx,
    condition: Option<Statement>,
) -> Result<(Statement, String)> {
    let (actor_kind, actor_id) = actor.parts();
    let json = |v: Option<Json>| v.map(|v| redact(v).to_string());
    let id = services.new_id()?;
    let head = "INSERT INTO audit_log (id, at, actor_kind, actor_id, action, target_kind, \
        target_id, before, after, ip, user_agent, request_id)";
    let values = "?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?";
    let sql = match &condition {
        None => format!("{head} VALUES ({values})"),
        Some(c) => format!("{head} SELECT {values} WHERE {}", c.sql),
    };
    let mut stmt = Statement::new(sql)
        .bind(id.clone())
        .bind(services.now_ms())
        .bind(actor_kind)
        .bind(actor_id.map(str::to_string))
        .bind(action)
        .bind(target.map(|t| t.kind.clone()))
        .bind(target.map(|t| t.id.clone()))
        .bind(json(before))
        .bind(json(after))
        .bind(ctx.ip.clone())
        .bind(ctx.user_agent.clone())
        .bind(ctx.request_id.clone());
    if let Some(c) = condition {
        stmt.params.extend(c.params);
    }
    Ok((stmt, id))
}

/// Writes one audit row on its own. Prefer [`entry`] inside the write's
/// own batch when there is one.
pub async fn record(
    services: &Services,
    actor: &Actor,
    action: &str,
    target: Option<&Target>,
    before: Option<Json>,
    after: Option<Json>,
    ctx: &RequestCtx,
) -> Result<()> {
    let stmt = entry(services, actor, action, target, before, after, ctx)?;
    services.db.execute(stmt).await.map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn secrets_are_redacted_at_any_depth() {
        let v = json!({
            "email": "a@example.edu",
            "password_hash": "$argon2id$...",
            "nested": [{ "api_token": "t", "Client_Secret": "s", "hash": "h", "ok": 1 }],
            "new_password": "hunter2hunter2",
        });
        let r = redact(v);
        assert_eq!(r["email"], "a@example.edu");
        assert_eq!(r["password_hash"], REDACTED);
        assert_eq!(r["new_password"], REDACTED);
        assert_eq!(r["nested"][0]["api_token"], REDACTED);
        assert_eq!(r["nested"][0]["Client_Secret"], REDACTED);
        assert_eq!(r["nested"][0]["hash"], REDACTED);
        assert_eq!(r["nested"][0]["ok"], 1);
    }
}

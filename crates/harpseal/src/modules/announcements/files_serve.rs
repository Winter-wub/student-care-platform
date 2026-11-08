//! Serving announcement files (#109; PDFs #113 reuse it).
//!
//! - `GET /announcements/files/{token}`, public: LINE's and Telegram's
//!   servers fetch it without a session. Served only while the
//!   announcement is sending or sent and not expired ("live"); a draft, a
//!   schedule, a retracted or expired announcement, a removed file and an
//!   unknown token all get the same 404.
//! - `GET /admin/announcements/{id}/files/{token}`, Permission
//!   `announcements.view`, any status: Staff see drafts and retracted
//!   ones there.
//!
//! Both answer with the stored content type (never one guessed from the
//! bytes or the FileStore), `X-Content-Type-Options: nosniff`,
//! `Content-Disposition: inline` with a generated name, and a
//! `Content-Security-Policy` that runs nothing, should a browser render
//! the file as a page.

use axum::extract::{Path, State};
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};

use super::files::{COLUMNS, Stored, stored_at};
use crate::port_types::Statement;
use crate::{AppState, Result, Services};

/// How long a platform or browser may keep a public file: short, since a
/// retract or an expiry must stop serving it soon.
pub(crate) const PUBLIC_CACHE: &str = "public, max-age=300";

/// A token as minted: 32 lowercase hex characters.
fn well_formed(token: &str) -> bool {
    token.len() == 32
        && token
            .bytes()
            .all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

/// The file with `token`, and whether its announcement is live at `now`.
pub(crate) async fn by_token(
    s: &Services,
    token: &str,
    now: i64,
) -> Result<Option<(Stored, bool)>> {
    if !well_formed(token) {
        return Ok(None);
    }
    let sql = format!(
        "SELECT {COLUMNS}, (a.status IN ('sending', 'sent') \
         AND (a.expires_at IS NULL OR a.expires_at > ?)) AS live \
         FROM announcement_file f JOIN announcement a ON a.id = f.announcement_id \
         WHERE f.token = ?"
    );
    let rows =
        s.db.query(Statement::new(sql).bind(now).bind(token))
            .await?;
    if rows.is_empty() {
        return Ok(None);
    }
    let live = super::store::int(&rows, 0, "live") == Some(1);
    Ok(Some((stored_at(&rows, 0), live)))
}

fn not_found() -> Response {
    let mut r = (StatusCode::NOT_FOUND, "ไม่พบไฟล์ (not found)").into_response();
    r.headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    r
}

/// The file's bytes with the headers above; 404 when the object is gone.
async fn respond(s: &Services, f: &Stored, cache: &'static str) -> Response {
    let bytes = match s.files.get(&f.file_key).await {
        Ok(Some(b)) => b,
        Ok(None) => return not_found(),
        Err(_) => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    };
    // Only the types the upload takes; anything else stored is not served.
    let Some(kind) = super::files_sniff::ImageType::parse(&f.content_type) else {
        return not_found();
    };
    let name = format!("image-{}.{}", f.n, kind.ext());
    let mut r = (StatusCode::OK, bytes).into_response();
    let h = r.headers_mut();
    h.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static(kind.content_type()),
    );
    h.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    if let Ok(v) = HeaderValue::from_str(&format!("inline; filename=\"{name}\"")) {
        h.insert(header::CONTENT_DISPOSITION, v);
    }
    h.insert(
        header::CONTENT_SECURITY_POLICY,
        HeaderValue::from_static("default-src 'none'; sandbox"),
    );
    h.insert(header::CACHE_CONTROL, HeaderValue::from_static(cache));
    r
}

/// `GET /announcements/files/{token}`.
pub(super) async fn public(State(s): State<AppState>, Path(token): Path<String>) -> Response {
    let svc = &s.services;
    match by_token(svc, &token, svc.now_ms()).await {
        Ok(Some((f, true))) => respond(svc, &f, PUBLIC_CACHE).await,
        Ok(_) => not_found(),
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

/// `GET /admin/announcements/{id}/files/{token}`.
pub(super) async fn staff(
    State(s): State<AppState>,
    Path((id, token)): Path<(String, String)>,
) -> Response {
    let svc = &s.services;
    match by_token(svc, &token, svc.now_ms()).await {
        Ok(Some((f, _))) if f.announcement_id == id => respond(svc, &f, "private, no-store").await,
        Ok(_) => not_found(),
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

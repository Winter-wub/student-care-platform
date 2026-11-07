//! The CSRF check of the back-office guard, which may read the request
//! body: the `X-CSRF-Token` header, or the `_csrf` field of a urlencoded
//! body or the `_csrf` part of a multipart one. A body over its limit is
//! told apart (#46), so the guard answers 413 instead of "form expired".

use axum::body::Body;
use axum::extract::Request;
use axum::http::{Method, header};
use http_body_util::LengthLimitError;

use super::{CSRF_FIELD, CSRF_HEADER, cookies};

/// Largest form body the guard reads to find the CSRF field.
const FORM_LIMIT: usize = 1024 * 1024;
/// Largest `multipart/form-data` body read for its `_csrf` part on a route
/// that does not take uploads. An upload route (`back_office_upload`, the
/// roster import) sets its own limit in `Gate::body_limit`, and its body
/// is only read once the Staff member is known to hold its Permission.
pub(crate) const MULTIPART_LIMIT: usize = 64 * 1024;

/// What the check found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Csrf {
    Ok,
    /// Missing or wrong token, or a body that is neither form kind.
    Bad,
    /// The body is over its limit (by `Content-Length`, before reading,
    /// or while reading); the request keeps no body.
    TooLarge,
}

/// Safe methods pass. Otherwise the `X-CSRF-Token` header or the `_csrf`
/// field of a urlencoded body must equal `expected`; the body is read and
/// put back for the handler. A multipart body is read up to
/// `multipart_limit` for its `_csrf` part.
pub(crate) async fn check_csrf(
    req: Request,
    expected: &str,
    multipart_limit: usize,
) -> (Request, Csrf) {
    let verdict = |ok: bool| if ok { Csrf::Ok } else { Csrf::Bad };
    if matches!(*req.method(), Method::GET | Method::HEAD | Method::OPTIONS) {
        return (req, Csrf::Ok);
    }
    if let Some(v) = req.headers().get(CSRF_HEADER).and_then(|v| v.to_str().ok()) {
        let ok = cookies::ct_eq(v.as_bytes(), expected.as_bytes());
        return (req, verdict(ok));
    }
    let content_type = req
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default()
        .to_string();
    // A file upload (the roster import, #24) carries `_csrf` as a part.
    let boundary = cookies::multipart_boundary(&content_type);
    let is_form = content_type.starts_with("application/x-www-form-urlencoded");
    if !is_form && boundary.is_none() {
        return (req, Csrf::Bad);
    }
    let limit = if is_form { FORM_LIMIT } else { multipart_limit };
    let declared = req
        .headers()
        .get(header::CONTENT_LENGTH)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<u64>().ok());
    let (parts, body) = req.into_parts();
    if declared.is_some_and(|n| n > limit as u64) {
        return (Request::from_parts(parts, Body::empty()), Csrf::TooLarge);
    }
    let bytes = match axum::body::to_bytes(body, limit).await {
        Ok(b) => b,
        Err(e) => {
            let why = if over_limit(&e) {
                Csrf::TooLarge
            } else {
                Csrf::Bad
            };
            return (Request::from_parts(parts, Body::empty()), why);
        }
    };
    let found = match &boundary {
        Some(b) => cookies::multipart_field(&bytes, b, CSRF_FIELD),
        None => std::str::from_utf8(&bytes)
            .ok()
            .and_then(|b| cookies::form_field(b, CSRF_FIELD)),
    };
    let ok = found.is_some_and(|v| cookies::ct_eq(v.as_bytes(), expected.as_bytes()));
    (Request::from_parts(parts, Body::from(bytes)), verdict(ok))
}

/// Whether reading stopped at the length limit (anywhere in the chain).
fn over_limit(e: &axum::Error) -> bool {
    let mut cur: Option<&(dyn std::error::Error + 'static)> = Some(e);
    while let Some(err) = cur {
        if err.is::<LengthLimitError>() {
            return true;
        }
        cur = err.source();
    }
    false
}

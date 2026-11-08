//! `GET /branding/logo` (#114): the saved logo, public because the
//! sign-in page and the Mini App show it before any session. Its URL
//! carries the start of the content hash (`?v=`), so a new logo is a new
//! URL and the old one may be cached for a day.

use axum::extract::State;
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};

use super::check::LogoKind;
use super::{LOGO_KEY, LOGO_PATH, Stored};
use crate::AppState;

/// The logo's URL for FileStore key `key` (`branding/<sha256>.<ext>`).
pub(crate) fn url(key: &str) -> String {
    let file = key.rsplit('/').next().unwrap_or_default();
    let v: String = file.chars().take(12).collect();
    format!("{LOGO_PATH}?v={v}")
}

/// An image response with its real type, never sniffed.
pub(crate) fn image(bytes: Vec<u8>, content_type: &'static str, cache: &'static str) -> Response {
    let mut r = bytes.into_response();
    let h = r.headers_mut();
    h.insert(header::CONTENT_TYPE, HeaderValue::from_static(content_type));
    h.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    h.insert(header::CACHE_CONTROL, HeaderValue::from_static(cache));
    r
}

pub(super) async fn serve(State(s): State<AppState>) -> Response {
    let Ok(stored) = Stored::load(&s.services).await else {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    };
    let Some((key, kind)) = stored
        .get(LOGO_KEY)
        .and_then(|k| Some((k, LogoKind::of_key(k)?)))
    else {
        return StatusCode::NOT_FOUND.into_response();
    };
    match s.services.files.get(key).await {
        Ok(Some(bytes)) => image(bytes, kind.content_type(), "public, max-age=86400"),
        Ok(None) => StatusCode::NOT_FOUND.into_response(),
        Err(_) => StatusCode::SERVICE_UNAVAILABLE.into_response(),
    }
}

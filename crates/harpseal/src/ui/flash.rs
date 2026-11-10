//! The one-shot flash for a Module's own back-office handlers (#162): the
//! same cookie the Core's pages set, so the shell of the next page shows
//! the message once.

use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};

use crate::PageState;

/// A `303 See Other` to `to` that shows the text of catalog key `key` once,
/// at the top of the next back-office page ([`super::back_office`] draws
/// it in the Staff member's Locale; green when `ok`, red otherwise), as the
/// Core's own pages do after a save. Use it after a POST instead of a
/// `?done=` query: a reload does not show the message again.
///
/// `key` is a catalog key the Module registered (letters, digits, `.` and
/// `_`; any other key shows nothing). `to` should be a back-office path
/// (the flash cookie is scoped to `/admin`).
///
/// ```
/// use harpseal::web::axum::extract::State;
/// use harpseal::web::axum::response::Response;
/// use harpseal::{AppState, CurrentStaff, ui};
///
/// async fn cancel(State(s): State<AppState>, _staff: CurrentStaff) -> Response {
///     // ... the work; on failure `ui::service_error(&s, &staff, &e)`.
///     ui::redirect_with_flash(&s, "/admin/fees", true, "fees.cancelled")
/// }
/// ```
pub fn redirect_with_flash(s: &PageState, to: &str, ok: bool, key: &str) -> Response {
    let secure = s.config.staff.cookie_secure;
    let mut resp = (StatusCode::SEE_OTHER, [(header::LOCATION, to)]).into_response();
    resp.headers_mut().append(
        header::SET_COOKIE,
        crate::auth::flash_cookie(ok, key, secure),
    );
    resp
}

//! The one-shot flash for a Module's own back-office handlers (#162): the
//! same cookie the Core's pages set, so the shell of the next page shows
//! the message once.

use axum::http::HeaderMap;
use axum::response::Response;

use crate::PageState;

/// Where a flash redirect may go: a path under `/admin`.
const HOME: &str = crate::deployment::ADMIN;

/// A redirect to `to` that shows the text of catalog key `key` once, at
/// the top of the next back-office page ([`super::back_office`] draws it in
/// the Staff member's Locale; green when `ok`, red otherwise), as the
/// Core's own pages do after a save. Use it after a POST instead of a
/// `?done=` query: a reload does not show the message again.
///
/// - Pass the request's `headers` (extract `HeaderMap`): a plain request
///   gets `303 See Other`, an HTMX request (`HX-Request`) `200` with
///   `HX-Redirect`, as the Core's own redirects do.
/// - `to` must be a path under `/admin` (`/admin`, `/admin/…`, `/admin?…`);
///   anything else (another site, `//host`, a relative path) goes to
///   `/admin` instead. Characters outside printable ASCII are
///   percent-encoded.
/// - `key` is a catalog key the Module registered, made of ASCII letters,
///   digits, `.` and `_`; any other key sets no flash. A well-formed key
///   the catalog does not know shows as the key itself (debug builds
///   assert it is known).
///
/// ```
/// use harpseal::web::axum;
/// use axum::extract::State;
/// use axum::http::HeaderMap;
/// use axum::response::Response;
/// use harpseal::{AppState, CurrentStaff, ui};
///
/// async fn cancel(State(s): State<AppState>, _staff: CurrentStaff, headers: HeaderMap) -> Response {
///     // ... the work; on failure `ui::service_error(&s, &staff, &e)`.
///     ui::redirect_with_flash(&s, &headers, "/admin/fees", true, "fees.cancelled")
/// }
/// ```
pub fn redirect_with_flash(
    s: &PageState,
    headers: &HeaderMap,
    to: &str,
    ok: bool,
    key: &str,
) -> Response {
    debug_assert!(s.catalog.knows(key), "unknown flash key {key}");
    let secure = s.config.staff.cookie_secure;
    let cookie = crate::auth::flash_cookie(ok, key, secure);
    crate::auth::redirect(headers, &admin_path(to), Some(cookie))
}

/// `to` if it is a path under `/admin`, else `/admin`; bytes outside
/// printable ASCII percent-encoded, so the header value always builds.
fn admin_path(to: &str) -> String {
    let under = to
        .strip_prefix(HOME)
        .is_some_and(|rest| rest.is_empty() || rest.starts_with(['/', '?', '#']));
    if !under || to.contains('\\') {
        return HOME.to_string();
    }
    let mut out = String::with_capacity(to.len());
    for b in to.bytes() {
        if (0x21..0x7f).contains(&b) {
            out.push(char::from(b));
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::admin_path;

    #[test]
    fn only_paths_under_admin_are_kept() {
        for (to, want) in [
            ("/admin/fees", "/admin/fees"),
            ("/admin", "/admin"),
            ("/admin?x=1", "/admin?x=1"),
            ("//evil.example", "/admin"),
            ("https://evil.example", "/admin"),
            ("/\\evil.example", "/admin"),
            ("/admin\\evil", "/admin"),
            ("/administrator", "/admin"),
            ("fees", "/admin"),
            (
                "/admin/ค่า ธรรม",
                "/admin/%E0%B8%84%E0%B9%88%E0%B8%B2%20%E0%B8%98%E0%B8%A3%E0%B8%A3%E0%B8%A1",
            ),
        ] {
            assert_eq!(admin_path(to), want, "{to}");
        }
    }
}

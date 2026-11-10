//! Shared pieces of the admin pages: texts in the Staff member's Locale,
//! times and date filters in the Deployment's zone, HTMX fragments,
//! pagers and responses.

use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::{Html, IntoResponse, Response};
use maud::{Markup, html};
use serde_json::Value as Json;

use crate::audit::{self, Actor, RequestCtx, Target};
use crate::kernel::students::view::is_htmx;
use crate::{CurrentStaff, Error, PageState, Result};

/// Rows per list page.
pub(super) const PER_PAGE: i64 = 50;

/// Texts and settings one page render needs.
pub(super) struct View<'a> {
    pub(super) s: &'a PageState,
    pub(super) staff: &'a CurrentStaff,
}

impl<'a> View<'a> {
    pub(super) fn new(s: &'a PageState, staff: &'a CurrentStaff) -> Self {
        Self { s, staff }
    }

    /// `key` in the Staff member's Locale.
    pub(super) fn t(&self, key: &str) -> String {
        self.s.catalog.t(&self.staff.locale, key)
    }

    /// The name Staff read for Role `name` (#125): its `kernel.role.<name>`
    /// text, else the stored name.
    pub(super) fn role(&self, name: &str) -> String {
        crate::auth::roles::label(&self.s.catalog, &self.staff.locale, name)
    }

    /// `ms` as a Thai date and time (`9 พ.ย. 2569 14:30`) in the
    /// Deployment's time zone.
    pub(super) fn time(&self, ms: i64) -> String {
        crate::ui::thai_datetime(ms, self.offset())
    }

    fn offset(&self) -> i32 {
        self.s.config.utc_offset_minutes().unwrap_or(0)
    }

    /// `[from, to)` in ms for the date filters (`YYYY-MM-DD`, local days,
    /// `to` inclusive). An unparseable date is ignored.
    pub(super) fn range(&self, from: &str, to: &str) -> (Option<i64>, Option<i64>) {
        let start = |d: &str| day_start_ms(d.trim(), self.offset());
        (start(from), start(to).map(|ms| ms + 86_400_000))
    }

    /// A whole back-office page.
    pub(super) fn page(&self, status: StatusCode, title: &str, body: Markup) -> Response {
        let page = crate::ui::back_office(self.s, self.staff, title, body);
        (status, Html(page.into_string())).into_response()
    }

    /// The fragment alone for HTMX (not for a history restore, which swaps
    /// the whole body), else the page from `full`; `Vary: HX-Request`.
    pub(super) fn listing(
        &self,
        headers: &HeaderMap,
        fragment: Markup,
        full: impl FnOnce(Markup) -> Response,
    ) -> Response {
        let restore = headers.contains_key("hx-history-restore-request");
        let mut resp = if is_htmx(headers) && !restore {
            (StatusCode::OK, Html(fragment.into_string())).into_response()
        } else {
            full(fragment)
        };
        resp.headers_mut()
            .insert(header::VARY, HeaderValue::from_static("HX-Request"));
        resp
    }

    /// 404 inside the layout.
    pub(super) fn not_found(&self, back: &str) -> Response {
        let text = self.t("kernel.admin.not_found");
        let body = html! {
            div class="card" {
                p { (text) }
                a class="btn" href=(back) { (self.t("kernel.admin.back")) }
            }
        };
        self.page(StatusCode::NOT_FOUND, &text, body)
    }

    /// 422 inside the layout: a form arrived without a field it needs.
    pub(super) fn bad_form(&self) -> Response {
        let text = self.t("kernel.admin.bad_form");
        self.page(
            StatusCode::UNPROCESSABLE_ENTITY,
            &text,
            html! { div class="card" { p { (text) } } },
        )
    }

    /// 500 inside the layout; the error text is never shown.
    pub(super) fn error(&self, _e: &Error) -> Response {
        let text = self.t("kernel.auth.error");
        self.page(
            StatusCode::INTERNAL_SERVER_ERROR,
            &text,
            html! { div class="card" { p { (text) } } },
        )
    }

    /// 303 to `to` with a one-shot flash (`ok`, catalog key).
    pub(super) fn redirect(&self, to: &str, ok: bool, key: &str) -> Response {
        let secure = self.s.config.staff.cookie_secure;
        let mut resp = (StatusCode::SEE_OTHER, [(header::LOCATION, to)]).into_response();
        resp.headers_mut().append(
            header::SET_COOKIE,
            crate::auth::flash_cookie(ok, key, secure),
        );
        resp
    }

    /// Logs a sensitive read (no before/after): one row per request.
    pub(super) async fn log_read(
        &self,
        action: &str,
        target: Target,
        ctx: &RequestCtx,
    ) -> Result<()> {
        let actor = Actor::staff(self.staff);
        audit::record(
            &self.s.services,
            &actor,
            action,
            Some(&target),
            None,
            None,
            ctx,
        )
        .await
    }

    /// Prev / next links; `href(page)` builds each URL.
    pub(super) fn pager(&self, page: i64, pages: i64, href: impl Fn(i64) -> String) -> Markup {
        html! {
            @if pages > 1 {
                nav class="pager" aria-label=(self.t("kernel.admin.page")) {
                    @if page > 1 {
                        a class="btn secondary" rel="prev" href=(href(page - 1)) { (self.t("kernel.admin.prev")) }
                    }
                    span class="muted" { (self.t("kernel.admin.page")) " " (page) " / " (pages) }
                    @if page < pages {
                        a class="btn secondary" rel="next" href=(href(page + 1)) { (self.t("kernel.admin.next")) }
                    }
                }
            }
        }
    }

    /// The submit and reset row of a filter form.
    pub(super) fn filter_buttons(&self, reset: &str) -> Markup {
        html! {
            div class="actions" {
                button type="submit" { (self.t("kernel.admin.filter")) }
                a class="btn secondary" href=(reset) { (self.t("kernel.admin.clear")) }
            }
        }
    }
}

/// Pages for `total` rows and the requested page clamped to them.
pub(super) fn paging(total: i64, requested: &str) -> (i64, i64) {
    let pages = ((total + PER_PAGE - 1) / PER_PAGE).max(1);
    let page = requested.parse::<i64>().unwrap_or(1).clamp(1, pages);
    (page, pages)
}

/// Form value cut to `max` characters, trimmed.
pub(super) fn cut(v: &str, max: usize) -> String {
    v.trim().chars().take(max).collect()
}

/// Percent-encodes a query value (all but ASCII letters, digits, `-._~`).
pub(super) fn enc(s: &str) -> String {
    s.bytes()
        .map(|b| {
            if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~') {
                (b as char).to_string()
            } else {
                format!("%{b:02X}")
            }
        })
        .collect()
}

/// Top-level keys of a JSON object as `(key, value text)`, redacted;
/// a non-object is one row with an empty key.
pub(super) fn pairs(v: &Json) -> Vec<(String, String)> {
    match audit::redact(v.clone()) {
        Json::Object(m) => m.into_iter().map(|(k, v)| (k, shown(&v))).collect(),
        Json::Null => Vec::new(),
        other => vec![(String::new(), shown(&other))],
    }
}

/// A JSON value as shown to Staff: strings bare, the rest as JSON.
pub(super) fn shown(v: &Json) -> String {
    match v {
        Json::String(s) => s.clone(),
        other => other.to_string(),
    }
}

/// `external_id` shortened to its first 6 and last 4 characters.
pub(super) fn short_id(id: &str) -> String {
    let n = id.chars().count();
    if n <= 12 {
        return id.to_string();
    }
    let head: String = id.chars().take(6).collect();
    let tail: String = id.chars().skip(n - 4).collect();
    format!("{head}…{tail}")
}

/// Local midnight of `YYYY-MM-DD` at `offset_minutes` east, in ms UTC.
pub(super) fn day_start_ms(date: &str, offset_minutes: i32) -> Option<i64> {
    let mut it = date.splitn(3, '-');
    let (y, m, d) = (it.next()?, it.next()?, it.next()?);
    if y.len() != 4 || m.len() != 2 || d.len() != 2 {
        return None;
    }
    let (y, m, d): (i64, i64, i64) = (y.parse().ok()?, m.parse().ok()?, d.parse().ok()?);
    if !(1..=12).contains(&m) || !(1..=31).contains(&d) {
        return None;
    }
    // Howard Hinnant's days-from-civil.
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    Some((days * 86_400 - i64::from(offset_minutes) * 60) * 1000)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_days_follow_the_offset() {
        // 2026-11-05T00:00:00Z
        assert_eq!(day_start_ms("2026-11-05", 0), Some(1_793_836_800_000));
        assert_eq!(
            day_start_ms("2026-11-05", 420),
            Some(1_793_836_800_000 - 7 * 3_600_000)
        );
        assert_eq!(day_start_ms("2000-02-29", 0), Some(951_782_400_000));
        assert_eq!(day_start_ms("2026-13-01", 0), None);
        assert_eq!(day_start_ms("26-11-05", 0), None);
        assert_eq!(day_start_ms("", 0), None);
    }

    #[test]
    fn helpers() {
        assert_eq!(short_id("U1234567890abcdef1234"), "U12345…1234");
        assert_eq!(short_id("short"), "short");
        assert_eq!(paging(0, ""), (1, 1));
        assert_eq!(paging(101, "9"), (3, 3));
        let p = pairs(&serde_json::json!({ "a": 1, "password_hash": "x" }));
        assert_eq!(p[0], ("a".into(), "1".into()));
        assert_eq!(p[1].1, crate::audit::REDACTED);
    }
}

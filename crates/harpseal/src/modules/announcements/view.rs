//! Shared pieces of the Announcements pages: texts in the Staff member's
//! Locale, local times, the layout, responses and status chips.

use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::{Html, IntoResponse, Response};
use maud::{Markup, html};

use crate::kernel::students::{format_ms, is_htmx};
use crate::{CurrentStaff, Error, PageState};

/// Texts and settings one page render needs.
pub(crate) struct View<'a> {
    pub(crate) s: &'a PageState,
    pub(crate) staff: &'a CurrentStaff,
}

impl<'a> View<'a> {
    pub(crate) fn new(s: &'a PageState, staff: &'a CurrentStaff) -> Self {
        Self { s, staff }
    }

    /// `key` in the Staff member's Locale.
    pub(crate) fn t(&self, key: &str) -> String {
        self.s.catalog.t(&self.staff.locale, key)
    }

    /// The Deployment's offset from UTC, in minutes.
    pub(crate) fn offset(&self) -> i32 {
        self.s.config.utc_offset_minutes().unwrap_or(0)
    }

    /// `ms` as a Thai date and time (`9 พ.ย. 2569 14:30`), or "-".
    pub(crate) fn time(&self, ms: Option<i64>) -> String {
        ms.map_or_else(
            || "-".into(),
            |ms| crate::ui::thai_datetime(ms, self.offset()),
        )
    }

    /// A whole back-office page.
    pub(crate) fn page(&self, status: StatusCode, title: &str, body: Markup) -> Response {
        let page = crate::ui::back_office(self.s, self.staff, title, body);
        (status, Html(page.into_string())).into_response()
    }

    /// 404 inside the layout.
    pub(crate) fn not_found(&self) -> Response {
        let text = self.t("announcements.page.not_found");
        let body = html! {
            div class="card" {
                p { (text) }
                a class="btn" href="/admin/announcements" { (self.t("announcements.page.back")) }
            }
        };
        self.page(StatusCode::NOT_FOUND, &text, body)
    }

    /// 500 inside the layout; the error text is never shown.
    pub(crate) fn error(&self, _e: &Error) -> Response {
        let text = self.t("kernel.auth.error");
        self.page(
            StatusCode::INTERNAL_SERVER_ERROR,
            &text,
            html! { div class="card" { p { (text) } } },
        )
    }

    /// 303 to `to` with a one-shot flash (`ok`, catalog key).
    pub(crate) fn redirect(&self, to: &str, ok: bool, key: &str) -> Response {
        let secure = self.s.config.staff.cookie_secure;
        let mut resp = (StatusCode::SEE_OTHER, [(header::LOCATION, to)]).into_response();
        resp.headers_mut().append(
            header::SET_COOKIE,
            crate::auth::flash_cookie(ok, key, secure),
        );
        resp
    }

    /// Status chip of an announcement.
    pub(crate) fn status_chip(&self, status: &str) -> Markup {
        let class = match status {
            "sent" => "chip ok",
            "retracted" => "chip err",
            _ => "chip",
        };
        html! { span class=(class) { (self.t(&format!("announcements.status.{status}"))) } }
    }

    /// "หมดอายุแล้ว" next to the status of a sent announcement whose
    /// `expires_at` has passed (#77): Students no longer open it.
    pub(crate) fn expired_chip(&self, row: &super::store::Row) -> Markup {
        let now = self.s.services.now_ms();
        let gone = matches!(row.status.as_str(), "sending" | "sent")
            && row.expires_at.is_some_and(|e| e <= now);
        html! { @if gone { " " span class="chip warn" { (self.t("announcements.page.expired")) } } }
    }
}

/// A fragment for an HTMX request (not a history restore), else the full
/// page; either way `Vary: HX-Request`.
pub(crate) fn fragment_or_page(
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

/// Days since 1970-01-01 of a civil date (Howard Hinnant).
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

fn date_parts(s: &str) -> Option<(i64, i64, i64)> {
    let mut it = s.splitn(3, '-');
    let y: i64 = it.next()?.parse().ok()?;
    let m: i64 = it.next()?.parse().ok()?;
    let d: i64 = it.next()?.parse().ok()?;
    let ok = (1970..=9999).contains(&y) && (1..=12).contains(&m) && (1..=31).contains(&d);
    // Round-trip to refuse 2026-02-31.
    let days = days_from_civil(y, m, d);
    let back = format_ms(days * 86_400_000, 0);
    (ok && back.starts_with(&format!("{y:04}-{m:02}-{d:02}"))).then_some((y, m, d))
}

/// `YYYY-MM-DD` local → the instant the next local day starts (an
/// expiry at the end of that day).
pub(crate) fn end_of_day(s: &str, offset_minutes: i32) -> Option<i64> {
    let (y, m, d) = date_parts(s.trim())?;
    Some((days_from_civil(y, m, d) + 1) * 86_400_000 - i64::from(offset_minutes) * 60_000)
}

/// `YYYY-MM-DDTHH:MM` local (an `<input type=datetime-local>`) → UTC ms.
pub(crate) fn local_datetime(s: &str, offset_minutes: i32) -> Option<i64> {
    let (date, time) = s.trim().split_once('T')?;
    let (y, m, d) = date_parts(date)?;
    let (h, min) = time.get(..5)?.split_once(':')?;
    let (h, min): (i64, i64) = (h.parse().ok()?, min.parse().ok()?);
    if !(0..24).contains(&h) || !(0..60).contains(&min) {
        return None;
    }
    let local = days_from_civil(y, m, d) * 86_400_000 + (h * 60 + min) * 60_000;
    Some(local - i64::from(offset_minutes) * 60_000)
}

/// UTC ms → `YYYY-MM-DDTHH:MM` local, for a `datetime-local` value.
pub(crate) fn input_datetime(ms: i64, offset_minutes: i32) -> String {
    format_ms(ms, offset_minutes).replacen(' ', "T", 1)
}

/// The last local day before `expires_at` (the form's date value).
pub(crate) fn input_date(expires_at: i64, offset_minutes: i32) -> String {
    format_ms(expires_at - 1, offset_minutes)
        .chars()
        .take(10)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_times_round_trip_in_the_offset() {
        // 2026-11-05T00:00:00Z is 07:00 in Bangkok.
        let ms = 1_793_836_800_000;
        assert_eq!(local_datetime("2026-11-05T07:00", 420), Some(ms));
        assert_eq!(input_datetime(ms, 420), "2026-11-05T07:00");
        assert_eq!(local_datetime("2026-02-31T07:00", 420), None);
        assert_eq!(local_datetime("2026-11-05T24:00", 420), None);
        let end = end_of_day("2026-11-05", 420).unwrap();
        assert_eq!(input_datetime(end, 420), "2026-11-06T00:00");
        assert_eq!(input_date(end, 420), "2026-11-05");
        assert_eq!(end_of_day("nope", 420), None);
    }
}

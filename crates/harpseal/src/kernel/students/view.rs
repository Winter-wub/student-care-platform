//! Shared pieces of the Students pages: texts in the Staff member's
//! Locale, times in the Deployment's zone, Link icons and responses.

use axum::http::{HeaderValue, StatusCode, header};
use axum::response::{Html, IntoResponse, Response};
use maud::{Markup, html};

use crate::config::AttributeSpec;
use crate::{CurrentStaff, Error, PageState};

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

    /// `ms` as a Thai date and time in the Deployment's time zone.
    pub(super) fn time(&self, ms: i64) -> String {
        crate::ui::local_datetime(self.s, ms)
    }

    /// An attribute's label in the Staff member's Locale.
    pub(super) fn label<'s>(&self, key: &'s str, spec: &'s AttributeSpec) -> &'s str {
        spec.label_in(key, &self.staff.locale, &self.s.config.default_locale)
    }

    /// A stored attribute value as Staff read it: an enum's label, a date
    /// as `1 มิ.ย. 2568` (#125).
    pub(super) fn shown(&self, spec: &AttributeSpec, value: Option<&serde_json::Value>) -> String {
        let shown = spec.shown(value, &self.staff.locale, &self.s.config.default_locale);
        if spec.kind == crate::config::AttributeType::Date {
            crate::ui::thai_day(&shown)
        } else {
            shown
        }
    }

    /// The label of the enum value `value` in the Staff member's Locale.
    pub(super) fn value_label<'s>(&self, spec: &'s AttributeSpec, value: &'s str) -> &'s str {
        spec.value_label(value, &self.staff.locale, &self.s.config.default_locale)
    }

    /// A whole back-office page.
    pub(super) fn page(&self, status: StatusCode, title: &str, body: Markup) -> Response {
        let page = crate::ui::back_office(self.s, self.staff, title, body);
        (status, Html(page.into_string())).into_response()
    }

    /// 404 inside the layout.
    pub(super) fn not_found(&self) -> Response {
        let text = self.t("kernel.students.not_found");
        let body = html! {
            div class="card" {
                p { (text) }
                a class="btn" href="/admin/students" { (self.t("kernel.students.back")) }
            }
        };
        self.page(StatusCode::NOT_FOUND, &text, body)
    }

    /// 500 inside the layout; the error text is never shown (it may name
    /// a column).
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

    /// Status chip of a Student.
    pub(super) fn status_chip(&self, status: &str) -> Markup {
        let (class, key) = if status == "left" {
            ("chip", "kernel.students.status_left")
        } else {
            ("chip ok", "kernel.students.status_active")
        };
        html! { span class=(class) { (self.t(key)) } }
    }

    /// One Channel's Link icon: ● active, ◐ unreachable, ○ none.
    pub(super) fn link_icon(&self, channel: &str, status: Option<&str>) -> Markup {
        let (glyph, class, key) = match status {
            Some("active") => ("●", "li li-active", "kernel.students.link_active"),
            Some("unreachable") => ("◐", "li li-unreachable", "kernel.students.link_unreachable"),
            _ => ("○", "li li-none", "kernel.students.link_none"),
        };
        let label = format!("{}: {}", channel_label(channel), self.t(key));
        html! {
            span class=(class) title=(label) aria-label=(label) {
                span aria-hidden="true" { (glyph) } " " (channel_label(channel))
            }
        }
    }
}

/// A Channel kind as shown to Staff (`line` → `LINE`).
pub(crate) fn channel_label(kind: &str) -> String {
    match kind {
        "line" => "LINE".into(),
        "telegram" => "Telegram".into(),
        other => other.to_string(),
    }
}

/// `true` for an HTMX request (`HX-Request` header).
pub(crate) fn is_htmx(headers: &axum::http::HeaderMap) -> bool {
    headers.get("hx-request") == Some(&HeaderValue::from_static("true"))
}

/// `ms` (UTC) at `offset_minutes` east, as `YYYY-MM-DD HH:MM`.
pub(crate) fn format_ms(ms: i64, offset_minutes: i32) -> String {
    let local = ms.div_euclid(1000) + i64::from(offset_minutes) * 60;
    let (days, secs) = (local.div_euclid(86_400), local.rem_euclid(86_400));
    // Howard Hinnant's civil-from-days.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!(
        "{y:04}-{m:02}-{d:02} {:02}:{:02}",
        secs / 3600,
        (secs % 3600) / 60
    )
}

#[cfg(test)]
mod tests {
    use super::format_ms;

    #[test]
    fn times_follow_the_offset() {
        // 2026-11-05T00:00:00Z
        let ms = 1_793_836_800_000;
        assert_eq!(format_ms(ms, 0), "2026-11-05 00:00");
        assert_eq!(format_ms(ms, 420), "2026-11-05 07:00");
        assert_eq!(format_ms(ms - 60_000, 0), "2026-11-04 23:59");
        assert_eq!(format_ms(951_782_400_000, 0), "2000-02-29 00:00");
        assert_eq!(format_ms(0, -60), "1969-12-31 23:00");
    }
}

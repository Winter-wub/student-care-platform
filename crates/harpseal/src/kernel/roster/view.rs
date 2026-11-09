//! Shared pieces of the roster import pages: texts in the Staff member's
//! Locale, times, responses and file downloads.

use axum::http::{HeaderValue, StatusCode, header};
use axum::response::{Html, IntoResponse, Response};
use maud::{Markup, html};

use super::store::Import;
use crate::{CurrentStaff, Error, PageState};

/// Where the pages live.
pub(super) const BASE: &str = "/admin/roster";
/// Permission to see imports and download reports and files.
pub(super) const VIEW: &str = "kernel.roster.view";
/// Permission to upload, confirm and cancel.
pub(crate) const IMPORT: &str = "kernel.roster.import";

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

    /// The state as Staff read it.
    pub(super) fn state(&self, state: &str) -> Markup {
        let class = match state {
            "done" => "chip ok",
            "failed" => "chip err",
            _ => "chip",
        };
        html! { span class=(class) { (self.t(&format!("kernel.roster.state_{state}"))) } }
    }

    /// The mode as Staff read it.
    pub(super) fn mode(&self, mode: &str) -> String {
        self.t(&format!("kernel.roster.mode_{mode}"))
    }

    /// A whole back-office page.
    pub(super) fn page(&self, status: StatusCode, title: &str, body: Markup) -> Response {
        let page = crate::ui::back_office(self.s, self.staff, title, body);
        (status, Html(page.into_string())).into_response()
    }

    /// 404 inside the layout.
    pub(super) fn not_found(&self) -> Response {
        let text = self.t("kernel.roster.not_found");
        let body = html! {
            div class="card" {
                p { (text) }
                a class="btn" href=(BASE) { (self.t("kernel.roster.back")) }
            }
        };
        self.page(StatusCode::NOT_FOUND, &text, body)
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

    /// `role="alert"` for the page's own message when no flash shows: a
    /// page announces one message per update (#129), and after a redirect
    /// the flash is that message.
    pub(super) fn alert(&self) -> Option<&'static str> {
        self.staff.flash.is_none().then_some("alert")
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
}

/// `true` for an HTMX request that swaps a fragment (not a history
/// restore, which needs the whole page).
pub(super) fn is_fragment(headers: &axum::http::HeaderMap) -> bool {
    headers.get("hx-request") == Some(&HeaderValue::from_static("true"))
        && !headers.contains_key("hx-history-restore-request")
}

/// A download: `content_type`, `no-store`, and `file_name` as an
/// attachment (ASCII `fallback` plus the UTF-8 name, RFC 6266).
pub(super) fn download(
    content_type: &'static str,
    file_name: &str,
    fallback: &str,
    body: Vec<u8>,
) -> Response {
    let encoded: String = file_name
        .bytes()
        .map(|b| {
            if b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'_') {
                (b as char).to_string()
            } else {
                format!("%{b:02X}")
            }
        })
        .collect();
    let disposition = format!("attachment; filename=\"{fallback}\"; filename*=UTF-8''{encoded}");
    let mut resp = (StatusCode::OK, body).into_response();
    let h = resp.headers_mut();
    h.insert(header::CONTENT_TYPE, HeaderValue::from_static(content_type));
    h.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    if let Ok(v) = HeaderValue::from_str(&disposition) {
        h.insert(header::CONTENT_DISPOSITION, v);
    }
    resp
}

/// UTF-8 CSV with a BOM, so Excel opens Thai text correctly.
pub(super) fn csv_body(lines: impl IntoIterator<Item = String>) -> Vec<u8> {
    let mut out = "\u{feff}".to_string();
    out.extend(lines);
    out.into_bytes()
}

/// The preview's counts next to what the import did (#46): the same
/// things counted the same way, so a difference is a change made between
/// the preview and the run.
pub(super) fn compared(v: &View<'_>, imp: &Import) -> Markup {
    let (p, r) = (&imp.counts.preview, &imp.counts.result);
    let mut lines = vec![
        ("new", "kernel.roster.new", p.tally.new, r.new),
        (
            "updated",
            "kernel.roster.updated",
            p.tally.updated,
            r.updated,
        ),
        (
            "unchanged",
            "kernel.roster.unchanged",
            p.tally.unchanged,
            r.unchanged,
        ),
        (
            "returning",
            "kernel.roster.returning",
            p.tally.returning,
            r.returning,
        ),
        (
            "skipped_by_override",
            "kernel.roster.skipped_by_override",
            p.tally.skipped_by_override,
            r.skipped_by_override,
        ),
    ];
    if imp.mode == "full" {
        let c = &imp.counts;
        lines.push(("left", "kernel.roster.left", p.will_become_left, c.left));
        let kept = ("kept_by_status", "kernel.roster.kept_by_status");
        lines.push((kept.0, kept.1, p.kept_by_status, c.kept_by_status));
    }
    let (in_preview, applied) = (
        v.t("kernel.roster.in_preview"),
        v.t("kernel.roster.applied"),
    );
    html! {
        table class="rows compared" {
            thead { tr { th {} th { (in_preview) } th { (applied) } } }
            tbody {
                @for (name, key, before, after) in lines {
                    tr data-count=(name) {
                        th scope="row" { (v.t(key)) }
                        td data-label=(in_preview) data-preview { (before) }
                        td data-label=(applied) data-result { (after) }
                    }
                }
            }
        }
    }
}

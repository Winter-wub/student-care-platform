//! Shared pieces of `/admin/branding` (#114): texts, responses, the
//! wording of a refused Menu image and the tap-area preview.

use axum::http::{StatusCode, header};
use axum::response::{Html, IntoResponse, Response};
use maud::{Markup, html};

use super::check::Refused;
use crate::backup::value::base64_encode;
use crate::channel::{MenuImageProblem, MenuLayout};
use crate::{CurrentStaff, PageState};

/// Texts and responses of one request.
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

    /// A refusal's text.
    pub(super) fn refused(&self, r: &Refused) -> String {
        r.text(|k| self.t(k))
    }

    /// A whole back-office page.
    pub(super) fn page(&self, status: StatusCode, body: Markup) -> Response {
        let title = self.t("kernel.branding.title");
        let page = crate::ui::back_office(self.s, self.staff, &title, body);
        (status, Html(page.into_string())).into_response()
    }

    /// 303 to the page with a one-shot flash (`ok`, catalog key).
    pub(super) fn redirect(&self, ok: bool, key: &str) -> Response {
        let to = format!("/admin{}", super::PATH);
        let secure = self.s.config.staff.cookie_secure;
        let mut resp = (StatusCode::SEE_OTHER, [(header::LOCATION, to)]).into_response();
        resp.headers_mut().append(
            header::SET_COOKIE,
            crate::auth::flash_cookie(ok, key, secure),
        );
        resp
    }

    /// 500 inside the layout; the error text is never shown.
    pub(super) fn error(&self) -> Response {
        let text = self.t("kernel.auth.error");
        let body = html! { div class="card" { p role="alert" { (text) } } };
        self.page(StatusCode::INTERNAL_SERVER_ERROR, body)
    }

    /// Why a Menu image was refused, in the Staff member's Locale.
    pub(super) fn problem(&self, p: &MenuImageProblem) -> String {
        let size = |(w, h): (u32, u32)| format!("{w} x {h}");
        let fill = |key: &str, args: &[(&str, String)]| {
            let mut s = self.t(key);
            for (n, v) in args {
                s = s.replace(&format!("{{{n}}}"), v);
            }
            s
        };
        match p {
            MenuImageProblem::NotImage => self.t("kernel.branding.err_menu_type"),
            MenuImageProblem::TooLarge { max_bytes } => fill(
                "kernel.branding.err_menu_large",
                &[("mb", (max_bytes / (1024 * 1024)).max(1).to_string())],
            ),
            MenuImageProblem::Size {
                width,
                height,
                allowed,
            } => {
                let allowed: Vec<String> = allowed.iter().map(|a| size(*a)).collect();
                fill(
                    "kernel.branding.err_menu_size",
                    &[
                        ("w", width.to_string()),
                        ("h", height.to_string()),
                        ("allowed", allowed.join(", ")),
                    ],
                )
            }
            MenuImageProblem::Grid { rows, want } => fill(
                "kernel.branding.err_menu_grid",
                &[("rows", rows.to_string()), ("want", size(*want))],
            ),
            _ => self.t("kernel.branding.err_menu_unused"),
        }
    }
}

/// `bytes` as a `data:` URL of `content_type`, for a preview of a file
/// not saved yet.
pub(super) fn data_url(content_type: &str, bytes: &[u8]) -> String {
    format!("data:{content_type};base64,{}", base64_encode(bytes))
}

/// The image at `src` with the tap areas of `layout` drawn over it, each
/// boxed and labelled.
pub(super) fn areas(src: &str, layout: &MenuLayout, caption: &str) -> Markup {
    let view_box = format!("0 0 {} {}", layout.width, layout.height);
    html! {
        figure class="menu-preview" {
            svg xmlns="http://www.w3.org/2000/svg" viewBox=(view_box) role="img"
                aria-label=(caption) {
                image href=(src) x="0" y="0" width=(layout.width) height=(layout.height) {}
                @for a in &layout.areas {
                    rect x=(a.x) y=(a.y) width=(a.width) height=(a.height) {}
                    text x=(a.x + 40) y=(a.y + 110) { (a.label) }
                }
            }
            figcaption class="muted" { (caption) }
        }
    }
}

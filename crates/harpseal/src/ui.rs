//! Shared page layout for back-office and Mini App pages (issue #5): one
//! maud layout, one CSS file served at [`CSS_PATH`], HTMX from a pinned
//! cdnjs URL with Subresource Integrity. No JS framework, no build step.
//!
//! Back-office handlers wrap their content with [`back_office`], which adds
//! the skip link, the brand bar (saved on `/admin/branding`, else
//! `[branding]` in the TOML), the navigation the Staff member may see,
//! grouped "งานประจำ" / "ผู้ดูแลระบบ" with the current page marked (#123;
//! a Module picks its page's group with
//! [`crate::ModuleRegistrar::back_office_page_in`]), the pending flash, the
//! usage banner slot and the CSRF header HTMX sends. Plain `<form method="post">` needs
//! `staff.csrf_field()` inside.
//!
//! Layout helpers in `ui.css`: `.card`, `.field`, `.row2`, `.cols`,
//! `.list`, `.chip`, and `table.rows` (stacked cards below 900 px, a table
//! from 900 px; give each `td` a `data-label`). Buttons: the default
//! (brand), `.secondary`, `.done`, `.link`, and [`danger_button`].

use axum::http::StatusCode;
use axum::response::{Html, IntoResponse, Response};
use maud::{DOCTYPE, Markup, PreEscaped, html};

use crate::{CurrentStaff, Error, PageState};

mod shell;

/// The stylesheet, served by `Deployment::router` at [`CSS_PATH`]:
/// `ui.css`, then the back-office shell's `ui_shell.css` (#123).
pub const CSS: &str = concat!(include_str!("ui.css"), include_str!("ui_shell.css"));
/// Bump the query string when the CSS changes (it is cached for a day).
pub const CSS_PATH: &str = "/static/app.css";
const CSS_HREF: &str = "/static/app.css?v=15";
/// Pinned HTMX build on cdnjs.
pub const HTMX_URL: &str = "https://cdnjs.cloudflare.com/ajax/libs/htmx/2.0.4/htmx.min.js";
/// SRI hash of [`HTMX_URL`] (`openssl dgst -sha384 -binary | base64`).
pub const HTMX_SRI: &str =
    "sha384-HGfztofotfshcF7+8n44JQL2oJmowVChPTg48S+jvZoztPfvwD79OC/LTtG6dMp+";

/// Which chrome a page gets.
#[non_exhaustive]
pub enum Shell {
    /// Inside LINE LIFF / Telegram Web App: no navigation, the host app
    /// already draws a title bar.
    MiniApp,
    /// Brand bar only: sign-in and error pages.
    Plain,
}

/// A short message shown once at the top of the main column.
pub struct Flash<'a> {
    /// Success (green) or failure (red).
    pub ok: bool,
    /// The message.
    pub text: &'a str,
}

/// Renders a [`Flash`].
pub fn flash(f: &Flash<'_>) -> Markup {
    html! {
        div class=(if f.ok { "flash ok" } else { "flash err" }) role="status" { (f.text) }
    }
}

/// The submit button of an action in proportion to its reach (#120): one
/// that reaches every Student or makes many of them lose their Links.
/// Refused Ink with white text (9.3:1), never the brand colour, 48 px
/// tall, with its own focus ring. Name the reach in `label` ("ยืนยัน: 120
/// คนจะพ้นสภาพ"), and check any typed confirmation on the server.
pub fn danger_button(label: &str) -> Markup {
    html! { button class="danger" type="submit" { (label) } }
}

/// A "ข้อมูลทางเทคนิค" disclosure (#123): raw details an administrator
/// may need (hashes, IPs, request ids, cron, table and file names),
/// closed by default so the page reads in plain Thai first.
pub(crate) fn technical(s: &PageState, staff: &CurrentStaff, body: Markup) -> Markup {
    let label = s.catalog.t(&staff.locale, "kernel.backoffice.technical");
    html! {
        details class="tech" {
            summary { (label) }
            div class="tech-body" { (body) }
        }
    }
}

/// The look: the values saved on `/admin/branding` (#114) over
/// `[branding]` in the TOML.
type Brand = crate::kernel::branding::Chrome;

fn brand_of(s: &PageState) -> Brand {
    let stored = s.deployment.branding().stored();
    Brand::of(&s.config, &stored)
}

/// Whole document. `csrf` goes into `hx-headers` on `<body>`.
fn document(brand: &Brand, title: &str, class: &str, csrf: Option<&str>, inner: Markup) -> Markup {
    let hx_headers = csrf.map(|t| format!(r#"{{"X-CSRF-Token":"{t}"}}"#));
    html! {
        (DOCTYPE)
        html lang="th" {
            head {
                meta charset="utf-8";
                meta name="viewport" content="width=device-width, initial-scale=1, viewport-fit=cover";
                meta name="color-scheme" content="light";
                title { (title) " · " (brand.name) }
                @if let Some(f) = &brand.font_url { link rel="stylesheet" href=(f); }
                link rel="stylesheet" href=(CSS_HREF);
                // `primary_color` is checked to be a CSS hex colour at load,
                // and a saved one to be `#rrggbb`.
                style { (PreEscaped(format!(":root{{--brand:{}}}", brand.color))) }
                script src=(HTMX_URL) integrity=(HTMX_SRI) crossorigin="anonymous"
                    referrerpolicy="no-referrer" defer {}
            }
            body class=(class) hx-headers=[hx_headers] { (inner) }
        }
    }
}

fn brand_link(brand: &Brand, href: &str) -> Markup {
    html! {
        a class="brand" href=(href) {
            @if let Some(logo) = &brand.logo { img class="logo" src=(logo) alt=""; }
            span { (brand.name) }
        }
    }
}

/// The Mini App's slim header: the logo and the name, in the brand colour.
fn mini_bar(brand: &Brand) -> Markup {
    html! {
        header class="mini-bar" {
            @if let Some(logo) = &brand.logo { img class="logo" src=(logo) alt=""; }
            span { (brand.name) }
        }
    }
}

/// The back-office and Mini App headers drawn with `name`, `color` (a
/// checked `#rrggbb`) and the logo at `logo` (a URL or a `data:` URL), for
/// the preview of `/admin/branding` (#114). Nothing is saved.
pub(crate) fn preview_bars(
    name: &str,
    color: &str,
    logo: Option<&str>,
    office: &str,
    mini: &str,
) -> Markup {
    let brand = Brand {
        name: name.to_string(),
        color: color.to_string(),
        logo: logo.map(str::to_string),
        font_url: None,
    };
    html! {
        div class="brand-preview" style=(format!("--brand:{color}")) {
            p class="muted" { (office) }
            div class="bar" { (brand_link(&brand, "#")) }
            p class="muted" { (mini) }
            div class="mini-frame" { (mini_bar(&brand)) }
        }
    }
}

/// A back-office page for the signed-in Staff member.
pub fn back_office(s: &PageState, staff: &CurrentStaff, title: &str, body: Markup) -> Markup {
    let brand = brand_of(s);
    let t = |k: &str| s.catalog.t(&staff.locale, k);
    let nav = !staff.must_change_password && !staff.nav().is_empty();
    let texts = shell::NavTexts {
        menu: t("kernel.backoffice.menu"),
        home: t("kernel.backoffice.home"),
        daily: t("kernel.backoffice.group_daily"),
        admin: t("kernel.backoffice.group_admin"),
    };
    let inner = html! {
        (shell::skip_link(&t("kernel.backoffice.skip")))
        header class="bar" {
            (brand_link(&brand, "/admin"))
            form method="post" action="/admin/logout" class="who" {
                (staff.csrf_field())
                span class="who-email" { (staff.email) }
                button class="link" type="submit" { (t("kernel.auth.sign_out")) }
            }
        }
        // The Quota banner (#26): empty below 80 % of every Quota.
        div id="usage-banner" { @if let Some(b) = &staff.usage_banner { (b) } }
        div class=(if nav { "office-shell" } else { "office-shell no-nav" }) {
            @if nav { (shell::nav(staff, &texts)) }
            main id=(shell::MAIN_ID) tabindex="-1" {
                @if let Some((ok, text)) = &staff.flash { (flash(&Flash { ok: *ok, text })) }
                (body)
            }
        }
    };
    document(&brand, title, "office", Some(staff.csrf_token()), inner)
}

/// What a back-office handler answers when a Core service it called (e.g.
/// `modules::announcements::create_draft`) failed, as the Core's pages
/// answer: [`Error::Forbidden`] gets the same 403 page the back-office
/// guard shows a Staff member without the route's Permission (naming the
/// Permission missing); [`Error::Conflict`] (the row changed since the
/// page showed it, e.g. a stale `version` to `confirm`) a 409 page asking
/// to reload; anything else a 500 page that never shows the error text (it
/// may name a column). Check the form yourself first: an
/// [`Error::Invalid`] here is a 500 too.
pub fn service_error(s: &PageState, staff: &CurrentStaff, e: &Error) -> Response {
    let (status, key) = match e.root() {
        Error::Forbidden { permission } => return crate::auth::forbidden(s, staff, permission),
        Error::Conflict(_) => (StatusCode::CONFLICT, "kernel.auth.conflict"),
        _ => (StatusCode::INTERNAL_SERVER_ERROR, "kernel.auth.error"),
    };
    let text = s.catalog.t(&staff.locale, key);
    let body = html! { div class="card" { p role="alert" { (text) } } };
    let page = back_office(s, staff, &text, body);
    (status, Html(page.into_string())).into_response()
}

/// A page with the brand bar and no navigation (sign-in, errors).
pub fn bare(s: &PageState, title: &str, body: Markup) -> Markup {
    let brand = brand_of(s);
    let inner = html! {
        header class="bar" { (brand_link(&brand, "/admin")) }
        main { (body) }
    };
    document(&brand, title, "office", None, inner)
}

/// A Mini App page with the Deployment's branding.
pub fn mini_app(s: &PageState, title: &str, body: Markup) -> Markup {
    let brand = brand_of(s);
    let inner = html! { (mini_bar(&brand)) (mini_inner(&brand.name, body)) };
    document(&brand, title, "mini", None, inner)
}

/// A Mini App page of an App Session (#28): `csrf` goes into the headers
/// HTMX sends, and the footer opens the notice inside the app shell with
/// `notice_label`.
pub fn mini_app_session(
    s: &PageState,
    title: &str,
    csrf: Option<&str>,
    notice_label: &str,
    body: Markup,
) -> Markup {
    let brand = brand_of(s);
    // A page of a live session checks it is this tab's before it is
    // painted, and ends the entry page's loop guard (#86).
    let body = html! {
        @if let Some(t) = csrf { script { (PreEscaped(crate::kernel::app::tab_js(t))) } }
        (body)
    };
    let inner = html! {
        (mini_bar(&brand))
        (mini_footer(&brand.name, "/notice?app=1", notice_label, body))
    };
    document(&brand, title, "mini", csrf, inner)
}

fn mini_inner(org: &str, body: Markup) -> Markup {
    mini_footer(org, "/notice", "ประกาศความเป็นส่วนตัว", body)
}

fn mini_footer(org: &str, notice: &str, label: &str, body: Markup) -> Markup {
    html! {
        main { (body) }
        footer class="foot" { (org) " · " a href=(notice) { (label) } }
    }
}

/// The whole document with the default theme. `org` is the Deployment
/// name. Prefer [`mini_app`] and [`bare`], which apply the branding.
pub fn page(org: &str, title: &str, shell: Shell, body: Markup) -> Markup {
    let brand = Brand {
        name: org.to_string(),
        color: crate::config::DEFAULT_PRIMARY.to_string(),
        logo: None,
        font_url: None,
    };
    match shell {
        Shell::MiniApp => document(&brand, title, "mini", None, mini_inner(org, body)),
        Shell::Plain => {
            let inner =
                html! { header class="bar" { (brand_link(&brand, "/admin")) } main { (body) } };
            document(&brand, title, "office", None, inner)
        }
    }
}

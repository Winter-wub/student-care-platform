//! Shared page layout for back-office and Mini App pages (issue #5): one
//! maud layout, one CSS file served at [`CSS_PATH`], HTMX from a pinned
//! cdnjs URL with Subresource Integrity. No JS framework, no build step.
//!
//! Back-office handlers wrap their content with [`back_office`], which adds
//! the brand bar (`[branding]` in the TOML), the navigation the Staff
//! member may see, the pending flash, the usage banner slot and the CSRF
//! header HTMX sends. Plain `<form method="post">` needs
//! `staff.csrf_field()` inside.
//!
//! Layout helpers in `ui.css`: `.card`, `.field`, `.row2`, `.cols`,
//! `.list`, `.chip`, and `table.rows` (stacked cards below 900 px, a table
//! from 900 px; give each `td` a `data-label`).

use maud::{DOCTYPE, Markup, PreEscaped, html};

use crate::{CurrentStaff, PageState};

/// The stylesheet, served by `Deployment::router` at [`CSS_PATH`].
pub const CSS: &str = include_str!("ui.css");
/// Bump the query string when `ui.css` changes (it is cached for a day).
pub const CSS_PATH: &str = "/static/app.css";
const CSS_HREF: &str = "/static/app.css?v=2";
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

/// The look from `[branding]`.
struct Brand<'a> {
    name: &'a str,
    color: &'a str,
    logo: Option<&'a str>,
    font_url: Option<&'a str>,
}

impl<'a> Brand<'a> {
    fn of(s: &'a PageState) -> Self {
        let b = &s.config.branding;
        Brand {
            name: b.name.as_deref().unwrap_or(&s.config.name),
            color: &b.primary_color,
            logo: b.logo.as_deref(),
            font_url: b.font_url.as_deref(),
        }
    }
}

/// Whole document. `csrf` goes into `hx-headers` on `<body>`.
fn document(
    brand: &Brand<'_>,
    title: &str,
    class: &str,
    csrf: Option<&str>,
    inner: Markup,
) -> Markup {
    let hx_headers = csrf.map(|t| format!(r#"{{"X-CSRF-Token":"{t}"}}"#));
    html! {
        (DOCTYPE)
        html lang="th" {
            head {
                meta charset="utf-8";
                meta name="viewport" content="width=device-width, initial-scale=1, viewport-fit=cover";
                meta name="color-scheme" content="light";
                title { (title) " · " (brand.name) }
                @if let Some(f) = brand.font_url { link rel="stylesheet" href=(f); }
                link rel="stylesheet" href=(CSS_HREF);
                // `primary_color` is checked to be a CSS hex colour at load.
                style { (PreEscaped(format!(":root{{--brand:{}}}", brand.color))) }
                script src=(HTMX_URL) integrity=(HTMX_SRI) crossorigin="anonymous"
                    referrerpolicy="no-referrer" defer {}
            }
            body class=(class) hx-headers=[hx_headers] { (inner) }
        }
    }
}

fn brand_link(brand: &Brand<'_>, href: &str) -> Markup {
    html! {
        a class="brand" href=(href) {
            @if let Some(logo) = brand.logo { img class="logo" src=(logo) alt=""; }
            span { (brand.name) }
        }
    }
}

/// A back-office page for the signed-in Staff member.
pub fn back_office(s: &PageState, staff: &CurrentStaff, title: &str, body: Markup) -> Markup {
    let brand = Brand::of(s);
    let t = |k: &str| s.catalog.t(&staff.locale, k);
    let nav = !staff.must_change_password && !staff.nav().is_empty();
    let inner = html! {
        header class="bar" {
            (brand_link(&brand, "/admin"))
            form method="post" action="/admin/logout" class="who" {
                (staff.csrf_field())
                span { (staff.email) }
                button class="link" type="submit" { (t("kernel.auth.sign_out")) }
            }
            @if nav {
                nav class="nav" aria-label=(t("kernel.backoffice.menu")) {
                    @for (label, href) in staff.nav() { a href=(href) { (label) } }
                }
            }
        }
        // Filled by the usage warnings (#26); empty until then.
        div id="usage-banner" {}
        main {
            @if let Some((ok, text)) = &staff.flash { (flash(&Flash { ok: *ok, text })) }
            (body)
        }
    };
    document(&brand, title, "office", Some(staff.csrf_token()), inner)
}

/// A page with the brand bar and no navigation (sign-in, errors).
pub fn bare(s: &PageState, title: &str, body: Markup) -> Markup {
    let brand = Brand::of(s);
    let inner = html! {
        header class="bar" { (brand_link(&brand, "/admin")) }
        main { (body) }
    };
    document(&brand, title, "office", None, inner)
}

/// A Mini App page with the Deployment's branding.
pub fn mini_app(s: &PageState, title: &str, body: Markup) -> Markup {
    let brand = Brand::of(s);
    document(&brand, title, "mini", None, mini_inner(brand.name, body))
}

fn mini_inner(org: &str, body: Markup) -> Markup {
    html! {
        main { (body) }
        footer class="foot" { (org) " · " a href="/notice" { "ประกาศความเป็นส่วนตัว" } }
    }
}

/// The whole document with the default theme. `org` is the Deployment
/// name. Prefer [`mini_app`] and [`bare`], which apply `[branding]`.
pub fn page(org: &str, title: &str, shell: Shell, body: Markup) -> Markup {
    let brand = Brand {
        name: org,
        color: crate::config::DEFAULT_PRIMARY,
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

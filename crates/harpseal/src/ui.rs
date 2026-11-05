//! Shared page layout for back-office and Mini App pages (issue #5
//! prototype): one maud layout, one CSS file served at [`CSS_PATH`], HTMX
//! from a pinned CDN URL. No JS framework, no build step.

use maud::{DOCTYPE, Markup, html};

/// The stylesheet, served by `Deployment::router` at [`CSS_PATH`].
pub const CSS: &str = include_str!("ui.css");
/// Bump the query string when `ui.css` changes (it is cached for a day).
pub const CSS_PATH: &str = "/static/app.css";
const CSS_HREF: &str = "/static/app.css?v=1";
/// Pinned HTMX build.
pub const HTMX_URL: &str = "https://cdn.jsdelivr.net/npm/htmx.org@2.0.4/dist/htmx.min.js";

/// Which chrome a page gets.
pub enum Shell<'a> {
    /// Back office: brand bar, navigation, signed-in Staff email.
    BackOffice { staff_email: Option<&'a str> },
    /// Inside LINE LIFF / Telegram Web App: no navigation, the host app
    /// already draws a title bar.
    MiniApp,
}

/// A short message shown once at the top of the main column.
pub struct Flash<'a> {
    pub ok: bool,
    pub text: &'a str,
}

pub fn flash(f: &Flash<'_>) -> Markup {
    html! {
        div class=(if f.ok { "flash ok" } else { "flash err" }) role="status" { (f.text) }
    }
}

/// The whole document. `org` is the Deployment name shown in the bar.
pub fn page(org: &str, title: &str, shell: Shell<'_>, body: Markup) -> Markup {
    let mini = matches!(shell, Shell::MiniApp);
    html! {
        (DOCTYPE)
        html lang="th" {
            head {
                meta charset="utf-8";
                meta name="viewport" content="width=device-width, initial-scale=1, viewport-fit=cover";
                meta name="color-scheme" content="light";
                title { (title) " · " (org) }
                link rel="stylesheet" href=(CSS_HREF);
                script src=(HTMX_URL) defer {}
            }
            body class=(if mini { "mini" } else { "office" }) {
                @if let Shell::BackOffice { staff_email } = shell {
                    header class="bar" {
                        a class="brand" href="/admin/students" { (org) }
                        @if let Some(email) = staff_email {
                            nav {
                                a href="/admin/students" { "นักศึกษา" }
                                a href="/admin/students#announce" { "ส่งประกาศ" }
                            }
                            form method="post" action="/admin/logout" class="who" {
                                span { (email) }
                                button class="link" type="submit" { "ออกจากระบบ" }
                            }
                        }
                    }
                }
                main { (body) }
                @if mini {
                    footer class="foot" { (org) " · " a href="/notice" { "ประกาศความเป็นส่วนตัว" } }
                }
            }
        }
    }
}

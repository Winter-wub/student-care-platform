//! The back-office shell (#123): the skip link, the letterhead bar, and
//! the navigation grouped as "งานประจำ" and "ผู้ดูแลระบบ" with the
//! current page marked `aria-current="page"`.
//!
//! The navigation is one `<details class="menu">`. Below 640 px it is a
//! "เมนู" button that opens the groups, with no JS. From 640 px it is a
//! side column: where the browser can style `::details-content`, the
//! column is always open and the button hidden; elsewhere the button
//! stays and still works.

use maud::{Markup, html};

use crate::auth::NavLink;
use crate::{CurrentStaff, NavGroup};

/// The `id` of `<main>`, the skip link's target.
pub(crate) const MAIN_ID: &str = "main";
/// The home page, listed above the groups.
const HOME: &str = "/admin";

/// The nav entry that owns `path`: the longest `href` that is `path` or
/// a parent of it (`/admin/roster` for `/admin/roster/123`). The home
/// owns only itself.
pub(crate) fn current<'a>(links: &'a [NavLink], path: &str) -> Option<&'a NavLink> {
    let owns = |href: &str| {
        path == href
            || (href != HOME && path.starts_with(href) && path[href.len()..].starts_with('/'))
    };
    links
        .iter()
        .filter(|l| owns(&l.href))
        .max_by_key(|l| l.href.len())
}

/// The skip link: the first focusable element of every back-office page.
pub(crate) fn skip_link(label: &str) -> Markup {
    html! { a class="skip" href=(format!("#{MAIN_ID}")) { (label) } }
}

/// Texts the navigation shows, in the Staff member's Locale.
pub(crate) struct NavTexts {
    pub(crate) menu: String,
    pub(crate) home: String,
    pub(crate) daily: String,
    pub(crate) admin: String,
}

/// The grouped navigation of `staff` on the page at `staff.path`.
pub(crate) fn nav(staff: &CurrentStaff, t: &NavTexts) -> Markup {
    let links = staff.nav();
    let on_home = staff.path == HOME;
    let here = current(links, &staff.path);
    let label = here.map_or(if on_home { t.home.as_str() } else { "" }, |l| {
        l.label.as_str()
    });
    let group = |g: NavGroup, id: &str, name: &str| {
        let items: Vec<&NavLink> = links.iter().filter(|l| l.group == g).collect();
        html! {
            @if !items.is_empty() {
                p class="nav-group" id=(id) { (name) }
                ul aria-labelledby=(id) {
                    @for l in items {
                        @let is_here = here.is_some_and(|h| h.href == l.href);
                        li { a href=(l.href) aria-current=[is_here.then_some("page")] { (l.label) } }
                    }
                }
            }
        }
    };
    html! {
        details class="menu" {
            summary {
                svg class="icon" viewBox="0 0 20 20" aria-hidden="true" focusable="false" {
                    path d="M3 5h14M3 10h14M3 15h14" stroke="currentColor" stroke-width="2"
                        stroke-linecap="round" fill="none" {}
                }
                span class="menu-word" { (t.menu) }
                @if !label.is_empty() { span class="menu-here" { (label) } }
            }
            nav class="nav" aria-label=(t.menu) {
                ul {
                    li { a href=(HOME) aria-current=[on_home.then_some("page")] { (t.home) } }
                }
                (group(NavGroup::Daily, "nav-daily", &t.daily))
                (group(NavGroup::Admin, "nav-admin", &t.admin))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn link(href: &str) -> NavLink {
        NavLink {
            label: href.into(),
            href: href.into(),
            group: NavGroup::Daily,
        }
    }

    #[test]
    fn the_longest_parent_owns_the_path() {
        let links = [
            link("/admin/roster"),
            link("/admin/roles"),
            link("/admin/r"),
        ];
        let at = |p: &str| current(&links, p).map(|l| l.href.as_str());
        assert_eq!(at("/admin/roster"), Some("/admin/roster"));
        assert_eq!(at("/admin/roster/0193/confirm"), Some("/admin/roster"));
        assert_eq!(at("/admin/rosters"), None, "a segment, not a prefix");
        assert_eq!(at("/admin/roles/7"), Some("/admin/roles"));
        assert_eq!(at("/admin"), None);
        let home = [link("/admin")];
        assert_eq!(current(&home, "/admin/x").map(|l| l.href.as_str()), None);
    }
}

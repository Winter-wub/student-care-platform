//! The back-office shell (#123): the skip link, the letterhead bar, and
//! the navigation grouped as "งานประจำ" and "ผู้ดูแลระบบ" with the
//! current page marked `aria-current="page"`.
//!
//! The navigation is one `<details class="menu">`. Below 640 px it is a
//! "เมนู" button that opens the groups, with no JS. From 640 px it is a
//! side column: where the browser can style `::details-content`, the
//! column is always open and the button hidden; elsewhere the button
//! stays and still works.
//!
//! Every entry starts with its [`Icon`]. Each group is a `<details>`
//! that folds; from 640 px a "ย่อเมนู" toggle folds the side column to a
//! rail of icons, each name shown beside its icon on hover and focus.
//! [`NAV_JS`] remembers both in `localStorage` and shows the toggle, so
//! without JS the column stays as it was.

use maud::{Markup, PreEscaped, html};

use super::icons::{self, Icon};
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
    pub(crate) rail: String,
}

/// One entry: its icon, then its name. The name stays in the link's
/// text when the column is an icon rail (drawn as a label on hover and
/// focus), so a screen reader always reads it.
fn entry(href: &str, label: &str, icon: Icon, is_here: bool) -> Markup {
    html! {
        li {
            a href=(href) aria-current=[is_here.then_some("page")] {
                (icon.svg())
                span class="nav-label" { (label) }
            }
        }
    }
}

/// The grouped navigation of `staff` on the page at `staff.path`.
pub(crate) fn nav(staff: &CurrentStaff, t: &NavTexts) -> Markup {
    let links = staff.nav();
    let on_home = staff.path == HOME;
    let here = current(links, &staff.path);
    let label = here.map_or(if on_home { t.home.as_str() } else { "" }, |l| {
        l.label.as_str()
    });
    // A group is a <details>: open by default and always open while it
    // holds the current page (the script keeps a closed one closed).
    let group = |g: NavGroup, key: &str, name: &str| {
        let items: Vec<&NavLink> = links.iter().filter(|l| l.group == g).collect();
        let id = format!("nav-{key}");
        html! {
            @if !items.is_empty() {
                details class="nav-sec" data-sec=(key) open {
                    summary class="nav-group" id=(id) {
                        span class="nav-label" { (name) }
                        (icons::chevron())
                    }
                    ul aria-labelledby=(id) {
                        @for l in items {
                            @let is_here = here.is_some_and(|h| h.href == l.href);
                            (entry(&l.href, &l.label, l.icon, is_here))
                        }
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
            nav class="nav" id="nav" aria-label=(t.menu) {
                // Drawn by the script (no JS, no rail): from 640 px only.
                button type="button" class="link nav-rail-toggle" aria-pressed="false"
                    aria-controls="nav" hidden {
                    (icons::panel())
                    span class="nav-label" { (t.rail) }
                }
                ul {
                    (entry(HOME, &t.home, Icon::Home, on_home))
                }
                (group(NavGroup::Daily, "daily", &t.daily))
                (group(NavGroup::Admin, "admin", &t.admin))
            }
        }
        script { (PreEscaped(NAV_JS)) }
    }
}

/// Remembers the side column (an icon rail or not) and each closed group
/// in `localStorage` (`harpseal.nav`), and applies them as the page is
/// read, before `<main>`, so nothing jumps. Without storage (private
/// mode) the toggles still work for the page. The rail is a class on
/// `<html>`, so CSS draws it only from 640 px.
pub(crate) const NAV_JS: &str = r#"(function(){
var K='harpseal.nav',d=document.documentElement,S={};
try{S=JSON.parse(localStorage.getItem(K))||{};}catch(e){}
function save(){try{localStorage.setItem(K,JSON.stringify(S));}catch(e){}}
var b=document.querySelector('.nav-rail-toggle');
function rail(on){d.classList.toggle('nav-rail',on);b.setAttribute('aria-pressed',on?'true':'false');}
if(b){b.hidden=false;rail(!!S.rail);
b.addEventListener('click',function(){S.rail=!d.classList.contains('nav-rail');rail(S.rail);save();});}
document.querySelectorAll('.nav-sec').forEach(function(x){var k='sec:'+x.getAttribute('data-sec');
if(S[k]===0&&!x.querySelector('[aria-current]'))x.open=false;
x.addEventListener('toggle',function(){if(d.classList.contains('nav-rail'))return;S[k]=x.open?1:0;save();});});
})();"#;

#[cfg(test)]
mod tests {
    use super::*;

    fn link(href: &str) -> NavLink {
        NavLink {
            label: href.into(),
            href: href.into(),
            group: NavGroup::Daily,
            icon: Icon::Page,
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

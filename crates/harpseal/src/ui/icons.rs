//! The navigation icons: one small set drawn for the back office, inline
//! SVG on a 24 px grid, 1.75 px round strokes in `currentColor`, no fill.
//! Nothing is fetched at run time and each icon is a few path commands,
//! so the whole set adds about 2 KB to the Worker.
//!
//! A Module picks an icon for its page with
//! [`crate::ModuleRegistrar::nav_icon`]; a page with none gets
//! [`NavIcon::Page`]. Icons are named by what they show, not by a page,
//! so a Module can reuse them.

use maud::{Markup, PreEscaped, html};

/// An icon before a navigation entry.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum NavIcon {
    /// A sheet with a folded corner: any page (the default).
    #[default]
    Page,
    /// A house: the home.
    Home,
    /// Two people: Students, a list of people.
    People,
    /// One person: Staff, an account.
    Person,
    /// An arrow up out of a tray: an import.
    Upload,
    /// A megaphone: announcements.
    Megaphone,
    /// A question mark in a circle: questions, help.
    Question,
    /// A wallet: money, fees.
    Wallet,
    /// A list of rows: a list of records.
    List,
    /// A bell: reminders, notifications.
    Bell,
    /// A clipboard with a tick: results, grades, checks.
    Clipboard,
    /// A building with columns: an institution, faculties.
    Building,
    /// A shield: security.
    Shield,
    /// A clock turning back: a history, the audit log.
    History,
    /// A key: Roles and Permissions.
    Key,
    /// A speech bubble with lines: texts, messages.
    Message,
    /// A gauge: usage, quotas.
    Gauge,
    /// A palette: branding.
    Palette,
    /// Three sliders: settings.
    Sliders,
    /// A box: backups, archives.
    Archive,
    /// A calendar: dates, terms, schedules.
    Calendar,
}

impl NavIcon {
    /// The inner SVG of the icon (paths and circles), trusted constants.
    fn body(self) -> &'static str {
        match self {
            Self::Page => {
                r#"<path d="M14 3H6.5A2.5 2.5 0 0 0 4 5.5v13A2.5 2.5 0 0 0 6.5 21h11a2.5 2.5 0 0 0 2.5-2.5V9z"/><path d="M14 3v6h6M8.5 13h7M8.5 17h5"/>"#
            }
            Self::Home => {
                r#"<path d="M3.5 10.5 12 3.5l8.5 7"/><path d="M5.5 9v11h13V9"/><path d="M10 20v-5.5h4V20"/>"#
            }
            Self::People => {
                r#"<circle cx="9" cy="8" r="3.5"/><path d="M2.5 20c0-3.6 2.9-6 6.5-6s6.5 2.4 6.5 6M16 4.7a3.5 3.5 0 0 1 0 6.6M18 14.4c2.1.7 3.5 2.6 3.5 5.6"/>"#
            }
            Self::Person => {
                r#"<circle cx="12" cy="8" r="4"/><path d="M4 21c0-4.1 3.6-7 8-7s8 2.9 8 7"/>"#
            }
            Self::Upload => {
                r#"<path d="M12 15V4M7.5 8.5 12 4l4.5 4.5"/><path d="M4 14.5v3.5A2.5 2.5 0 0 0 6.5 20.5h11a2.5 2.5 0 0 0 2.5-2.5v-3.5"/>"#
            }
            Self::Megaphone => {
                r#"<path d="M3.5 10v4a1 1 0 0 0 1 1h2.5l8 5V4l-8 5H4.5a1 1 0 0 0-1 1z"/><path d="M18.5 9a4 4 0 0 1 0 6"/>"#
            }
            Self::Question => {
                r#"<circle cx="12" cy="12" r="9"/><path d="M9.5 9.3a2.6 2.6 0 0 1 5 .9c0 1.8-2.5 2.2-2.5 4M12 17.5h.01"/>"#
            }
            Self::Wallet => {
                r#"<path d="M17.5 7.5V5H6a2.5 2.5 0 0 0 0 5"/><path d="M3.5 7.5v10A2.5 2.5 0 0 0 6 20h14.5v-10H6"/><path d="M16.5 15h.01"/>"#
            }
            Self::List => r#"<path d="M9.5 6h11M9.5 12h11M9.5 18h11M4 6h.01M4 12h.01M4 18h.01"/>"#,
            Self::Bell => {
                r#"<path d="M6 9a6 6 0 0 1 12 0c0 6.5 2.5 8.5 2.5 8.5h-17S6 15.5 6 9"/><path d="M10.2 20.5a2 2 0 0 0 3.6 0"/>"#
            }
            Self::Clipboard => {
                r#"<path d="M9 3.5h6v3H9z"/><path d="M15 5h2a2 2 0 0 1 2 2v12a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V7a2 2 0 0 1 2-2h2"/><path d="m9 14 2 2 4-4"/>"#
            }
            Self::Building => {
                r#"<path d="M3 20.5h18M4.5 9.5h15M12 3.5l-8 4.5v1.5h16V8z"/><path d="M6.5 9.5v8M10 9.5v8M14 9.5v8M17.5 9.5v8M4.5 17.5h15"/>"#
            }
            Self::Shield => {
                r#"<path d="M12 3 19.5 6v5.5c0 4.6-3.2 8.3-7.5 9.5-4.3-1.2-7.5-4.9-7.5-9.5V6z"/><path d="M12 8v4.5M12 16h.01"/>"#
            }
            Self::History => {
                r#"<path d="M3.5 12A8.5 8.5 0 1 0 6 6L3.5 8.5"/><path d="M3.5 3.5v5h5M12 7.5V12l3 2"/>"#
            }
            Self::Key => {
                r#"<circle cx="8" cy="15.5" r="4.5"/><path d="M11.2 12.3 20 3.5M16.5 7l2.5 2.5M14 9.5l2 2"/>"#
            }
            Self::Message => {
                r#"<path d="M20.5 15a2 2 0 0 1-2 2H8l-4.5 3.5V5.5a2 2 0 0 1 2-2h13a2 2 0 0 1 2 2z"/><path d="M7.5 8.5h9M7.5 12.5h5.5"/>"#
            }
            Self::Gauge => r#"<path d="M5 19a9 9 0 1 1 14 0"/><path d="m12 14 4-4.5M12 14h.01"/>"#,
            Self::Palette => {
                r#"<path d="M12 3a9 9 0 0 0 0 18c1.1 0 1.9-.8 1.9-1.9 0-.5-.2-.9-.5-1.3-.3-.3-.5-.8-.5-1.3 0-1 .9-1.9 1.9-1.9h2.3A4.4 4.4 0 0 0 21.5 11C21.5 6.6 17.2 3 12 3z"/><path d="M7.5 12h.01M9.5 7.5h.01M14.5 7.5h.01"/>"#
            }
            Self::Sliders => {
                r#"<path d="M4 6.5h9M17 6.5h3M4 12h3M11 12h9M4 17.5h11M19 17.5h1"/><circle cx="15" cy="6.5" r="2"/><circle cx="9" cy="12" r="2"/><circle cx="17" cy="17.5" r="2"/>"#
            }
            Self::Archive => {
                r#"<path d="M3.5 4.5h17v4h-17z"/><path d="M5 8.5v10a1.5 1.5 0 0 0 1.5 1.5h11a1.5 1.5 0 0 0 1.5-1.5v-10M10 12.5h4"/>"#
            }
            Self::Calendar => {
                r#"<path d="M5.5 5h13A2 2 0 0 1 20.5 7v11.5a2 2 0 0 1-2 2h-13a2 2 0 0 1-2-2V7a2 2 0 0 1 2-2z"/><path d="M8 3v4M16 3v4M3.5 10h17"/>"#
            }
        }
    }

    /// The icon as an inline `<svg>`, hidden from assistive technology
    /// (the entry's text names it).
    pub(crate) fn svg(self) -> Markup {
        svg(self.body())
    }
}

/// An inline 20 px icon around `body` (trusted constants only).
fn svg(body: &str) -> Markup {
    html! {
        svg class="icon" viewBox="0 0 24 24" width="20" height="20" aria-hidden="true"
            focusable="false" fill="none" stroke="currentColor" stroke-width="1.75"
            stroke-linecap="round" stroke-linejoin="round" { (PreEscaped(body)) }
    }
}

/// The chevron on a navigation group's heading.
pub(crate) fn chevron() -> Markup {
    svg(r#"<path d="m6 9 6 6 6-6"/>"#)
}

/// The side column's "ย่อเมนู" button: a panel with its left column.
pub(crate) fn panel() -> Markup {
    svg(
        r#"<path d="M5.5 4h13a2 2 0 0 1 2 2v12a2 2 0 0 1-2 2h-13a2 2 0 0 1-2-2V6a2 2 0 0 1 2-2zM9.5 4v16"/><path d="m16 10-2 2 2 2"/>"#,
    )
}

#[cfg(test)]
mod tests {
    use super::NavIcon;

    #[test]
    fn every_icon_is_a_small_stroked_svg_hidden_from_readers() {
        let all = [
            NavIcon::Page,
            NavIcon::Home,
            NavIcon::People,
            NavIcon::Person,
            NavIcon::Upload,
            NavIcon::Megaphone,
            NavIcon::Question,
            NavIcon::Wallet,
            NavIcon::List,
            NavIcon::Bell,
            NavIcon::Clipboard,
            NavIcon::Building,
            NavIcon::Shield,
            NavIcon::History,
            NavIcon::Key,
            NavIcon::Message,
            NavIcon::Gauge,
            NavIcon::Palette,
            NavIcon::Sliders,
            NavIcon::Archive,
            NavIcon::Calendar,
        ];
        let mut total = 0;
        for i in all {
            let svg = i.svg().into_string();
            assert!(svg.contains(r#"aria-hidden="true""#), "{i:?}");
            assert!(svg.contains(r#"stroke="currentColor""#), "{i:?}");
            assert!(!svg.contains("fill=\"#"), "{i:?}: no fixed colour");
            total += i.body().len();
        }
        assert!(total < 3_000, "the set stays small: {total} bytes");
    }
}

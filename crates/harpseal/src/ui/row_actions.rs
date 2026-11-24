//! Row actions: the buttons at the end of a table row or beside a card's
//! heading ("เลื่อนขึ้น เลื่อนลง ซ่อน แก้ไข ลบ"), drawn the same way on
//! every page.
//!
//! ```text
//! [↑|↓]  [👁 ซ่อน]  [✎ แก้ไข]  │ [🗑]
//!  pair   labelled   labelled    danger, set apart
//! ```
//!
//! - [`row_actions`] holds them: one row, right-aligned, even 8 px gaps,
//!   never wrapping where the table is a table; stacked rows (phones) give
//!   it the cell's whole width and let it wrap.
//! - Each action is a *tool*: a 48 px Paper button with a Field Edge
//!   border and an [`Icon`]. [`ToolLook::Labelled`] shows the icon and
//!   its name; [`ToolLook::IconOnly`] shows the icon with the name as its
//!   `aria-label` and as a label on hover and keyboard focus;
//!   [`ToolLook::Danger`] is an icon-only delete in Refused Ink, set
//!   apart by a Rule Line. A delete still goes through its confirmation
//!   page: only the look changes.
//! - [`tool_pair`] joins two tools (move up, move down) into one control;
//!   [`tool_off`] keeps a place for an action that does not apply (the
//!   first row cannot move up), so every row lines up.
//!
//! The busy state (`ui::busy`) applies to a tool's form like any other.

use maud::{Markup, html};

use super::Icon;
use crate::CurrentStaff;

/// How a tool is drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum ToolLook {
    /// The icon and the name: for actions whose icon alone may not say
    /// enough (ซ่อน / แสดง, แก้ไข).
    Labelled,
    /// The icon only; the name is its `aria-label` and its hover/focus
    /// label. For actions an icon says plainly (move up, move down).
    IconOnly,
    /// The destructive action (ลบ): icon only, Refused Ink, set apart.
    Danger,
}

impl ToolLook {
    fn class(self) -> &'static str {
        match self {
            Self::Labelled => "tool",
            Self::IconOnly => "tool icon-only",
            Self::Danger => "tool icon-only tool-danger",
        }
    }
}

/// The tool's inside: the icon, and the name when it is shown.
fn inside(icon: Icon, label: &str, look: ToolLook) -> Markup {
    html! {
        (icon.svg())
        @if look == ToolLook::Labelled { span class="tool-label" { (label) } }
    }
}

/// The row of actions, in the order given (put the danger tool last).
pub fn row_actions(inner: Markup) -> Markup {
    html! { div class="row-actions" { (inner) } }
}

/// A tool that posts `fields` (and the CSRF field) to `action`.
pub fn tool_post(
    staff: &CurrentStaff,
    action: &str,
    fields: &[(&str, &str)],
    icon: Icon,
    label: &str,
    look: ToolLook,
) -> Markup {
    let labelled = look == ToolLook::Labelled;
    let danger = look == ToolLook::Danger;
    html! {
        form method="post" action=(action) class=(if danger { "tool-form row-danger" } else { "tool-form" }) {
            (staff.csrf_field())
            @for (name, value) in fields { input type="hidden" name=(*name) value=(*value); }
            button type="submit" class=(look.class())
                aria-label=[(!labelled).then_some(label)] data-tip=[(!labelled).then_some(label)] {
                (inside(icon, label, look))
            }
        }
    }
}

/// A tool that opens `href` (แก้ไข).
pub fn tool_link(href: &str, icon: Icon, label: &str, look: ToolLook) -> Markup {
    let labelled = look == ToolLook::Labelled;
    html! {
        a href=(href) class=(look.class())
            aria-label=[(!labelled).then_some(label)] data-tip=[(!labelled).then_some(label)] {
            (inside(icon, label, look))
        }
    }
}

/// An icon-only tool that does not apply on this row, kept so the rows
/// line up: disabled, named, never focusable.
pub fn tool_off(icon: Icon, label: &str) -> Markup {
    html! {
        button type="button" class="tool icon-only" disabled aria-label=(label) { (icon.svg()) }
    }
}

/// Two tools joined into one control (move up, move down), named
/// `label` for assistive technology (`role="group"`).
pub fn tool_pair(label: &str, first: Markup, second: Markup) -> Markup {
    html! { span class="tool-pair" role="group" aria-label=(label) { (first) (second) } }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_icon_only_tool_is_named_and_a_labelled_one_shows_its_name() {
        let edit = tool_link("/x/edit", Icon::Pencil, "แก้ไข", ToolLook::Labelled).into_string();
        assert!(edit.contains("<span class=\"tool-label\">แก้ไข</span>"));
        assert!(!edit.contains("aria-label"), "the text names it");
        let up = tool_link("/x", Icon::ArrowUp, "เลื่อนขึ้น", ToolLook::IconOnly).into_string();
        assert!(up.contains("aria-label=\"เลื่อนขึ้น\" data-tip=\"เลื่อนขึ้น\""));
        assert!(!up.contains("tool-label"));
        let off = tool_off(Icon::ArrowUp, "เลื่อนขึ้น").into_string();
        assert!(off.contains("disabled") && off.contains("aria-label=\"เลื่อนขึ้น\""));
        let pair =
            tool_pair("ลำดับ", maud::PreEscaped(off.clone()), maud::PreEscaped(off)).into_string();
        assert!(pair.starts_with("<span class=\"tool-pair\" role=\"group\" aria-label=\"ลำดับ\">"));
    }
}

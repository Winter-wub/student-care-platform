//! The audit log in Staff's words (#125): an action as a Thai sentence
//! ("สร้างประกาศ"), an actor and a target by what they are ("ระบบ",
//! "ประกาศ"), the raw codes and ids in the technical details.
//!
//! An action's label is the text `kernel.admin.audit.action.<action>` (dots
//! as `_`) for the Kernel's actions, else `<module>.audit.<rest>` a Module
//! declares for its own (`fees.import` → `fees.audit.import`), else the
//! action itself.

use maud::{Markup, html};

use super::audit::Entry;
use super::view::View;

/// What Staff read for `action`.
pub(super) fn action(v: &View<'_>, action: &str) -> String {
    let kernel = format!("kernel.admin.audit.action.{}", action.replace('.', "_"));
    let module = action
        .split_once('.')
        .map(|(m, rest)| format!("{m}.audit.{rest}"));
    [Some(kernel), module]
        .into_iter()
        .flatten()
        .find(|k| v.s.catalog.knows(k))
        .map_or_else(|| action.to_string(), |k| v.t(&k))
}

/// A kind of actor or target in Staff's words, else the kind itself.
fn kind(v: &View<'_>, kind: &str) -> String {
    let key = format!("kernel.admin.audit.kind.{kind}");
    if v.s.catalog.knows(&key) {
        v.t(&key)
    } else {
        kind.to_string()
    }
}

/// The actor: the Staff member's name, else what acted ("ระบบ").
pub(super) fn actor(v: &View<'_>, e: &Entry) -> Markup {
    html! {
        @match (&e.actor_name, &e.actor_id) {
            (Some(name), Some(id)) => a href=(format!("/admin/staff/{id}")) { (name) },
            _ => span { (kind(v, &e.actor_kind)) },
        }
    }
}

/// The target by what it is, linked to its page when it has one.
pub(super) fn target(v: &View<'_>, e: &Entry) -> Markup {
    let k = e.target_kind.as_deref().unwrap_or_default();
    let id = e.target_id.as_deref().unwrap_or_default();
    let href = match k {
        "student" => Some(format!("/admin/students/{id}")),
        "staff" => Some(format!("/admin/staff/{id}")),
        "announcements" | "announcement" => Some(format!("/admin/announcements/{id}")),
        "roster_import" => Some(format!("/admin/roster/{id}")),
        "role" => Some(format!("/admin/roles/{id}")),
        _ => None,
    };
    html! {
        @if k.is_empty() { span class="muted" { "–" } }
        @else if let Some(h) = href { a href=(h) { (kind(v, k)) } }
        @else { span { (kind(v, k)) } }
    }
}

/// The raw actor and target, for the technical details.
pub(super) fn raw(e: &Entry) -> String {
    let actor = match &e.actor_id {
        Some(id) => format!("{}:{id}", e.actor_kind),
        None => e.actor_kind.clone(),
    };
    match (&e.target_kind, &e.target_id) {
        (Some(k), Some(id)) => format!("{} · {actor} → {k}:{id}", e.action),
        (Some(k), None) => format!("{} · {actor} → {k}", e.action),
        _ => format!("{} · {actor}", e.action),
    }
}

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
    // A Role target written before #125 by the CLI's `role.create` holds
    // the Role's name, which no page opens: show it, without a link.
    let href = match k {
        "student" => Some("/admin/students"),
        "staff" => Some("/admin/staff"),
        "announcements" | "announcement" => Some("/admin/announcements"),
        "roster_import" => Some("/admin/roster"),
        "role" => Some("/admin/roles"),
        _ => None,
    }
    .filter(|_| k != "role" || is_id(id))
    .map(|base| format!("{base}/{id}"));
    html! {
        @if k.is_empty() { span class="muted" { "–" } }
        @else if let Some(h) = href { a href=(h) { (kind(v, k)) } }
        @else if k == "role" && !id.is_empty() { span { (kind(v, k)) " " (v.role(id)) } }
        @else { span { (kind(v, k)) } }
    }
}

/// Whether `s` has the shape of an id the Core generates
/// (`01a1bd91-3822-7a0b-9f74-3a1024ae8f36`).
fn is_id(s: &str) -> bool {
    s.len() == 36
        && s.char_indices().all(|(i, c)| match i {
            8 | 13 | 18 | 23 => c == '-',
            _ => c.is_ascii_hexdigit(),
        })
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

#[cfg(test)]
mod tests {
    use std::path::Path;

    #[test]
    fn only_generated_ids_are_linked() {
        assert!(super::is_id("01a1bd91-3822-7a0b-9f74-3a1024ae8f36"));
        assert!(!super::is_id("Administrator"));
        assert!(!super::is_id("01a1bd91-3822-7a0b-9f74-3a1024ae8f3"));
        assert!(!super::is_id("01a1bd91x3822-7a0b-9f74-3a1024ae8f36"));
    }

    /// The first word of every audit action the Core writes.
    const FAMILIES: [&str; 15] = [
        "announcement",
        "audit",
        "backup",
        "branding",
        "consent",
        "data",
        "link",
        "module",
        "role",
        "roster",
        "security",
        "setting",
        "staff",
        "student",
        "usage",
    ];

    /// String literals of those families that are not audit actions.
    const NOT_ACTIONS: [&str; 4] = [
        "link.revoke.no",
        "link.revoke.yes",
        "roster.csv",
        "roster.xlsx",
    ];

    /// Every `"family.word…"` string literal in the Core's sources.
    fn literals(dir: &Path, out: &mut Vec<String>) {
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                literals(&path, out);
            } else if path.extension().is_some_and(|e| e == "rs") {
                let src = std::fs::read_to_string(&path).unwrap();
                for lit in src.split('"').skip(1).step_by(2) {
                    let mut parts = lit.split('.');
                    let first = parts.next().unwrap_or_default();
                    let rest: Vec<&str> = parts.collect();
                    let word = |w: &str| {
                        !w.is_empty() && w.bytes().all(|b| b.is_ascii_lowercase() || b == b'_')
                    };
                    if FAMILIES.contains(&first) && !rest.is_empty() && rest.iter().all(|w| word(w))
                    {
                        out.push(lit.to_string());
                    }
                }
            }
        }
    }

    /// Each audit action the Core writes has a Thai label (#125): the
    /// audit log never shows a raw code for a Kernel action.
    #[test]
    fn every_core_audit_action_has_a_label() {
        let mut found = Vec::new();
        literals(
            &Path::new(env!("CARGO_MANIFEST_DIR")).join("src"),
            &mut found,
        );
        found.sort();
        found.dedup();
        assert!(
            found.iter().any(|a| a == "module.upload.confirm"),
            "{found:?}"
        );
        let labelled: Vec<&str> = super::super::texts_audit::AUDIT
            .iter()
            .map(|e| e.key)
            .collect();
        let missing: Vec<&String> = found
            .iter()
            .filter(|a| !NOT_ACTIONS.contains(&a.as_str()))
            .filter(|a| {
                let key = format!("kernel.admin.audit.action.{}", a.replace('.', "_"));
                !labelled.contains(&key.as_str())
            })
            .collect();
        assert!(
            missing.is_empty(),
            "audit actions without a label: {missing:?}"
        );
    }
}

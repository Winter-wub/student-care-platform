//! The per-Student lists of a preview (#134): who changes and how
//! (collapsed, old → new per field, labelled with the `[roster]` labels),
//! who will become left with the chat Links each loses, and
//! `GET /admin/roster/{id}/changes.csv`, every change as CSV (a sensitive
//! read like the original file: audit `roster.changes.download`). The
//! page reads the stored head (the first of each list); the CSV reads the
//! whole stored list (`diff.rs`), and answers 410 once it is gone.

use axum::extract::{Path, State};
use axum::response::Response;
use maud::{Markup, html};
use serde_json::Value as Json;

use super::diff::{self, Change, FieldChange, Head};
use super::store::{self, Import};
use super::view::{BASE, View, csv_body, download};
use crate::audit::{self, Actor, RequestCtx, Target};
use crate::ui::number;
use crate::{AppState, CurrentStaff};

/// The field as Staff read it: the `[roster]` label of an attribute, the
/// Students page's label of the other fields.
fn label(v: &View<'_>, field: &str) -> String {
    if let Some(key) = field.strip_prefix("attributes.") {
        let default = &v.s.config.default_locale;
        // An attribute without a label reads as its column header, the
        // name Staff know it by from the file.
        return match v.s.config.roster.attributes.get(key) {
            Some(a) if a.label.is_empty() => a.column.clone(),
            Some(a) => a.label_in(key, &v.staff.locale, default).to_string(),
            None => key.to_string(),
        };
    }
    match field {
        "display_name" => v.t("kernel.students.name"),
        "tags" => v.t("kernel.students.tags"),
        other => v.t(&format!("kernel.students.{other}")),
    }
}

/// A stored value as Staff read it: a status in words, an enum value with
/// its label, Tags joined, nothing as "(ว่าง)".
fn value(v: &View<'_>, field: &str, x: &Json) -> String {
    let default = &v.s.config.default_locale;
    match x {
        Json::Null => v.t("kernel.roster.empty_value"),
        Json::Array(a) if a.is_empty() => v.t("kernel.roster.empty_value"),
        Json::Array(a) => a
            .iter()
            .map(|t| t.as_str().map_or_else(|| t.to_string(), str::to_string))
            .collect::<Vec<_>>()
            .join(", "),
        Json::String(s) if field == "status" => v.t(&format!("kernel.students.status_{s}")),
        Json::String(s) => match field
            .strip_prefix("attributes.")
            .and_then(|k| v.s.config.roster.attributes.get(k))
        {
            Some(a) if !a.value_labels.is_empty() => a.labelled(s, &v.staff.locale, default),
            _ => s.clone(),
        },
        other => other.to_string(),
    }
}

/// The kind as Staff read it.
fn kind(v: &View<'_>, kind: &str) -> String {
    v.t(match kind {
        "new" => "kernel.roster.new",
        "returning" => "kernel.roster.returning",
        "leaving" => "kernel.roster.will_become_left",
        _ => "kernel.roster.updated",
    })
}

fn delta(v: &View<'_>, f: &FieldChange) -> Markup {
    html! {
        li {
            span class="delta-field" { (label(v, &f.field)) } " "
            span class="delta-before" { (value(v, &f.field, &f.before)) }
            span class="delta-arrow" aria-hidden="true" { " → " }
            span class="visually-hidden" { " " (v.t("kernel.roster.csv_after")) " " }
            span class="delta-after" { (value(v, &f.field, &f.after)) }
        }
    }
}

fn shown_note(v: &View<'_>, shown: usize, total: u64) -> Markup {
    html! {
        @if (shown as u64) < total {
            p class="muted" {
                (v.t("kernel.roster.list_shown")
                    .replace("{shown}", &number(shown as u64))
                    .replace("{total}", &number(total)))
            }
        }
    }
}

fn csv_link(v: &View<'_>, imp: &Import) -> Markup {
    html! {
        p { a class="btn secondary" href=(format!("{BASE}/{}/changes.csv", imp.id)) {
            (v.t("kernel.roster.changes_csv"))
        } }
    }
}

/// Who changes and how, collapsed, from the stored `head`; nothing for
/// a preview stored before #134 (or whose head is gone).
pub(super) fn changes(v: &View<'_>, imp: &Import, head: Option<&Head>) -> Markup {
    let p = &imp.counts.preview;
    if !p.detailed {
        return html! {};
    }
    let total = p.changed();
    // The lists are not stored any more (a restored backup carries no
    // FileStore object but the file): say so, so a full replace never looks
    // as if no one leaves.
    let missing = head.is_none() && (total > 0 || p.will_become_left > 0);
    let missing = html! {
        @if missing { p class="muted" id="roster-lists-missing" { (v.t("kernel.roster.lists_missing")) } }
    };
    if total == 0 {
        // About the file's rows only, so it stands beside a leaving figure.
        return html! { p class="muted" { (v.t("kernel.roster.no_changes")) } (missing) };
    }
    let Some(head) = head else {
        return missing;
    };
    let (code, name, change) = (
        v.t("kernel.students.code"),
        v.t("kernel.students.name"),
        v.t("kernel.roster.change"),
    );
    html! {
        details class="changes" id="roster-changes" {
            summary { (v.t("kernel.roster.changes_open").replace("{n}", &number(total))) }
            table class="rows" {
                thead { tr { th { (code) } th { (name) } th { (change) } } }
                tbody {
                    @for c in &head.changes {
                        tr data-kind=(c.kind) {
                            td class="code id" data-label=(code) { (c.code) }
                            td data-label=(name) { (c.name) }
                            td data-label=(change) {
                                span class=(if c.kind == "new" { "chip ok" } else { "chip" }) { (kind(v, &c.kind)) }
                                @if !c.fields.is_empty() {
                                    ul class="deltas" { @for f in &c.fields { (delta(v, f)) } }
                                }
                            }
                        }
                    }
                }
            }
            (shown_note(v, head.changes.len(), total))
            (csv_link(v, imp))
        }
    }
}

/// Who will become left, with the Links each loses, from the stored
/// `head`; a preview stored before #134 keeps its list of names.
pub(super) fn leaving(v: &View<'_>, imp: &Import, head: Option<&Head>) -> Markup {
    let p = &imp.counts.preview;
    let Some(head) = head.filter(|h| !h.leaving.is_empty()) else {
        return html! {
            @if !p.left_names.is_empty() {
                div class="card" {
                    h2 { (v.t("kernel.roster.left_names")) }
                    ul class="names" { @for n in &p.left_names { li { (n) } } }
                    @if p.left_more > 0 { p class="muted" { (v.t("kernel.roster.more")) " +" (p.left_more) } }
                }
            }
        };
    };
    let (code, name, links) = (
        v.t("kernel.students.code"),
        v.t("kernel.students.name"),
        v.t("kernel.roster.links_col"),
    );
    html! {
        section class="card" aria-labelledby="roster-leaving-title" id="roster-leaving-list" {
            h2 id="roster-leaving-title" { (v.t("kernel.roster.left_names")) " " strong class="leaving-count" { (number(p.will_become_left)) } }
            table class="rows" {
                thead { tr { th { (code) } th { (name) } th class="num" { (links) } } }
                tbody {
                    @for c in &head.leaving {
                        tr {
                            td class="code id" data-label=(code) { (c.code) }
                            td data-label=(name) { (c.name) }
                            td class="num" data-label=(links) { (number(c.links)) }
                        }
                    }
                }
            }
            (shown_note(v, head.leaving.len(), p.will_become_left))
            (csv_link(v, imp))
        }
    }
}

/// Every change as CSV in the Staff member's Locale: one line per field
/// that changes, one per new or leaving Student.
pub(super) async fn csv(
    State(s): State<AppState>,
    staff: CurrentStaff,
    ctx: RequestCtx,
    Path(id): Path<String>,
) -> Response {
    let v = View::new(&s, &staff);
    let imp = match store::load(&s.services, &id).await {
        Ok(Some(i)) if i.counts.preview.detailed => i,
        Ok(_) => return v.not_found(),
        Err(e) => return v.error(&e),
    };
    // Deleted with the file after a year, or never stored (nothing
    // changes): gone, never a silent part of the list.
    let stored = if imp.file_key.is_empty() {
        Ok(None)
    } else {
        s.services.files.get(&diff::key(&imp.id)).await
    };
    let all = match stored {
        Ok(Some(b)) => diff::decode(&b),
        Ok(None) => return gone(&v),
        Err(e) => return v.error(&e),
    };
    let after = serde_json::json!({ "file_name": imp.file_name, "students": all.len() });
    let target = Target::new("roster_import", imp.id.as_str());
    let logged = audit::record(
        &s.services,
        &Actor::staff(&staff),
        "roster.changes.download",
        Some(&target),
        None,
        Some(after),
        &ctx,
    )
    .await;
    if let Err(e) = logged {
        return v.error(&e);
    }
    let name = format!("roster-{id}-changes.csv");
    download(
        "text/csv; charset=utf-8",
        &name,
        &name,
        csv_body(lines(&v, &all)),
    )
}

/// 410 in the layout: the list of changes is no longer kept.
fn gone(v: &View<'_>) -> Response {
    let text = v.t("kernel.roster.changes_gone");
    let body = html! {
        div class="card" {
            p { (text) }
            a class="btn" href=(BASE) { (v.t("kernel.roster.back")) }
        }
    };
    v.page(axum::http::StatusCode::GONE, &text, body)
}

fn lines(v: &View<'_>, all: &[Change]) -> Vec<String> {
    let head = [
        v.t("kernel.students.code"),
        v.t("kernel.students.name"),
        v.t("kernel.roster.change"),
        v.t("kernel.roster.csv_field"),
        v.t("kernel.roster.csv_before"),
        v.t("kernel.roster.csv_after"),
        v.t("kernel.roster.links_col"),
    ];
    let mut out = vec![super::csv::line(
        &head.iter().map(String::as_str).collect::<Vec<_>>(),
    )];
    for c in all {
        let k = kind(v, &c.kind);
        let links = if c.kind == "leaving" {
            c.links.to_string()
        } else {
            String::new()
        };
        if c.fields.is_empty() {
            out.push(super::csv::line(&[
                &c.code, &c.name, &k, "", "", "", &links,
            ]));
        }
        for f in &c.fields {
            let (l, b, a) = (
                label(v, &f.field),
                value(v, &f.field, &f.before),
                value(v, &f.field, &f.after),
            );
            out.push(super::csv::line(&[
                &c.code, &c.name, &k, &l, &b, &a, &links,
            ]));
        }
    }
    out
}

//! `GET /admin/audit/{id}` (`kernel.audit.view`): one audit row with its
//! before/after as a two-column diff of top-level keys (changed keys
//! highlighted), always through [`crate::audit::redact`]. Writes the read
//! row `audit.detail` (target = the row id); refused if it cannot.

use std::collections::BTreeSet;

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::Response;
use maud::{Markup, html};
use serde_json::{Map, Value as Json};

use super::audit::{COLUMNS, FROM, actor, entries, target};
use super::view::{View, shown};
use crate::audit::{self, Target};
use crate::port_types::{Statement, Value};
use crate::{AppState, CurrentStaff, RequestCtx};

pub(super) async fn page(
    State(s): State<AppState>,
    staff: CurrentStaff,
    ctx: RequestCtx,
    Path(id): Path<String>,
) -> Response {
    let v = View::new(&s, &staff);
    let q = Statement::new(format!(
        "SELECT {COLUMNS}, a.before, a.after, a.user_agent {FROM} WHERE a.id = ?"
    ))
    .bind(id.as_str());
    let rows = match s.services.db.query(q).await {
        Ok(r) => r,
        Err(e) => return v.error(&e),
    };
    let Some(e) = entries(&rows).into_iter().next() else {
        return v.not_found("/admin/audit");
    };
    if let Err(err) = v
        .log_read("audit.detail", Target::new("audit_log", id), &ctx)
        .await
    {
        return v.error(&err);
    }
    let json = |c: &str| {
        rows.get(0, c)
            .and_then(Value::as_text)
            .and_then(|t| serde_json::from_str::<Json>(t).ok())
            .map(audit::redact)
    };
    let (before, after) = (json("before"), json("after"));
    let ua = rows.get(0, "user_agent").and_then(Value::as_text);
    let l = |k: &str| v.t(k);
    let body = html! {
        p { a href="/admin/audit" { "← " (l("kernel.admin.audit.back")) } }
        h1 class="code" { (e.action) }
        section class="card" {
            dl class="facts" {
                dt { (l("kernel.admin.at")) } dd { (v.time(e.at)) }
                dt { (l("kernel.admin.audit.actor")) } dd { (actor(&e)) }
                dt { (l("kernel.admin.audit.target")) } dd { (target(&e)) }
            }
        }
        // Raw details (#123): the IP, the browser and the request id.
        (crate::ui::technical(&s, &staff, html! {
            dl class="facts" {
                dt { (l("kernel.admin.audit.ip")) } dd { (e.ip.as_deref().unwrap_or("–")) }
                dt { (l("kernel.admin.audit.user_agent")) } dd { (ua.unwrap_or("–")) }
                dt { (l("kernel.admin.audit.request")) } dd class="code" { (e.request_id.as_deref().unwrap_or("–")) }
            }
        }))
        section class="card" {
            h2 { (l("kernel.admin.audit.changes")) }
            (diff(&v, before.as_ref(), after.as_ref()))
        }
    };
    v.page(StatusCode::OK, &e.action, body)
}

/// One row per top-level key of either side; changed rows marked.
fn diff(v: &View<'_>, before: Option<&Json>, after: Option<&Json>) -> Markup {
    if before.is_none() && after.is_none() {
        return html! { p class="muted" { (v.t("kernel.admin.audit.no_values")) } };
    }
    let empty = Map::new();
    let obj = |j: Option<&Json>| -> Map<String, Json> {
        match j {
            Some(Json::Object(m)) => m.clone(),
            Some(Json::Null) | None => empty.clone(),
            Some(other) => Map::from_iter([(String::new(), other.clone())]),
        }
    };
    let (b, a) = (obj(before), obj(after));
    let keys: BTreeSet<&String> = b.keys().chain(a.keys()).collect();
    let cell = |m: &Map<String, Json>, k: &str| m.get(k).map(shown).unwrap_or_else(|| "–".into());
    let l = |k: &str| v.t(k);
    html! {
        table class="rows diff" {
            thead { tr {
                th { (l("kernel.admin.audit.key")) }
                th { (l("kernel.admin.audit.before")) }
                th { (l("kernel.admin.audit.after")) }
            } }
            tbody {
                @for k in keys {
                    @let changed = b.get(k.as_str()) != a.get(k.as_str());
                    tr class=[changed.then_some("changed")] {
                        td class="code" data-label=(l("kernel.admin.audit.key")) {
                            (k) @if changed { " " span class="chip override" { (l("kernel.admin.audit.changed")) } }
                        }
                        td data-label=(l("kernel.admin.audit.before")) { (cell(&b, k)) }
                        td data-label=(l("kernel.admin.audit.after")) { (cell(&a, k)) }
                    }
                }
            }
        }
    }
}

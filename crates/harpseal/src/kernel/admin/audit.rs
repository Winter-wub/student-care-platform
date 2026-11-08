//! `GET /admin/audit` (`kernel.audit.view`): the audit log, newest first,
//! 50 per page, filtered by actor (a Staff email, `kind`, `kind:id` or an
//! id), action prefix (`student.`), target (`kind` or `kind:id`) and local
//! date range. Every request writes one read row `audit.view` (target = the
//! filter summary), never one per row listed. The detail page is
//! `audit_detail.rs`.

use axum::extract::{Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::Response;
use maud::{Markup, html};
use serde::Deserialize;

use super::view::{PER_PAGE, View, cut, enc, like, paging};
use crate::audit::Target;
use crate::port_types::{Rows, Statement, Value};
use crate::{AppState, CurrentStaff, RequestCtx, Result};

/// Actor kinds of `audit_log.actor_kind`.
const ACTOR_KINDS: [&str; 4] = ["staff", "student", "system", "job"];

/// `?actor=&action=&target=&from=&to=&page=`.
#[derive(Debug, Default, Deserialize)]
pub(super) struct Filters {
    #[serde(default)]
    actor: String,
    #[serde(default)]
    action: String,
    #[serde(default)]
    target: String,
    #[serde(default)]
    from: String,
    #[serde(default)]
    to: String,
    #[serde(default)]
    page: String,
}

impl Filters {
    fn clean(self) -> Self {
        Self {
            actor: cut(&self.actor, 254),
            action: cut(&self.action, 64),
            target: cut(&self.target, 128),
            from: cut(&self.from, 10),
            to: cut(&self.to, 10),
            page: cut(&self.page, 9),
        }
    }

    fn query(&self) -> String {
        format!(
            "actor={}&action={}&target={}&from={}&to={}",
            enc(&self.actor),
            enc(&self.action),
            enc(&self.target),
            enc(&self.from),
            enc(&self.to)
        )
    }
}

/// One listed row.
pub(super) struct Entry {
    pub(super) id: String,
    pub(super) at: i64,
    pub(super) actor_kind: String,
    pub(super) actor_id: Option<String>,
    pub(super) actor_name: Option<String>,
    pub(super) action: String,
    pub(super) target_kind: Option<String>,
    pub(super) target_id: Option<String>,
    pub(super) ip: Option<String>,
    pub(super) request_id: Option<String>,
}

/// Columns every audit query selects, with the Staff name joined.
pub(super) const COLUMNS: &str = "a.id, a.at, a.actor_kind, a.actor_id, st.display_name AS actor_name, \
     a.action, a.target_kind, a.target_id, a.ip, a.request_id";
/// The table with the actor's Staff row.
pub(super) const FROM: &str = "FROM audit_log a LEFT JOIN staff st \
     ON a.actor_kind = 'staff' AND st.id = a.actor_id";

pub(super) async fn page(
    State(s): State<AppState>,
    staff: CurrentStaff,
    ctx: RequestCtx,
    headers: HeaderMap,
    Query(f): Query<Filters>,
) -> Response {
    let v = View::new(&s, &staff);
    let f = f.clean();
    // The read row is written before the query, so the list shows it.
    let target = Target::new("audit_log", f.query());
    if let Err(e) = v.log_read("audit.view", target, &ctx).await {
        return v.error(&e);
    }
    let (rows, total, page, pages) = match search(&v, &f).await {
        Ok(r) => r,
        Err(e) => return v.error(&e),
    };
    let results = results(&v, &f, &rows, total, page, pages);
    v.listing(&headers, results, |results| full(&v, &f, results))
}

/// `kind:id` / `kind` / bare value.
fn split(v: &str) -> (Option<&str>, Option<&str>) {
    match v.split_once(':') {
        Some((k, id)) => (Some(k.trim()), Some(id.trim()).filter(|i| !i.is_empty())),
        None if ACTOR_KINDS.contains(&v) => (Some(v), None),
        None => (None, Some(v)),
    }
}

async fn search(v: &View<'_>, f: &Filters) -> Result<(Vec<Entry>, i64, i64, i64)> {
    let mut wh = vec!["1 = 1".to_string()];
    let mut params: Vec<Value> = Vec::new();
    if f.actor.contains('@') {
        wh.push(
            "a.actor_kind = 'staff' AND a.actor_id = (SELECT id FROM staff WHERE email = ?)".into(),
        );
        params.push(f.actor.to_lowercase().into());
    } else if !f.actor.is_empty() {
        let (kind, id) = split(&f.actor);
        if let Some(k) = kind {
            wh.push("a.actor_kind = ?".into());
            params.push(k.into());
        }
        if let Some(id) = id {
            wh.push("a.actor_id = ?".into());
            params.push(id.into());
        }
    }
    if !f.action.is_empty() {
        wh.push("a.action LIKE ? ESCAPE '\\'".into());
        params.push(like(&f.action, false).into());
    }
    if !f.target.is_empty() {
        match f.target.split_once(':') {
            Some((k, id)) => {
                wh.push("a.target_kind = ? AND a.target_id = ?".into());
                params.push(k.trim().into());
                params.push(id.trim().into());
            }
            None => {
                wh.push("a.target_kind = ?".into());
                params.push(f.target.clone().into());
            }
        }
    }
    let (from, to) = v.range(&f.from, &f.to);
    if let Some(ms) = from {
        wh.push("a.at >= ?".into());
        params.push(ms.into());
    }
    if let Some(ms) = to {
        wh.push("a.at < ?".into());
        params.push(ms.into());
    }
    let cond = wh.join(" AND ");
    let db = &v.s.services.db;
    let mut count = Statement::new(format!("SELECT count(*) AS n {FROM} WHERE {cond}"));
    count.params = params.clone();
    let total = db.query(count).await?.get(0, "n").and_then(Value::as_i64);
    let total = total.unwrap_or(0);
    let (page, pages) = paging(total, &f.page);
    let mut list = Statement::new(format!(
        "SELECT {COLUMNS} {FROM} WHERE {cond} ORDER BY a.at DESC, a.id DESC LIMIT ? OFFSET ?"
    ));
    list.params = params;
    let list = list.bind(PER_PAGE).bind((page - 1) * PER_PAGE);
    Ok((entries(&db.query(list).await?), total, page, pages))
}

/// Rows of a query selecting [`COLUMNS`].
pub(super) fn entries(rows: &Rows) -> Vec<Entry> {
    let text = |i: usize, c: &str| rows.get(i, c).and_then(Value::as_text).map(str::to_string);
    (0..rows.len())
        .map(|i| Entry {
            id: text(i, "id").unwrap_or_default(),
            at: rows.get(i, "at").and_then(Value::as_i64).unwrap_or(0),
            actor_kind: text(i, "actor_kind").unwrap_or_default(),
            actor_id: text(i, "actor_id"),
            actor_name: text(i, "actor_name"),
            action: text(i, "action").unwrap_or_default(),
            target_kind: text(i, "target_kind"),
            target_id: text(i, "target_id"),
            ip: text(i, "ip"),
            request_id: text(i, "request_id"),
        })
        .collect()
}

/// The actor as shown: the Staff name, else `kind:id`.
pub(super) fn actor(e: &Entry) -> Markup {
    html! {
        @match (&e.actor_name, &e.actor_id) {
            (Some(name), Some(id)) => a href=(format!("/admin/staff/{id}")) { (name) },
            (None, Some(id)) => span class="code" { (e.actor_kind) ":" (id) },
            _ => span class="code" { (e.actor_kind) },
        }
    }
}

/// The target, linked when it is a Student or a Staff member.
pub(super) fn target(e: &Entry) -> Markup {
    let kind = e.target_kind.as_deref().unwrap_or_default();
    let id = e.target_id.as_deref().unwrap_or_default();
    let href = match kind {
        "student" => Some(format!("/admin/students/{id}")),
        "staff" => Some(format!("/admin/staff/{id}")),
        _ => None,
    };
    html! {
        @if kind.is_empty() { span class="muted" { "–" } }
        @else if let Some(h) = href { a href=(h) { (kind) ":" (id) } }
        @else { span class="code" { (kind) ":" (id) } }
    }
}

fn full(v: &View<'_>, f: &Filters, results: Markup) -> Response {
    let title = v.t("kernel.admin.audit.title");
    let field = |id: &str, name: &str, key: &str, value: &str, kind: &str| {
        html! {
            div class="field" {
                label for=(id) { (v.t(key)) }
                input id=(id) type=(kind) name=(name) value=(value) autocomplete="off";
            }
        }
    };
    let body = html! {
        h1 { (title) }
        form method="get" action="/admin/audit" class="card filters"
            hx-get="/admin/audit" hx-target="#audit-results" hx-swap="outerHTML"
            hx-push-url="true" hx-trigger="submit" {
            div class="row2" {
                (field("f-actor", "actor", "kernel.admin.audit.actor", &f.actor, "search"))
                (field("f-action", "action", "kernel.admin.audit.action", &f.action, "search"))
                (field("f-target", "target", "kernel.admin.audit.target", &f.target, "search"))
                (field("f-from", "from", "kernel.admin.from", &f.from, "date"))
                (field("f-to", "to", "kernel.admin.to", &f.to, "date"))
            }
            p class="muted" { (v.t("kernel.admin.audit.filter_help")) }
            (v.filter_buttons("/admin/audit"))
        }
        (results)
    };
    v.page(StatusCode::OK, &title, body)
}

fn results(v: &View<'_>, f: &Filters, rows: &[Entry], total: i64, page: i64, pages: i64) -> Markup {
    let q = f.query();
    let l = |k: &str| v.t(k);
    html! {
        div id="audit-results" {
            p class="muted count" { (l("kernel.admin.total")) " " (total) }
            @if rows.is_empty() {
                div class="card" { p { (l("kernel.admin.none")) } }
            } @else {
                table class="rows audit" {
                    thead { tr {
                        th { (l("kernel.admin.at")) }
                        th { (l("kernel.admin.audit.actor")) }
                        th { (l("kernel.admin.audit.action")) }
                        th { (l("kernel.admin.audit.target")) }
                    } }
                    tbody {
                        @for e in rows {
                            tr {
                                td data-label=(l("kernel.admin.at")) {
                                    a href=(format!("/admin/audit/{}", e.id)) { (v.time(e.at)) }
                                }
                                td data-label=(l("kernel.admin.audit.actor")) { (actor(e)) }
                                td data-label=(l("kernel.admin.audit.action")) {
                                    a class="code" href=(format!("/admin/audit/{}", e.id)) { (e.action) }
                                }
                                td data-label=(l("kernel.admin.audit.target")) { (target(e)) }
                            }
                        }
                    }
                }
            }
            @if !rows.is_empty() {
                // Raw details (#123): each row's IP and request id, linked to it.
                (crate::ui::technical(v.s, v.staff, html! {
                    ul class="list audit-tech" {
                        @for e in rows {
                            li {
                                a href=(format!("/admin/audit/{}", e.id)) {
                                    (v.time(e.at)) " " span class="code" { (e.action) }
                                }
                                span { (l("kernel.admin.audit.ip")) " " (e.ip.as_deref().unwrap_or("–")) }
                                span class="code" { (l("kernel.admin.audit.request")) " " (e.request_id.as_deref().unwrap_or("–")) }
                            }
                        }
                    }
                }))
            }
            (v.pager(page, pages, |p| format!("/admin/audit?{q}&page={p}")))
        }
    }
}

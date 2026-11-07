//! `GET /admin/security` (`kernel.security.view`): Security Events, newest
//! first, 50 per page, filtered by kind, Student (code or name) and local
//! date range. Every request (HTMX fragments included) writes one audit
//! read row `security.view` whose target is the filter summary; the page
//! is refused when that row cannot be written.

use axum::extract::{Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::Response;
use maud::{Markup, html};
use serde::Deserialize;
use serde_json::Value as Json;

use super::view::{PER_PAGE, View, cut, enc, like, paging, pairs, short_id};
use crate::audit::Target;
use crate::kernel::students::view::channel_label;
use crate::port_types::{Rows, Statement, Value};
use crate::{AppState, CurrentStaff, RequestCtx, Result, Services};

/// Kinds the Core writes, offered even before one happened.
const KNOWN_KINDS: [&str; 6] = [
    "otp.lockout",
    "link.duplicate_attempt",
    "inbound.failed",
    "inbound.lost",
    "otp.send_failed",
    "staff.lockout",
];

/// `?kind=&student=&from=&to=&page=`.
#[derive(Debug, Default, Deserialize)]
pub(super) struct Filters {
    #[serde(default)]
    kind: String,
    #[serde(default)]
    student: String,
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
            kind: cut(&self.kind, 64),
            student: cut(&self.student, 100),
            from: cut(&self.from, 10),
            to: cut(&self.to, 10),
            page: cut(&self.page, 9),
        }
    }

    /// The query string without `page`.
    fn query(&self) -> String {
        format!(
            "kind={}&student={}&from={}&to={}",
            enc(&self.kind),
            enc(&self.student),
            enc(&self.from),
            enc(&self.to)
        )
    }
}

struct Event {
    at: i64,
    kind: String,
    student: Option<(String, String, String)>,
    account: Option<(String, String)>,
    staff: Option<String>,
    details: Json,
}

pub(super) async fn page(
    State(s): State<AppState>,
    staff: CurrentStaff,
    ctx: RequestCtx,
    headers: HeaderMap,
    Query(f): Query<Filters>,
) -> Response {
    let v = View::new(&s, &staff);
    let f = f.clean();
    if let Err(e) = v
        .log_read(
            "security.view",
            Target::new("security_event", f.query()),
            &ctx,
        )
        .await
    {
        return v.error(&e);
    }
    let loaded =
        async { Ok::<_, crate::Error>((kinds(&s.services).await?, search(&v, &f).await?)) };
    let (kinds, (events, total, page, pages)) = match loaded.await {
        Ok(r) => r,
        Err(e) => return v.error(&e),
    };
    let results = results(&v, &f, &events, total, page, pages);
    v.listing(&headers, results, |results| full(&v, &f, &kinds, results))
}

/// Kinds in the table plus the known ones, sorted.
async fn kinds(s: &Services) -> Result<Vec<String>> {
    let rows =
        s.db.query(Statement::new("SELECT DISTINCT kind FROM security_event"))
            .await?;
    let mut kinds: Vec<String> = KNOWN_KINDS.iter().map(|k| k.to_string()).collect();
    for r in &rows.rows {
        if let Some(k) = r.first().and_then(Value::as_text)
            && !kinds.iter().any(|x| x == k)
        {
            kinds.push(k.to_string());
        }
    }
    kinds.sort();
    Ok(kinds)
}

async fn search(v: &View<'_>, f: &Filters) -> Result<(Vec<Event>, i64, i64, i64)> {
    let mut wh = vec!["1 = 1".to_string()];
    let mut params: Vec<Value> = Vec::new();
    if !f.kind.is_empty() {
        wh.push("e.kind = ?".into());
        params.push(f.kind.clone().into());
    }
    if !f.student.is_empty() {
        wh.push("(s.student_code = ? OR s.display_name LIKE ? ESCAPE '\\')".into());
        params.push(f.student.clone().into());
        params.push(like(&f.student, true).into());
    }
    let (from, to) = v.range(&f.from, &f.to);
    if let Some(ms) = from {
        wh.push("e.at >= ?".into());
        params.push(ms.into());
    }
    if let Some(ms) = to {
        wh.push("e.at < ?".into());
        params.push(ms.into());
    }
    let joins = "FROM security_event e LEFT JOIN student s ON s.id = e.student_id \
        LEFT JOIN channel_account ca ON ca.id = e.channel_account_id \
        LEFT JOIN staff st ON st.id = e.staff_id";
    let cond = wh.join(" AND ");
    let mut count = Statement::new(format!("SELECT count(*) AS n {joins} WHERE {cond}"));
    count.params = params.clone();
    let db = &v.s.services.db;
    let total = db.query(count).await?.get(0, "n").and_then(Value::as_i64);
    let total = total.unwrap_or(0);
    let (page, pages) = paging(total, &f.page);
    let mut list = Statement::new(format!(
        "SELECT e.at, e.kind, e.student_id, s.student_code, s.display_name, ca.channel, \
         ca.external_id, st.display_name AS staff_name, st.email AS staff_email, e.details \
         {joins} WHERE {cond} ORDER BY e.at DESC, e.id DESC LIMIT ? OFFSET ?"
    ));
    list.params = params;
    let list = list.bind(PER_PAGE).bind((page - 1) * PER_PAGE);
    let rows = db.query(list).await?;
    Ok((events(&rows), total, page, pages))
}

fn events(rows: &Rows) -> Vec<Event> {
    let text = |i: usize, c: &str| rows.get(i, c).and_then(Value::as_text).map(str::to_string);
    (0..rows.len())
        .map(|i| Event {
            at: rows.get(i, "at").and_then(Value::as_i64).unwrap_or(0),
            kind: text(i, "kind").unwrap_or_default(),
            student: text(i, "student_id").map(|id| {
                let code = text(i, "student_code").unwrap_or_default();
                (id, code, text(i, "display_name").unwrap_or_default())
            }),
            account: text(i, "channel").zip(text(i, "external_id")),
            staff: text(i, "staff_name").or_else(|| text(i, "staff_email")),
            details: text(i, "details")
                .and_then(|d| serde_json::from_str(&d).ok())
                .unwrap_or(Json::Null),
        })
        .collect()
}

/// A kind's label: `kernel.admin.kind.<kind with _>` when the catalog has
/// it, else the kind itself.
fn kind_label(v: &View<'_>, kind: &str) -> String {
    let key = format!("kernel.admin.kind.{}", kind.replace('.', "_"));
    if v.s.catalog.knows(&key) {
        v.t(&key)
    } else {
        kind.to_string()
    }
}

fn full(v: &View<'_>, f: &Filters, kinds: &[String], results: Markup) -> Response {
    let title = v.t("kernel.admin.security.title");
    let body = html! {
        h1 { (title) }
        form method="get" action="/admin/security" class="card filters"
            hx-get="/admin/security" hx-target="#security-results" hx-swap="outerHTML"
            hx-push-url="true" hx-trigger="change, submit" {
            div class="row2" {
                div class="field" {
                    label for="f-kind" { (v.t("kernel.admin.security.kind")) }
                    select id="f-kind" name="kind" {
                        option value="" { (v.t("kernel.admin.all")) }
                        @for k in kinds {
                            option value=(k) selected[*k == f.kind] { (kind_label(v, k)) " (" (k) ")" }
                        }
                    }
                }
                div class="field" {
                    label for="f-student" { (v.t("kernel.admin.security.student")) }
                    input id="f-student" type="search" name="student" value=(f.student) autocomplete="off";
                }
                div class="field" {
                    label for="f-from" { (v.t("kernel.admin.from")) }
                    input id="f-from" type="date" name="from" value=(f.from);
                }
                div class="field" {
                    label for="f-to" { (v.t("kernel.admin.to")) }
                    input id="f-to" type="date" name="to" value=(f.to);
                }
            }
            (v.filter_buttons("/admin/security"))
        }
        (results)
    };
    v.page(StatusCode::OK, &title, body)
}

fn results(v: &View<'_>, f: &Filters, rows: &[Event], total: i64, page: i64, pages: i64) -> Markup {
    let q = f.query();
    let label = |k: &str| v.t(k);
    html! {
        div id="security-results" {
            p class="muted count" { (v.t("kernel.admin.total")) " " (total) }
            @if rows.is_empty() {
                div class="card" { p { (v.t("kernel.admin.none")) } }
            } @else {
                table class="rows events" {
                    thead { tr {
                        th { (label("kernel.admin.at")) }
                        th { (label("kernel.admin.security.kind")) }
                        th { (label("kernel.admin.security.student")) }
                        th { (label("kernel.admin.security.account")) }
                        th { (label("kernel.admin.security.staff")) }
                        th { (label("kernel.admin.security.details")) }
                    } }
                    tbody {
                        @for e in rows {
                            tr {
                                td data-label=(label("kernel.admin.at")) { (v.time(e.at)) }
                                td data-label=(label("kernel.admin.security.kind")) {
                                    span class="chip" title=(e.kind) { (kind_label(v, &e.kind)) }
                                }
                                td data-label=(label("kernel.admin.security.student")) {
                                    @if let Some((id, code, name)) = &e.student {
                                        a href=(format!("/admin/students/{id}")) { (name) }
                                        " " span class="code" { (code) }
                                    } @else { span class="muted" { "–" } }
                                }
                                td data-label=(label("kernel.admin.security.account")) {
                                    @if let Some((ch, ext)) = &e.account {
                                        (channel_label(ch)) " " span class="code" { (short_id(ext)) }
                                    } @else { span class="muted" { "–" } }
                                }
                                td data-label=(label("kernel.admin.security.staff")) {
                                    @if let Some(n) = &e.staff { (n) } @else { span class="muted" { "–" } }
                                }
                                td data-label=(label("kernel.admin.security.details")) {
                                    dl class="kv" {
                                        @for (k, val) in pairs(&e.details) { dt { (k) } dd { (val) } }
                                    }
                                }
                            }
                        }
                    }
                }
            }
            (v.pager(page, pages, |p| format!("/admin/security?{q}&page={p}")))
        }
    }
}

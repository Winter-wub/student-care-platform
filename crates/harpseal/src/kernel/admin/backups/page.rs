//! `GET /admin/backups`: the runs, newest first (time, origin, status,
//! size, contents, error, download), the Job's recent errors, "back up
//! now" and "download all data". `POST /admin/backups/now` enqueues a run.

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::Response;
use maud::{Markup, html};
use serde_json::json;

use super::store::{self, Run};
use super::{DOWNLOAD, KIND, human_bytes};
use crate::audit::{self, Actor, RequestCtx};
use crate::backup::xlsx::{self, OPERATIONAL, Selection};
use crate::kernel::admin::view::{View, enc};
use crate::kernel::text::fill;
use crate::port_types::{Statement, Value};
use crate::ui::number;
use crate::{AppState, CurrentStaff, Result, Services};

/// Runs listed.
const SHOWN: i64 = 60;
/// Failed Job rows shown, and how far back.
const ERRORS: i64 = 5;
const ERRORS_DAYS: i64 = 30;

pub(super) async fn page(State(s): State<AppState>, staff: CurrentStaff) -> Response {
    let v = View::new(&s, &staff);
    let loaded = async {
        let tables = if staff.can(DOWNLOAD) {
            xlsx::sizes(&s.deployment, Selection::Every).await?
        } else {
            Vec::new()
        };
        Ok::<_, crate::Error>((
            store::list(&s.services, SHOWN).await?,
            job_errors(&s.services).await?,
            tables,
        ))
    };
    let (runs, errors, tables) = match loaded.await {
        Ok(x) => x,
        Err(e) => return v.error(&e),
    };
    let title = v.t("kernel.backup.title");
    let b = &s.config.backup;
    let days = b.retention_days.to_string();
    let offset = s.config.utc_offset.clone();
    let busy = runs.iter().any(|r| r.state == "running");
    let total = human_bytes(runs.iter().map(|r| r.bytes).sum());
    let latest_failed = runs
        .iter()
        .find(|r| r.state != "running")
        .is_some_and(|r| r.state == "failed");
    let body = html! {
        h1 { (title) }
        @if latest_failed {
            div class="flash err" id="backup-failed" role="alert" { (v.t("kernel.backup.failed_alert")) }
        }
        section class="card" {
            p { (fill(&v.t("kernel.backup.intro"), &[("days", &days)])) }
            @if !b.enabled { p class="muted" { (v.t("kernel.backup.disabled")) } }
            div class="actions" {
                form method="post" action="/admin/backups/now" {
                    (staff.csrf_field())
                    button type="submit" disabled[busy] { (v.t("kernel.backup.now")) }
                }
                @if staff.can(DOWNLOAD) {
                    a class="btn secondary" href="/admin/backups/export.xlsx" download {
                        (v.t("kernel.backup.export"))
                    }
                }
            }
            @if staff.can(DOWNLOAD) { p class="muted" { (v.t("kernel.backup.export_note")) } }
            p class="muted" id="backups-total" { (fill(&v.t("kernel.backup.total"), &[("size", &total)])) }
        }
        (table(&v, &runs))
        // Raw details (#123): the cron schedule, each run's error, the
        // Job's errors and the table names.
        (crate::ui::technical(&s, &staff, html! {
            @if b.enabled {
                p class="muted" {
                    (fill(&v.t("kernel.backup.schedule"), &[("schedule", &b.schedule), ("offset", &offset)]))
                }
            }
            p class="muted" { (v.t("kernel.backup.restore_hint")) }
            (run_errors(&v, &runs))
            @if !errors.is_empty() {
                section class="card" id="backup-job-errors" {
                    h2 { (v.t("kernel.backup.job_errors")) }
                    ul {
                        @for (at, e) in &errors { li { (v.time(*at)) ": " span class="code" { (e) } } }
                    }
                }
            }
            @if !tables.is_empty() { (table_list(&v, &tables)) }
        }))
    };
    v.page(StatusCode::OK, &title, body)
}

fn table(v: &View<'_>, runs: &[Run]) -> Markup {
    if runs.is_empty() {
        return html! { p class="muted" id="backups-empty" { (v.t("kernel.backup.none")) } };
    }
    html! {
        table class="rows" id="backups" aria-label=(v.t("kernel.backup.title")) {
            thead { tr {
                th { (v.t("kernel.backup.started")) } th { (v.t("kernel.backup.state")) }
                th { (v.t("kernel.backup.size")) } th { (v.t("kernel.backup.contents")) } th {}
            } }
            tbody {
                @for r in runs {
                    tr data-id=(r.id) data-state=(r.state) {
                        td data-label=(v.t("kernel.backup.started")) {
                            (v.time(r.started_at)) br;
                            span class="muted" { (v.t(&format!("kernel.backup.origin_{}", r.origin))) }
                        }
                        td data-label=(v.t("kernel.backup.state")) {
                            span class=(format!("chip {}", r.state)) {
                                (v.t(&format!("kernel.backup.state_{}", r.state)))
                            }
                        }
                        td data-label=(v.t("kernel.backup.size")) { (human_bytes(r.bytes)) }
                        td data-label=(v.t("kernel.backup.contents")) {
                            @if let (Some(t), Some(n), Some(f)) = (r.tables, r.rows, r.files) {
                                (fill(&v.t("kernel.backup.counts"), &[
                                    ("tables", &number(t)), ("rows", &number(n)),
                                    ("files", &number(f)),
                                ]))
                            }
                        }
                        td {
                            @if r.state == "done" && v.staff.can(DOWNLOAD) {
                                a class="btn secondary" download
                                    href=(format!("/admin/backups/{}/download", r.id)) {
                                    (v.t("kernel.backup.download"))
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

/// Each run's error and retention error, newest first, for the
/// technical details (#123).
fn run_errors(v: &View<'_>, runs: &[Run]) -> Markup {
    let failed: Vec<&Run> = runs
        .iter()
        .filter(|r| r.error.is_some() || r.prune_error.is_some())
        .collect();
    html! {
        @if !failed.is_empty() {
            ul class="run-errors" {
                @for r in failed {
                    li data-id=(r.id) {
                        (v.time(r.started_at)) ": "
                        @if let Some(e) = &r.error { span class="code" { (e) } }
                        @if let Some(e) = &r.prune_error {
                            " " span class="field-error prune-error" {
                                (fill(&v.t("kernel.backup.prune_failed"), &[("error", e)]))
                            }
                        }
                    }
                }
            }
        }
    }
}

/// Every table with its rows and a download of it alone.
fn table_list(v: &View<'_>, tables: &[(String, u64, u64)]) -> Markup {
    html! {
        section class="card" id="backup-tables" {
            h2 { (v.t("kernel.backup.tables")) }
            table class="rows" aria-label=(v.t("kernel.backup.tables")) {
                thead { tr { th { (v.t("kernel.backup.table")) } th { (v.t("kernel.backup.rows")) } th {} } }
                tbody {
                    @for (name, rows, _) in tables {
                        tr data-table=(name) {
                            td data-label=(v.t("kernel.backup.table")) {
                                span class="code" { (name) }
                                @if OPERATIONAL.contains(&name.as_str()) {
                                    br; span class="muted" { (v.t("kernel.backup.not_in_all")) }
                                }
                            }
                            td data-label=(v.t("kernel.backup.rows")) { (rows) }
                            td {
                                a class="btn secondary" download
                                    href=(format!("/admin/backups/export/{}", enc(name))) { "Excel" }
                            }
                        }
                    }
                }
            }
        }
    }
}

/// `(finished_at, last_error)` of the backup Jobs that failed lately.
async fn job_errors(s: &Services) -> Result<Vec<(i64, String)>> {
    let since = s.now_ms() - ERRORS_DAYS * super::DAY_MS;
    let rows =
        s.db.query(
            Statement::new(
                "SELECT finished_at, last_error FROM job WHERE kind = ? AND state = 'failed' \
                 AND finished_at >= ? ORDER BY finished_at DESC LIMIT ?",
            )
            .bind(KIND)
            .bind(since)
            .bind(ERRORS),
        )
        .await?;
    Ok((0..rows.len())
        .map(|i| {
            let at = rows.get(i, "finished_at").and_then(Value::as_i64);
            let e = rows.get(i, "last_error").and_then(Value::as_text);
            (at.unwrap_or(0), e.unwrap_or_default().to_string())
        })
        .collect())
}

/// Enqueues a manual run (one per minute however often it is pressed),
/// audited `backup.request`; refused while a run is going.
pub(super) async fn now(
    State(s): State<AppState>,
    staff: CurrentStaff,
    ctx: RequestCtx,
) -> Response {
    let v = View::new(&s, &staff);
    let work = async {
        if store::running(&s.services).await?.is_some() {
            return Ok(false);
        }
        let minute = s.services.now_ms().div_euclid(60_000);
        let key = format!("kernel.backup.manual:{minute}");
        let payload = json!({ "manual": staff.id });
        let (_, job) = s.jobs.insert_unique(&key, KIND, &payload, None)?;
        let audit = audit::entry(
            &s.services,
            &Actor::staff(&staff),
            "backup.request",
            None,
            None,
            None,
            &ctx,
        )?;
        s.services.db.batch(vec![job, audit]).await?;
        Ok::<_, crate::Error>(true)
    };
    match work.await {
        Ok(true) => v.redirect("/admin/backups", true, "kernel.backup.requested"),
        Ok(false) => v.redirect("/admin/backups", false, "kernel.backup.busy"),
        Err(e) => v.error(&e),
    }
}

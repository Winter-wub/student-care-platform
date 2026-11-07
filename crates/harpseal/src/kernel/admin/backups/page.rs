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
use crate::kernel::admin::view::View;
use crate::kernel::text::fill;
use crate::port_types::{Statement, Value};
use crate::{AppState, CurrentStaff, Result, Services};

/// Runs listed.
const SHOWN: i64 = 60;
/// Failed Job rows shown, and how far back.
const ERRORS: i64 = 5;
const ERRORS_DAYS: i64 = 30;

pub(super) async fn page(State(s): State<AppState>, staff: CurrentStaff) -> Response {
    let v = View::new(&s, &staff);
    let loaded = async {
        Ok::<_, crate::Error>((
            store::list(&s.services, SHOWN).await?,
            job_errors(&s.services).await?,
        ))
    };
    let (runs, errors) = match loaded.await {
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
            div class="flash error" id="backup-failed" role="alert" { (v.t("kernel.backup.failed_alert")) }
        }
        section class="card" {
            p { (fill(&v.t("kernel.backup.intro"), &[("days", &days)])) }
            @if b.enabled {
                p class="muted" {
                    (fill(&v.t("kernel.backup.schedule"), &[("schedule", &b.schedule), ("offset", &offset)]))
                }
            } @else {
                p class="muted" { (v.t("kernel.backup.disabled")) }
            }
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
            p class="muted" { (v.t("kernel.backup.restore_hint")) }
        }
        (table(&v, &runs))
        @if !errors.is_empty() {
            section class="card" id="backup-job-errors" {
                h2 { (v.t("kernel.backup.job_errors")) }
                ul {
                    @for (at, e) in &errors { li { (v.time(*at)) ": " span class="code" { (e) } } }
                }
            }
        }
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
                            @if let Some(e) = &r.error { br; span class="muted code" { (e) } }
                            @if let Some(e) = &r.prune_error {
                                br;
                                span class="field-error prune-error" {
                                    (fill(&v.t("kernel.backup.prune_failed"), &[("error", e)]))
                                }
                            }
                        }
                        td data-label=(v.t("kernel.backup.size")) { (human_bytes(r.bytes)) }
                        td data-label=(v.t("kernel.backup.contents")) {
                            @if let (Some(t), Some(n), Some(f)) = (r.tables, r.rows, r.files) {
                                (fill(&v.t("kernel.backup.counts"), &[
                                    ("tables", &t.to_string()), ("rows", &n.to_string()),
                                    ("files", &f.to_string()),
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

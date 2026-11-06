//! `GET /admin/roster/{id}`: one import, by state. `previewed`: the
//! counts, the red warning, who will become left and confirm / cancel.
//! `queued` / `running`: a progress bar that HTMX refreshes every 2 s
//! (`#roster-progress`, the only part an HTMX request gets). `done`: the
//! result. `failed`: the report of a refused file, or why the Job stopped.
//! Also the POSTs confirm / cancel and the downloads `report.csv` and
//! `file` (the original, a sensitive read: audit `roster.file.download`).

use axum::extract::{Path, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::{Html, IntoResponse, Response};
use maud::{Markup, html};

use super::service::{self, Outcome};
use super::store::{self, Counts, Import, Tally};
use super::view::{BASE, IMPORT, View, csv_body, download, is_fragment};
use crate::audit::{self, Actor, RequestCtx, Target};
use crate::{AppState, CurrentStaff};

/// Problems shown on the page; the CSV has all that were kept.
const SHOWN_PROBLEMS: usize = 50;

pub(super) async fn page(
    State(s): State<AppState>,
    staff: CurrentStaff,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Response {
    let v = View::new(&s, &staff);
    let imp = match store::load(&s.services, &id).await {
        Ok(Some(i)) => i,
        Ok(None) => return v.not_found(),
        Err(e) => return v.error(&e),
    };
    // An import whose Job died gives the slot up here (see `stuck.rs`).
    let imp = match super::stuck::reap(&s.services, &imp).await {
        Ok(true) => match store::load(&s.services, &id).await {
            Ok(Some(i)) => i,
            Ok(None) => return v.not_found(),
            Err(e) => return v.error(&e),
        },
        Ok(false) => imp,
        Err(e) => return v.error(&e),
    };
    let mut resp = if is_fragment(&headers) {
        (StatusCode::OK, Html(progress(&v, &imp).into_string())).into_response()
    } else {
        full(&v, &imp)
    };
    resp.headers_mut()
        .insert(header::VARY, HeaderValue::from_static("HX-Request"));
    resp
}

fn full(v: &View<'_>, imp: &Import) -> Response {
    let title = v.t("kernel.roster.import");
    let body = html! {
        p { a href=(BASE) { "← " (v.t("kernel.roster.back")) } }
        h1 { (title) " " (v.state(&imp.state)) }
        div class="card" {
            dl class="facts" {
                dt { (v.t("kernel.roster.file")) }
                dd {
                    (imp.file_name) " · "
                    a href=(format!("{BASE}/{}/file", imp.id)) { (v.t("kernel.roster.download_file")) }
                }
                dt { (v.t("kernel.roster.mode")) } dd { (v.mode(&imp.mode)) }
                dt { (v.t("kernel.roster.when")) } dd { (v.time(imp.created_at)) }
                dt { (v.t("kernel.roster.who")) } dd { (imp.staff) }
                dt { "SHA-256" } dd class="code" { (imp.counts.sha256) }
            }
        }
        @match imp.state.as_str() {
            "previewed" => (preview(v, imp)),
            "failed" => (failed(v, imp)),
            _ => (progress(v, imp)),
        }
        @if v.staff.can(IMPORT) && super::stuck::may_mark_failed(imp, v.s.services.now_ms()) {
            form method="post" action=(format!("{BASE}/{}/fail", imp.id)) {
                (v.staff.csrf_field())
                p class="muted" { (v.t("kernel.roster.stuck_help")) }
                button class="secondary" type="submit" { (v.t("kernel.roster.mark_failed")) }
            }
        }
    };
    v.page(StatusCode::OK, &title, body)
}

fn tally_rows(v: &View<'_>, t: &Tally) -> Markup {
    html! {
        dt { (v.t("kernel.roster.new")) } dd { (t.new) }
        dt { (v.t("kernel.roster.updated")) } dd { (t.updated) }
        dt { (v.t("kernel.roster.unchanged")) } dd { (t.unchanged) }
        dt { (v.t("kernel.roster.returning")) } dd { (t.returning) }
        dt { (v.t("kernel.roster.skipped_by_override")) } dd { (t.skipped_by_override) }
    }
}

fn preview(v: &View<'_>, imp: &Import) -> Markup {
    let p = &imp.counts.preview;
    let full = imp.mode == "full";
    html! {
        @if p.warning { div class="warn" role="alert" { (v.t("kernel.roster.warning")) } }
        div class="card" {
            dl class="facts counts" {
                dt { (v.t("kernel.roster.total")) } dd { (imp.total_rows) }
                (tally_rows(v, &p.tally))
                @if full {
                    dt { (v.t("kernel.roster.will_become_left")) }
                    dd data-count="will_become_left" { (p.will_become_left) }
                }
                dt { (v.t("kernel.roster.empty_email")) } dd { (p.empty_email) }
                dt { (v.t("kernel.roster.active_before")) } dd { (p.active_before) }
            }
            @if !p.left_names.is_empty() {
                h3 { (v.t("kernel.roster.left_names")) }
                ul class="names" { @for n in &p.left_names { li { (n) } } }
                @if p.left_more > 0 { p class="muted" { (v.t("kernel.roster.more")) " +" (p.left_more) } }
            }
        }
        @if v.staff.can(IMPORT) {
            div class="actions" {
                form method="post" action=(format!("{BASE}/{}/confirm", imp.id)) {
                    (v.staff.csrf_field())
                    button type="submit" { (v.t("kernel.roster.confirm")) }
                }
                form method="post" action=(format!("{BASE}/{}/cancel", imp.id)) {
                    (v.staff.csrf_field())
                    button class="secondary" type="submit" { (v.t("kernel.roster.cancel")) }
                }
            }
        }
    }
}

/// The part HTMX refreshes: progress while `queued` / `running` (and it
/// polls), the result once `done`.
fn progress(v: &View<'_>, imp: &Import) -> Markup {
    let live = matches!(imp.state.as_str(), "queued" | "running");
    let url = format!("{BASE}/{}", imp.id);
    let c: &Counts = &imp.counts;
    html! {
        div id="roster-progress" class="card" data-state=(imp.state)
            hx-get=[live.then_some(url.as_str())] hx-trigger=[live.then_some("every 2s")]
            hx-swap=[live.then_some("outerHTML")] {
            @if let Some(job) = imp.failure.as_ref().and_then(|f| f.job.as_ref()) {
                div class="warn" role="alert" { (v.t("kernel.roster.job_failed")) ": " (job) }
            }
            @if live {
                label for="roster-bar" { (v.t("kernel.roster.progress")) " "
                    span data-applied { (imp.applied_rows) } " / " (imp.total_rows) }
                progress id="roster-bar" max=(imp.total_rows.max(1)) value=(imp.applied_rows) {}
            } @else {
                h2 { (v.t("kernel.roster.result")) " " (v.state(&imp.state)) }
                dl class="facts counts" {
                    dt { (v.t("kernel.roster.progress")) } dd { (imp.applied_rows) " / " (imp.total_rows) }
                    (tally_rows(v, &c.result))
                    @if imp.mode == "full" {
                        dt { (v.t("kernel.roster.left")) } dd data-count="left" { (c.left) }
                    }
                    dt { (v.t("kernel.roster.links_revoked")) } dd { (c.links_revoked) }
                    dt { (v.t("kernel.roster.notices_failed")) } dd { (c.notices_failed) }
                }
                @if let Some(at) = imp.finished_at { p class="muted" { (v.time(at)) } }
            }
        }
    }
}

fn failed(v: &View<'_>, imp: &Import) -> Markup {
    let f = imp.failure.clone().unwrap_or_default();
    if f.job.is_some() {
        return progress(v, imp);
    }
    html! {
        div class="warn" role="alert" { (v.t("kernel.roster.refused")) }
        div class="card" {
            p { (v.t("kernel.roster.problems")) ": " strong data-count="problems" { (f.total) } }
            table class="rows" {
                thead { tr {
                    th { (v.t("kernel.roster.report_row")) }
                    th { (v.t("kernel.roster.report_column")) }
                    th { (v.t("kernel.roster.report_reason")) }
                } }
                tbody {
                    @for p in f.problems.iter().take(SHOWN_PROBLEMS) {
                        tr {
                            td data-label=(v.t("kernel.roster.report_row")) { (p.row) }
                            td data-label=(v.t("kernel.roster.report_column")) { (p.column) }
                            td data-label=(v.t("kernel.roster.report_reason")) {
                                (v.t(&format!("kernel.roster.reason_{}", p.reason)))
                            }
                        }
                    }
                }
            }
            p { a class="btn secondary" href=(format!("{BASE}/{}/report.csv", imp.id)) {
                (v.t("kernel.roster.report"))
            } }
        }
    }
}

pub(super) async fn confirm(
    State(s): State<AppState>,
    staff: CurrentStaff,
    ctx: RequestCtx,
    Path(id): Path<String>,
) -> Response {
    let v = View::new(&s, &staff);
    let back = format!("{BASE}/{id}");
    let actor = Actor::staff(&staff);
    match service::confirm(&s.services, &s.jobs, &id, &actor, &ctx).await {
        Ok(Outcome::Done) => v.redirect(&back, true, "kernel.roster.confirmed"),
        Ok(Outcome::NotPreviewed(_)) => v.redirect(&back, false, "kernel.roster.not_previewed"),
        Ok(Outcome::Missing) => v.not_found(),
        Err(e) => v.error(&e),
    }
}

pub(super) async fn cancel(
    State(s): State<AppState>,
    staff: CurrentStaff,
    ctx: RequestCtx,
    Path(id): Path<String>,
) -> Response {
    let v = View::new(&s, &staff);
    let back = format!("{BASE}/{id}");
    match service::cancel(&s.services, &id, &Actor::staff(&staff), &ctx).await {
        Ok(Outcome::Done) => v.redirect(&back, true, "kernel.roster.cancelled"),
        Ok(Outcome::NotPreviewed(_)) => v.redirect(&back, false, "kernel.roster.not_previewed"),
        Ok(Outcome::Missing) => v.not_found(),
        Err(e) => v.error(&e),
    }
}

/// Staff mark a stuck import failed (see `stuck.rs`).
pub(super) async fn fail(
    State(s): State<AppState>,
    staff: CurrentStaff,
    ctx: RequestCtx,
    Path(id): Path<String>,
) -> Response {
    let v = View::new(&s, &staff);
    let back = format!("{BASE}/{id}");
    let actor = Actor::staff(&staff);
    match super::stuck::mark_failed(&s.services, &id, &actor, &ctx).await {
        Ok(Outcome::Done) => v.redirect(&back, true, "kernel.roster.marked_failed"),
        Ok(Outcome::NotPreviewed(_)) => v.redirect(&back, false, "kernel.roster.not_stuck"),
        Ok(Outcome::Missing) => v.not_found(),
        Err(e) => v.error(&e),
    }
}

/// The validation report as CSV (row, column, problem in the Staff
/// member's Locale), read from `roster_import.error`.
pub(super) async fn report(
    State(s): State<AppState>,
    staff: CurrentStaff,
    Path(id): Path<String>,
) -> Response {
    let v = View::new(&s, &staff);
    let imp = match store::load(&s.services, &id).await {
        Ok(Some(i)) => i,
        Ok(None) => return v.not_found(),
        Err(e) => return v.error(&e),
    };
    let failure = imp.failure.unwrap_or_default();
    let problems = &failure.problems;
    let head = [
        v.t("kernel.roster.report_row"),
        v.t("kernel.roster.report_column"),
        v.t("kernel.roster.report_reason"),
    ];
    let mut lines = vec![super::csv::line(&head.each_ref().map(String::as_str))];
    for p in problems {
        let row = p.row.to_string();
        let reason = v.t(&format!("kernel.roster.reason_{}", p.reason));
        lines.push(super::csv::line(&[&row, &p.column, &reason]));
    }
    if (problems.len() as u64) < failure.total {
        let note = v
            .t("kernel.roster.report_truncated")
            .replace("{shown}", &problems.len().to_string())
            .replace("{total}", &failure.total.to_string());
        lines.push(super::csv::line(&[&note]));
    }
    let name = format!("roster-{id}-report.csv");
    download("text/csv; charset=utf-8", &name, &name, csv_body(lines))
}

/// The original file. Refused (500) when its audit row cannot be written.
pub(super) async fn file(
    State(s): State<AppState>,
    staff: CurrentStaff,
    ctx: RequestCtx,
    Path(id): Path<String>,
) -> Response {
    let v = View::new(&s, &staff);
    let imp = match store::load(&s.services, &id).await {
        Ok(Some(i)) => i,
        Ok(None) => return v.not_found(),
        Err(e) => return v.error(&e),
    };
    let bytes = match s.services.files.get(&imp.file_key).await {
        Ok(Some(b)) => b,
        Ok(None) => return v.not_found(),
        Err(e) => return v.error(&e),
    };
    let after = serde_json::json!({ "file_name": imp.file_name, "sha256": imp.counts.sha256 });
    let target = Target::new("roster_import", imp.id.as_str());
    let logged = audit::record(
        &s.services,
        &Actor::staff(&staff),
        "roster.file.download",
        Some(&target),
        None,
        Some(after),
        &ctx,
    )
    .await;
    if let Err(e) = logged {
        return v.error(&e);
    }
    let xlsx = super::file::is_xlsx(&bytes, &imp.file_name);
    let (kind, fallback) = if xlsx {
        (
            "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
            "roster.xlsx",
        )
    } else {
        ("text/csv; charset=utf-8", "roster.csv")
    };
    download(kind, &imp.file_name, fallback, bytes)
}

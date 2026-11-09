//! `GET /admin/roster/{id}`: one import, by state. `previewed`: the
//! counts, the red warning, who will become left and confirm / cancel.
//! `queued` / `running`: a progress bar that HTMX refreshes every 2 s
//! (`#roster-progress`, the only part an HTMX request gets). `done`: the
//! result. `failed`: the report of a refused file, or why the Job stopped.
//! Also the POSTs confirm / cancel and the downloads `report.csv` and
//! `file` (the original, a sensitive read: audit `roster.file.download`).

use axum::Form;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::{Html, IntoResponse, Response};
use maud::{Markup, html};

use super::page_problems as problems;
use super::reach;
use super::service::{self, Outcome};
use super::store::{self, Counts, Import};
use super::view::{BASE, IMPORT, View, download, is_fragment};
use crate::audit::{self, Actor, RequestCtx, Target};
use crate::{AppState, CurrentStaff};

pub(super) async fn page(
    State(s): State<AppState>,
    staff: CurrentStaff,
    headers: HeaderMap,
    Path(id): Path<String>,
    Query(q): Query<Vec<(String, String)>>,
) -> Response {
    let v = View::new(&s, &staff);
    // `?typed=wrong`: the last confirm's typed text did not match (#120).
    let wrong = q.iter().any(|(k, val)| k == reach::FIELD && val == "wrong");
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
        // The per-Student lists (#134), only where they are shown.
        let head = if imp.state == "previewed" && imp.counts.preview.detailed {
            match super::diff::head(&s.services, &imp.id).await {
                Ok(h) => h,
                Err(e) => return v.error(&e),
            }
        } else {
            None
        };
        full(&v, &imp, wrong, head.as_ref())
    };
    resp.headers_mut()
        .insert(header::VARY, HeaderValue::from_static("HX-Request"));
    resp
}

fn full(v: &View<'_>, imp: &Import, wrong: bool, head: Option<&super::diff::Head>) -> Response {
    let title = v.t("kernel.roster.import");
    let body = html! {
        p { a class="back" href=(BASE) { "← " (v.t("kernel.roster.back")) } }
        h1 { (title) " " (v.state(&imp.state)) }
        div class="card" {
            dl class="facts" {
                dt { (v.t("kernel.roster.file")) }
                dd {
                    (imp.file_name) " · "
                    @if imp.file_key.is_empty() {
                        span class="muted" { (v.t("kernel.roster.file_pruned")) }
                    } @else {
                        a href=(format!("{BASE}/{}/file", imp.id)) { (v.t("kernel.roster.download_file")) }
                    }
                }
                dt { (v.t("kernel.roster.mode")) } dd { (v.mode(&imp.mode)) }
                dt { (v.t("kernel.roster.when")) } dd { (v.time(imp.created_at)) }
                dt { (v.t("kernel.roster.who")) } dd { (imp.staff) }
            }
        }
        @match imp.state.as_str() {
            "previewed" => (super::page_preview::preview(v, imp, wrong, head)),
            "failed" => (failed(v, imp)),
            _ => { (progress(v, imp)) (problems::warnings_card(v, imp)) }
        }
        @if v.staff.can(IMPORT) && super::stuck::may_mark_failed(imp, v.s.services.now_ms()) {
            form method="post" action=(format!("{BASE}/{}/fail", imp.id)) {
                (v.staff.csrf_field())
                p class="muted" { (v.t("kernel.roster.stuck_help")) }
                button class="secondary" type="submit" { (v.t("kernel.roster.mark_failed")) }
            }
        }
        // Raw details (#125): the file's fingerprint.
        (crate::ui::technical(v.s, v.staff, html! {
            dl class="facts" { dt { "SHA-256" } dd class="code" { (imp.counts.sha256) } }
        }))
    };
    v.page(StatusCode::OK, &title, body)
}

/// The part HTMX refreshes: progress while `queued` / `running` (and it
/// polls), the result and the next step once `done`.
fn progress(v: &View<'_>, imp: &Import) -> Markup {
    let live = matches!(imp.state.as_str(), "queued" | "running");
    let url = format!("{BASE}/{}", imp.id);
    let c: &Counts = &imp.counts;
    html! {
        div id="roster-progress" data-state=(imp.state)
            hx-get=[live.then_some(url.as_str())] hx-trigger=[live.then_some("every 2s")]
            hx-swap=[live.then_some("outerHTML")] {
          div class="card" {
            @if let Some(job) = imp.failure.as_ref().and_then(|f| f.job.as_ref()) {
                // The alert only without a flash (#129): after "mark failed"
                // the flash announces; a poll carries none, so it does.
                div class="warn" role=[v.alert()] { (v.t("kernel.roster.job_failed")) ": " (job) }
            }
            @if live {
                label for="roster-bar" { (v.t("kernel.roster.progress")) " "
                    span data-applied { (imp.applied_rows) } " / " (imp.total_rows) }
                progress id="roster-bar" max=(imp.total_rows.max(1)) value=(imp.applied_rows) {}
            } @else {
                h2 { (v.t("kernel.roster.result")) " " (v.state(&imp.state)) }
                dl class="facts" {
                    dt { (v.t("kernel.roster.progress")) } dd { (imp.applied_rows) " / " (imp.total_rows) }
                }
                (super::view::compared(v, imp))
                dl class="facts counts" {
                    dt { (v.t("kernel.roster.links_revoked")) } dd { (crate::ui::number(c.links_revoked)) }
                    dt { (v.t("kernel.roster.notices_failed")) } dd { (crate::ui::number(c.notices_failed)) }
                }
                @if let Some(at) = imp.finished_at { p class="muted" { (v.time(at)) } }
            }
          }
            // What comes next (#127): tell Students. A roster file cannot
            // name its Audience exactly, so Staff choose it on the form.
            @if imp.state == "done" {
                (crate::ui::next_step(v.s, v.staff, &[], (&v.t("kernel.roster.back"), BASE)))
            }
        }
    }
}

fn failed(v: &View<'_>, imp: &Import) -> Markup {
    let f = imp.failure.clone().unwrap_or_default();
    if f.job.is_some() {
        return html! { (progress(v, imp)) (problems::warnings_card(v, imp)) };
    }
    // A header error (#134): found beside expected, and a new upload.
    if super::page_headers::applies(&f) {
        return super::page_headers::page(v, imp, &f);
    }
    html! {
        // The live region only without a flash (#129): after the upload the
        // flash announces the refusal.
        div class="warn" role=[v.alert()] { (v.t("kernel.roster.refused")) }
        (super::skipped::card(v, &imp.counts.skipped))
        div class="card" {
            h2 { (v.t("kernel.roster.problems")) ": " strong data-count="problems" { (crate::ui::number(f.total)) } }
            (problems::classes(v, &f.classes))
            (problems::table(v, &f.problems))
            p { a class="btn secondary" href=(format!("{BASE}/{}/report.csv", imp.id)) {
                (v.t("kernel.roster.report"))
            } }
        }
        (problems::expected(v, &f.problems))
        (problems::warnings_card(v, imp))
    }
}

pub(super) async fn confirm(
    State(s): State<AppState>,
    staff: CurrentStaff,
    ctx: RequestCtx,
    Path(id): Path<String>,
    Form(form): Form<Vec<(String, String)>>,
) -> Response {
    let v = View::new(&s, &staff);
    let back = format!("{BASE}/{id}");
    // A broad full replace needs the typed reach (#120), checked here and
    // never trusted from the page: nothing changes when it does not match.
    let imp = match store::load(&s.services, &id).await {
        Ok(Some(i)) => i,
        Ok(None) => return v.not_found(),
        Err(e) => return v.error(&e),
    };
    let typed = form
        .iter()
        .find(|(k, _)| k == reach::FIELD)
        .map(|(_, t)| t.as_str());
    // Whatever else refuses it (state, deleted file, mode) answers first.
    let askable = imp.state == "previewed"
        && !imp.file_key.is_empty()
        && s.config.roster.allows_mode(&imp.mode);
    if askable && !reach::typed_ok(&imp, typed) {
        let again = format!("{back}?{}=wrong#roster-typed", reach::FIELD);
        return v.redirect(&again, false, "kernel.roster.typed_refused");
    }
    let actor = Actor::staff(&staff);
    match service::confirm(&s.services, &s.config.roster, &s.jobs, &id, &actor, &ctx).await {
        Ok(Outcome::Done) => v.redirect(&back, true, "kernel.roster.confirmed"),
        Ok(Outcome::NotPreviewed(_)) => v.redirect(&back, false, "kernel.roster.not_previewed"),
        Ok(Outcome::FileDeleted) => v.redirect(&back, false, "kernel.roster.file_pruned"),
        Ok(Outcome::ModeNotAllowed) => {
            super::page_list::refused(&v, "kernel.roster.mode_no_longer")
        }
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
        Ok(Outcome::NotPreviewed(_) | Outcome::FileDeleted | Outcome::ModeNotAllowed) => {
            v.redirect(&back, false, "kernel.roster.not_previewed")
        }
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
        Ok(Outcome::NotPreviewed(_) | Outcome::FileDeleted | Outcome::ModeNotAllowed) => {
            v.redirect(&back, false, "kernel.roster.not_stuck")
        }
        Ok(Outcome::Missing) => v.not_found(),
        Err(e) => v.error(&e),
    }
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
    if imp.file_key.is_empty() {
        // Deleted a year after upload (`prune.rs`).
        return v.not_found();
    }
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
    let xlsx = super::file::is_xlsx(&imp.file_name);
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

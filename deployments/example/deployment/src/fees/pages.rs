//! The Fees Module's two pages: the upload form with its help beside it
//! (#127: the columns declared once in [`COLUMNS`] give the panel and the
//! template) and the preview. After a confirm it says the import is
//! queued (or failed); once the Job wrote the balances it is the result,
//! with the next step ([`super::result`]).

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use harpseal::{AppState, CurrentStaff, Statement, UploadColumn, Uploads};
use maud::html;

use super::read::{AMOUNT, CODE, NAME, baht};
use super::{BASE, job_failed, notice, open, render, result, t};

/// The upload route, under `/admin`.
pub const UPLOAD: &str = "/fees/upload";

/// The file's columns, for the help panel and the template.
pub const COLUMNS: &[UploadColumn] = &[
    UploadColumn::required(CODE, "fees.col.code", "68010001"),
    UploadColumn::optional(NAME, "fees.col.name", "นกน้อย ทะเลใส"),
    UploadColumn::required(AMOUNT, "fees.col.amount", "12,500.00"),
];

pub(super) async fn page(State(s): State<AppState>, staff: CurrentStaff) -> Response {
    let q = Statement::new("SELECT COUNT(*) AS n FROM fees_balance");
    let stored = match s.services.db.query(q).await {
        Ok(rows) => rows.get(0, "n").and_then(|v| v.as_i64()).unwrap_or(0),
        Err(_) => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    };
    let body = html! {
        h1 { (t(&s, &staff, "fees.title")) }
        p { (t(&s, &staff, "fees.stored")) ": " strong id="stored" { (stored) } }
        div class="cols" {
            form class="card" method="post" action=(format!("/admin{UPLOAD}"))
                enctype="multipart/form-data" {
                (staff.csrf_field())
                div class="field" {
                    label for="fees-file" { (t(&s, &staff, "fees.file")) }
                    input id="fees-file" type="file" name="file" required accept=".csv,.xlsx";
                }
                button type="submit" { (t(&s, &staff, "fees.check")) }
            }
            (harpseal::ui::upload_help(&s, &staff, UPLOAD))
        }
    };
    render(&s, &staff, StatusCode::OK, body)
}

pub(super) async fn preview(
    State(s): State<AppState>,
    staff: CurrentStaff,
    uploads: Uploads,
    Path(id): Path<String>,
) -> Response {
    // Imported: the durable result, read from the Module's tables (the Job
    // deleted the kept upload), with the next step.
    // Unreadable: the upload's own state below still says where it stands.
    if let Ok(Some(imp)) = result::imported(&s, &staff, &id).await {
        return result::page(&s, &staff, &imp);
    }
    let (kept, p) = match open(&s, &staff, &uploads, &id).await {
        Ok(x) => x,
        // A Job that failed for good may have outlived its upload.
        Err(_) if job_failed(&s, &id, &staff.id, false).await => {
            let text = t(&s, &staff, "fees.failed");
            return notice(&s, &staff, StatusCode::CONFLICT, &text);
        }
        Err(r) => return *r,
    };
    let action = |what: &str| format!("{BASE}/{}/{what}", kept.id);
    let failed = kept.confirmed_at.is_some() && job_failed(&s, &kept.id, &staff.id, true).await;
    let body = html! {
        h1 { (t(&s, &staff, "fees.preview")) }
        p class="muted" { (kept.file_name) " · " span id="row-count" { (p.fees.len()) } }
        @if !p.problems.is_empty() {
            h2 { (t(&s, &staff, "fees.problems")) }
            table class="rows" id="problems" { tbody {
                @for pr in &p.problems {
                    tr { td { (pr.sheet) } td { (pr.row) } td { (pr.text) } }
                }
            } }
        }
        // Captions on every cell, amounts right-aligned (#131).
        @let (sheet, row) = (t(&s, &staff, "fees.col_sheet"), t(&s, &staff, "fees.col_row"));
        @let sheets = p.fees.iter().any(|f| !f.sheet.is_empty());
        table class="rows" id="fees" {
            thead { tr {
                @if sheets { th { (sheet) } }
                th class="num" { (row) } th { (CODE) } th { (NAME) } th class="num" { (AMOUNT) }
            } }
            tbody {
                @for f in &p.fees {
                    tr {
                        @if sheets { td data-label=(sheet) { (f.sheet) } }
                        td class="num" data-label=(row) { (f.row) }
                        td class="code" data-label=(CODE) { (f.code) }
                        td data-label=(NAME) { (f.name) }
                        td class="num" data-label=(AMOUNT) { (baht(f.satang)) }
                    }
                }
            }
        }
        @if !p.skipped.is_empty() || !p.hidden.is_empty() {
            h2 { (t(&s, &staff, "fees.skipped")) }
            ul id="skipped" {
                @for r in &p.skipped { li { (r.sheet) " แถว " (r.row) ": " (r.text) } }
                @for h in &p.hidden { li { "ชีตที่ซ่อน: " (h) } }
            }
        }
        @if kept.confirmed_at.is_some() {
            @if failed {
                div class="flash err" role="status" id="job-failed" { (t(&s, &staff, "fees.failed")) }
            } @else {
                div class="flash ok" role="status" id="queued" { (t(&s, &staff, "fees.queued")) }
                // Students are told once the balances are written (#127).
                p class="muted" id="pending" { (t(&s, &staff, "fees.pending")) }
            }
        } @else {
            @if p.importable() {
                form method="post" action=(action("confirm")) {
                    (staff.csrf_field())
                    button type="submit" { (t(&s, &staff, "fees.confirm")) }
                }
            }
            form method="post" action=(action("cancel")) {
                (staff.csrf_field())
                button class="secondary" type="submit" { (t(&s, &staff, "fees.cancel")) }
            }
        }
    };
    render(&s, &staff, StatusCode::OK, body)
}

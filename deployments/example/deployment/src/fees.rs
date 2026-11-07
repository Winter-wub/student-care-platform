//! A customer Module's back-office spreadsheet upload (#74): "Fees"
//! imports each Student's outstanding balance from a CSV or XLSX file the
//! finance office exports, one term per sheet.
//!
//! 1. `GET /admin/fees`: the upload form and how many balances are stored.
//! 2. `POST /admin/fees/upload` ([`ModuleRegistrar::sheet_upload`]: the
//!    roster's 4 MiB limit, 413 above it): the file must read
//!    ([`harpseal::sheet`]), then it is kept ([`SheetUpload::keep`]) and
//!    the browser goes to its preview.
//! 3. `GET /admin/fees/{id}`: the rows, the problems (sheet and row), the
//!    skipped totals rows and hidden sheets, with Confirm (only when
//!    nothing blocks) and Cancel.
//! 4. `POST …/confirm` re-reads the kept file ([`KeptUpload::open`]
//!    refuses one whose hash changed) and enqueues the Job `fees.import`;
//!    `POST …/cancel` deletes it ([`KeptUpload::discard`]).
//! 5. The Job reads it once more, writes `fees_balance` and discards it.
//!    An upload nobody confirms is deleted by the tick after 24 hours.

pub mod read;

use axum::extract::{Path, State};
use axum::http::{StatusCode, header};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::{get, post};
use harpseal::audit::{self, Actor, RequestCtx, Target};
use harpseal::{
    AppState, CatalogEntry, CurrentStaff, Job, JobContext, JobSpec, KeptUpload, Module,
    ModuleRegistrar, Permission, Result, SheetUpload, Statement, UploadRejection,
};
use maud::{Markup, html};

use read::{Parsed, baht};

/// The Module's name, which scopes its kept uploads.
pub const NAME: &str = "fees";
/// The Permission for every page of the Module.
pub const IMPORT: &str = "fees.import";
/// The Job that imports a confirmed file.
pub const JOB: &str = "fees.import";
/// The pages, under `/admin`.
pub const BASE: &str = "/admin/fees";

/// The Module.
pub struct Fees;

const TEXTS: &[CatalogEntry] = &[
    CatalogEntry::new("fees.title", "th", "ยอดค้างชำระ"),
    CatalogEntry::new("fees.title", "en", "Outstanding fees"),
    CatalogEntry::new("fees.file", "th", "ไฟล์ CSV หรือ XLSX (ไม่เกิน 4 MB)"),
    CatalogEntry::new("fees.file", "en", "CSV or XLSX file (up to 4 MB)"),
    CatalogEntry::new("fees.check", "th", "ตรวจไฟล์"),
    CatalogEntry::new("fees.check", "en", "Check the file"),
    CatalogEntry::new("fees.stored", "th", "ยอดที่บันทึกไว้"),
    CatalogEntry::new("fees.stored", "en", "Balances stored"),
    CatalogEntry::new("fees.preview", "th", "ตรวจก่อนนำเข้า"),
    CatalogEntry::new("fees.preview", "en", "Check before importing"),
    CatalogEntry::new("fees.confirm", "th", "ยืนยันนำเข้า"),
    CatalogEntry::new("fees.confirm", "en", "Confirm import"),
    CatalogEntry::new("fees.cancel", "th", "ยกเลิก"),
    CatalogEntry::new("fees.cancel", "en", "Cancel"),
    CatalogEntry::new("fees.problems", "th", "ปัญหาที่ต้องแก้ก่อนนำเข้า"),
    CatalogEntry::new("fees.problems", "en", "Fix these before importing"),
    CatalogEntry::new("fees.skipped", "th", "ข้ามไม่นำเข้า"),
    CatalogEntry::new("fees.skipped", "en", "Left out"),
    CatalogEntry::new("fees.gone", "th", "ไม่พบไฟล์นี้ หรือหมดเวลาแล้ว อัปโหลดใหม่"),
    CatalogEntry::new(
        "fees.gone",
        "en",
        "This file is gone or expired. Upload it again.",
    ),
    CatalogEntry::new("fees.changed", "th", "ไฟล์ถูกแก้ไขหลังอัปโหลด นำเข้าไม่ได้"),
    CatalogEntry::new(
        "fees.changed",
        "en",
        "The file changed after upload; it cannot be imported.",
    ),
    CatalogEntry::new("fees.unreadable", "th", "อ่านไฟล์ไม่ได้"),
    CatalogEntry::new("fees.unreadable", "en", "The file cannot be read."),
];

impl Module for Fees {
    fn name(&self) -> &'static str {
        NAME
    }

    fn register(&self, r: &mut ModuleRegistrar) {
        r.permission(Permission::new(
            IMPORT,
            "Upload and import outstanding fees",
        ))
        .messages(TEXTS)
        .migration(
            "0001_fees_balance",
            include_str!("../migrations/fees/0001_fees_balance.sql"),
        )
        .back_office_page(IMPORT, "/fees", "fees.title", 60, get(page))
        .sheet_upload(IMPORT, "/fees/upload", post(upload))
        .back_office(IMPORT, "/fees/{id}", get(preview))
        .back_office(IMPORT, "/fees/{id}/confirm", post(confirm))
        .back_office(IMPORT, "/fees/{id}/cancel", post(cancel))
        .job(JobSpec::new(JOB), run);
    }
}

fn t(s: &AppState, staff: &CurrentStaff, key: &str) -> String {
    s.catalog.t(&staff.locale, key)
}

fn render(s: &AppState, staff: &CurrentStaff, status: StatusCode, body: Markup) -> Response {
    let title = t(s, staff, "fees.title");
    let page = harpseal::ui::back_office(s, staff, &title, body);
    (status, Html(page.into_string())).into_response()
}

/// A short message on the Module's page, with `status`.
fn notice(s: &AppState, staff: &CurrentStaff, status: StatusCode, text: &str) -> Response {
    let body = html! {
        div class="flash err" role="status" { (text) }
        p { a class="btn secondary" href=(BASE) { (t(s, staff, "fees.title")) } }
    };
    render(s, staff, status, body)
}

fn see_other(to: &str) -> Response {
    (StatusCode::SEE_OTHER, [(header::LOCATION, to.to_string())]).into_response()
}

async fn page(State(s): State<AppState>, staff: CurrentStaff) -> Response {
    let q = Statement::new("SELECT COUNT(*) AS n FROM fees_balance");
    let stored = match s.services.db.query(q).await {
        Ok(rows) => rows.get(0, "n").and_then(|v| v.as_i64()).unwrap_or(0),
        Err(_) => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    };
    let body = html! {
        h1 { (t(&s, &staff, "fees.title")) }
        p { (t(&s, &staff, "fees.stored")) ": " strong id="stored" { (stored) } }
        form class="card" method="post" action=(format!("{BASE}/upload"))
            enctype="multipart/form-data" {
            (staff.csrf_field())
            div class="field" {
                label for="fees-file" { (t(&s, &staff, "fees.file")) }
                input id="fees-file" type="file" name="file" required accept=".csv,.xlsx";
            }
            button type="submit" { (t(&s, &staff, "fees.check")) }
        }
    };
    render(&s, &staff, StatusCode::OK, body)
}

async fn upload(
    State(s): State<AppState>,
    staff: CurrentStaff,
    ctx: RequestCtx,
    up: std::result::Result<SheetUpload, UploadRejection>,
) -> Response {
    let up = match up {
        Ok(u) => u,
        Err(r) => return notice(&s, &staff, r.status(), r.text()),
    };
    // A file that does not read at all is refused now, not kept.
    if up.workbook().is_err() {
        let text = t(&s, &staff, "fees.unreadable");
        return notice(&s, &staff, StatusCode::UNPROCESSABLE_ENTITY, &text);
    }
    match up.keep(&s.services, NAME, &staff, &ctx).await {
        Ok(kept) => see_other(&format!("{BASE}/{}", kept.id)),
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

/// The kept upload and what it holds, or the page to answer instead.
async fn open(
    s: &AppState,
    staff: &CurrentStaff,
    id: &str,
) -> std::result::Result<(KeptUpload, Parsed), Box<Response>> {
    let gone = |key: &str, status| Box::new(notice(s, staff, status, &t(s, staff, key)));
    match KeptUpload::open(&s.services, NAME, id).await {
        Ok(Some(k)) => match k.workbook() {
            Ok(book) => {
                let parsed = read::parse(&book);
                Ok((k, parsed))
            }
            Err(_) => Err(gone("fees.unreadable", StatusCode::UNPROCESSABLE_ENTITY)),
        },
        Ok(None) => Err(gone("fees.gone", StatusCode::NOT_FOUND)),
        Err(_) => Err(gone("fees.changed", StatusCode::CONFLICT)),
    }
}

async fn preview(
    State(s): State<AppState>,
    staff: CurrentStaff,
    Path(id): Path<String>,
) -> Response {
    let (kept, p) = match open(&s, &staff, &id).await {
        Ok(x) => x,
        Err(r) => return *r,
    };
    let action = |what: &str| format!("{BASE}/{}/{what}", kept.id);
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
        table class="rows" id="fees" { tbody {
            @for f in &p.fees {
                tr { td { (f.sheet) } td { (f.row) } td class="code" { (f.code) }
                     td { (f.name) } td { (baht(f.satang)) } }
            }
        } }
        @if !p.skipped.is_empty() || !p.hidden.is_empty() {
            h2 { (t(&s, &staff, "fees.skipped")) }
            ul id="skipped" {
                @for r in &p.skipped { li { (r.sheet) " แถว " (r.row) ": " (r.text) } }
                @for h in &p.hidden { li { "ชีตที่ซ่อน: " (h) } }
            }
        }
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
    };
    render(&s, &staff, StatusCode::OK, body)
}

async fn confirm(
    State(s): State<AppState>,
    staff: CurrentStaff,
    ctx: RequestCtx,
    Path(id): Path<String>,
) -> Response {
    let (kept, p) = match open(&s, &staff, &id).await {
        Ok(x) => x,
        Err(r) => return *r,
    };
    if !p.importable() {
        return see_other(&format!("{BASE}/{}", kept.id));
    }
    let payload = serde_json::json!({ "upload": kept.id });
    let key = format!("{JOB}:{}", kept.id);
    if s.jobs
        .enqueue_unique(&key, JOB, payload, None)
        .await
        .is_err()
    {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }
    let after = serde_json::json!({ "file_name": kept.file_name, "sha256": kept.sha256,
                                    "rows": p.fees.len() });
    let target = Target::new("module_upload", kept.id.as_str());
    let actor = Actor::staff(&staff);
    let audited = audit::record(
        &s.services,
        &actor,
        "fees.import",
        Some(&target),
        None,
        Some(after),
        &ctx,
    );
    if audited.await.is_err() {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }
    see_other(BASE)
}

async fn cancel(
    State(s): State<AppState>,
    _staff: CurrentStaff,
    Path(id): Path<String>,
) -> Response {
    match KeptUpload::discard(&s.services, NAME, &id).await {
        Ok(_) => see_other(BASE),
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

/// The Job: imports the kept file, then discards it. Idempotent: a file
/// already discarded means a previous run finished.
async fn run(cx: JobContext, job: Job) -> Result<()> {
    let invalid = |m: &str| harpseal::Error::Invalid(format!("fees import: {m}"));
    let payload: serde_json::Value =
        serde_json::from_str(&job.payload).map_err(|_| invalid("payload"))?;
    let id = payload["upload"]
        .as_str()
        .ok_or_else(|| invalid("payload"))?;
    let Some(kept) = KeptUpload::open(&cx.services, NAME, id).await? else {
        return Ok(());
    };
    let book = kept
        .workbook()
        .map_err(|_| invalid("the file no longer reads"))?;
    let p = read::parse(&book);
    if !p.importable() {
        return Err(invalid("the file has problems"));
    }
    let now = cx.services.now_ms();
    let stmts = p
        .fees
        .iter()
        .map(|f| {
            Statement::new(
                "INSERT INTO fees_balance (student_code, amount_satang, upload_id, updated_at) \
                 VALUES (?, ?, ?, ?) ON CONFLICT (student_code) DO UPDATE SET \
                 amount_satang = excluded.amount_satang, upload_id = excluded.upload_id, \
                 updated_at = excluded.updated_at",
            )
            .bind(f.code.as_str())
            .bind(f.satang)
            .bind(id)
            .bind(now)
        })
        .collect();
    cx.services.db.batch(stmts).await?;
    KeptUpload::discard(&cx.services, NAME, id).await?;
    Ok(())
}

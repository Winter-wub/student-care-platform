//! A customer Module's back-office spreadsheet upload (#74): "Fees"
//! imports each Student's outstanding balance from a CSV or XLSX file the
//! finance office exports, one term per sheet.
//!
//! 1. `GET /admin/fees`: the upload form, with the help panel the
//!    declared `pages::COLUMNS` give it (#127), and how many balances
//!    are stored.
//! 2. `POST /admin/fees/upload` ([`ModuleRegistrar::sheet_upload`]: the
//!    roster's 4 MiB limit, 413 above it): the file must read
//!    ([`harpseal::sheet`]), then it is kept ([`Uploads::keep`]) and the
//!    browser goes to its preview. Only the uploader sees it afterwards.
//! 3. `GET /admin/fees/{id}`: the rows, the problems (sheet and row), the
//!    skipped totals rows and hidden sheets, with Confirm (only when
//!    nothing blocks) and Cancel.
//! 4. `POST …/confirm` re-reads the kept file ([`Uploads::open`] refuses
//!    one whose hash changed), pins it for the Job ([`Uploads::confirm`]:
//!    kept a week instead of the rest of the 24 hours) and enqueues the Job
//!    `fees.import`, then shows the preview again, saying it is queued;
//!    `POST …/cancel` deletes it ([`Uploads::discard`]).
//! 5. The Job reads it once more ([`KeptUpload::open_confirmed`]), writes
//!    `fees_balance` and deletes it ([`KeptUpload::finish`]). A Job that
//!    finds no upload succeeds only if `fees_balance` holds its rows.
//!    An upload nobody confirms is deleted by the tick after 24 hours.
//! 6. `GET /admin/fees/{id}` after the Job: the result, read from
//!    `fees_import` and `fees_balance` (the kept upload is gone), with the
//!    next step "แจ้งนักศึกษา", a draft announcement to the Students whose
//!    balance comes from this file (#127).
//!
//! Where Students pay is two Module settings (#75), the bank and branch
//! [`BANK_NAME`] and the account number [`BANK_ACCOUNT`] (digits, `-` and
//! spaces only, so no hidden character can change what Students copy),
//! edited on `/admin/settings` only with [`BANK_ACCOUNT_EDIT`] (not with
//! `kernel.setting.edit` alone) and shown on the Student's Mini App page
//! [`student::PATH`]. The account has no default (#103): Students see
//! "not set" until Finance saves one (the demo seed saves a fictional one).

mod pages;
pub mod read;
mod result;
pub mod status;
pub mod student;
mod texts;

use axum::extract::{Path, State};
use axum::http::{StatusCode, header};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::{get, post};
use harpseal::audit::{self, Actor, RequestCtx, Target};
use harpseal::{
    AppState, CurrentStaff, Job, JobContext, JobSpec, KeptUpload, Module, ModuleRegistrar,
    Permission, Result, SettingKind, SheetUpload, Statement, UploadRejection, Uploads,
};
use maud::{Markup, html};

use read::Parsed;

/// The Module's name, which scopes its kept uploads.
pub const NAME: &str = "fees";
/// The Permission for every page of the Module.
pub const IMPORT: &str = "fees.import";
/// The Job that imports a confirmed file.
pub const JOB: &str = "fees.import";
/// The setting: the bank and branch Students pay at (fictional default).
pub const BANK_NAME: &str = "fees.bank_name";
/// The setting: the account number Students pay into (no default).
pub const BANK_ACCOUNT: &str = "fees.bank_account";
/// The only Permission that edits [`BANK_ACCOUNT`].
pub const BANK_ACCOUNT_EDIT: &str = "fees.bank_account.edit";
/// The pages, under `/admin`.
pub const BASE: &str = "/admin/fees";

/// The Module.
pub struct Fees;

impl Module for Fees {
    fn name(&self) -> &'static str {
        NAME
    }

    fn register(&self, r: &mut ModuleRegistrar) {
        r.permission(
            Permission::new(IMPORT, "Upload and import outstanding fees").thai("นำเข้ายอดค้างชำระ"),
        )
        .permission(
            Permission::new(
                BANK_ACCOUNT_EDIT,
                "Change the bank account Students pay fees into",
            )
            .thai("เปลี่ยนบัญชีธนาคารที่นักศึกษาใช้ชำระค่าธรรมเนียม"),
        )
        .messages(texts::TEXTS)
        .messages(student::TEXTS)
        .setting(
            BANK_NAME,
            SettingKind::Text,
            "ธนาคารทะเลใส สาขาอ่าวแมวน้ำ บัญชี มหาวิทยาลัยซีลเบย์",
            BANK_ACCOUNT_EDIT,
            "fees.bank_name_label",
            "fees.bank_name_help",
        )
        // No default (#103): no placeholder account for Students to pay
        // into; "not set" shows until Finance saves the real one.
        .setting_without_default(
            BANK_ACCOUNT,
            SettingKind::AccountNumber,
            BANK_ACCOUNT_EDIT,
            "fees.bank_account_label",
            "fees.bank_account_help",
        )
        .public(student::PATH, get(student::page))
        .mini_app_link("fees.mine", student::PATH, 60)
        .migration(
            "0001_fees_balance",
            include_str!("../migrations/fees/0001_fees_balance.sql"),
        )
        .migration(
            "0002_fees_import",
            include_str!("../migrations/fees/0002_fees_import.sql"),
        )
        .back_office_page(IMPORT, "/fees", "fees.title", 60, get(pages::page))
        .messages(status::TEXTS)
        .status_card(IMPORT, "fees.home.title", status::ORDER, status::card)
        .sheet_upload(IMPORT, pages::UPLOAD, post(upload))
        // The columns, once (#127): the help panel and the template follow.
        .sheet_columns(pages::UPLOAD, pages::COLUMNS)
        .back_office(IMPORT, "/fees/{id}", get(pages::preview))
        .back_office(IMPORT, "/fees/{id}/confirm", post(confirm))
        .back_office(IMPORT, "/fees/{id}/cancel", post(cancel))
        .job(JobSpec::new(JOB), run);
    }
}

pub(crate) fn t(s: &AppState, staff: &CurrentStaff, key: &str) -> String {
    s.catalog.t(&staff.locale, key)
}

pub(crate) fn render(
    s: &AppState,
    staff: &CurrentStaff,
    status: StatusCode,
    body: Markup,
) -> Response {
    let title = t(s, staff, "fees.title");
    let page = harpseal::ui::back_office(s, staff, &title, body);
    (status, Html(page.into_string())).into_response()
}

/// A short message on the Module's page, with `status`.
pub(crate) fn notice(
    s: &AppState,
    staff: &CurrentStaff,
    status: StatusCode,
    text: &str,
) -> Response {
    let body = html! {
        div class="flash err" role="status" { (text) }
        p { a class="btn secondary" href=(BASE) { (t(s, staff, "fees.title")) } }
    };
    render(s, staff, status, body)
}

fn see_other(to: &str) -> Response {
    (StatusCode::SEE_OTHER, [(header::LOCATION, to.to_string())]).into_response()
}

async fn upload(
    State(s): State<AppState>,
    staff: CurrentStaff,
    uploads: Uploads,
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
    match uploads.keep(up, &ctx).await {
        Ok(id) => see_other(&format!("{BASE}/{id}")),
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

/// The kept upload and what it holds, or the page to answer instead.
pub(crate) async fn open(
    s: &AppState,
    staff: &CurrentStaff,
    uploads: &Uploads,
    id: &str,
) -> std::result::Result<(KeptUpload, Parsed), Box<Response>> {
    let gone = |key: &str, status| Box::new(notice(s, staff, status, &t(s, staff, key)));
    match uploads.open(id).await {
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

async fn confirm(
    State(s): State<AppState>,
    staff: CurrentStaff,
    uploads: Uploads,
    ctx: RequestCtx,
    Path(id): Path<String>,
) -> Response {
    let (kept, p) = match open(&s, &staff, &uploads, &id).await {
        Ok(x) => x,
        Err(r) => return *r,
    };
    if !p.importable() {
        return see_other(&format!("{BASE}/{}", kept.id));
    }
    // Already confirmed (a second click): its preview shows the Job.
    if kept.confirmed_at.is_some() {
        return see_other(&format!("{BASE}/{}", kept.id));
    }
    // Pinned first, so the sweep cannot take it before the Job runs.
    match uploads.confirm(&kept.id, &ctx).await {
        Ok(true) => {}
        // Expired or confirmed since it was opened: no second Job.
        Ok(false) => {
            let text = t(&s, &staff, "fees.gone");
            return notice(&s, &staff, StatusCode::CONFLICT, &text);
        }
        Err(_) => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
    let payload = serde_json::json!({ "upload": kept.id });
    let key = format!("{JOB}:{}", kept.id);
    match s.jobs.enqueue_unique(&key, JOB, payload, None).await {
        Ok(Some(_)) => {}
        Ok(None) => return see_other(BASE),
        Err(_) => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
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
    // The result page, with the next step (#127).
    see_other(&format!("{BASE}/{}", kept.id))
}

/// Whether the import Job of upload `id` failed for good.
pub(crate) async fn job_failed(s: &AppState, id: &str) -> bool {
    let q = Statement::new("SELECT 1 FROM job WHERE unique_key = ? AND state = 'failed'")
        .bind(format!("{JOB}:{id}"));
    s.services.db.query(q).await.is_ok_and(|r| !r.is_empty())
}

async fn cancel(uploads: Uploads, ctx: RequestCtx, Path(id): Path<String>) -> Response {
    match uploads.discard(&id, &ctx).await {
        Ok(_) => see_other(BASE),
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

/// The Job: imports the confirmed file, recording the import
/// (`fees_import`) in the same batch as the balances, then deletes the
/// file. A retry that finds the record only deletes the file, so it never
/// writes older balances over a newer import's.
async fn run(cx: JobContext, job: Job) -> Result<()> {
    let invalid = |m: &str| harpseal::Error::Invalid(format!("fees import: {m}"));
    let payload: serde_json::Value =
        serde_json::from_str(&job.payload).map_err(|_| invalid("payload"))?;
    let id = payload["upload"]
        .as_str()
        .ok_or_else(|| invalid("payload"))?;
    let done = Statement::new("SELECT 1 FROM fees_import WHERE upload_id = ?").bind(id);
    if !cx.services.db.query(done).await?.is_empty() {
        KeptUpload::finish(&cx.services, NAME, id).await?;
        return Ok(());
    }
    let Some(kept) = KeptUpload::open_confirmed(&cx.services, NAME, id).await? else {
        // Not imported and the file is gone (expired, deleted): fail loudly.
        return Err(invalid(
            "the confirmed upload is gone and nothing was imported",
        ));
    };
    let book = kept
        .workbook()
        .map_err(|_| invalid("the file no longer reads"))?;
    let p = read::parse(&book);
    if !p.importable() {
        return Err(invalid("the file has problems"));
    }
    let now = cx.services.now_ms();
    let rows = i64::try_from(p.fees.len()).unwrap_or(i64::MAX);
    let record =
        Statement::new("INSERT INTO fees_import (upload_id, rows, imported_at) VALUES (?, ?, ?)")
            .bind(id)
            .bind(rows)
            .bind(now);
    let stmts: Vec<Statement> = p
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
        .chain(std::iter::once(record))
        .collect();
    cx.services.db.batch(stmts).await?;
    KeptUpload::finish(&cx.services, NAME, id).await?;
    Ok(())
}

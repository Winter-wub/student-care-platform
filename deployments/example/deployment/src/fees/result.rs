//! The result of a confirmed upload once its Job imported it (#127 review):
//! read from the Module's own tables, because the Job deletes the kept
//! upload ([`harpseal::KeptUpload::finish`]) and `Uploads::open` no longer
//! finds it. Only here is "แจ้งนักศึกษา" offered: the balances are written.

use axum::http::StatusCode;
use axum::response::Response;
use harpseal::{AppState, CurrentStaff, Statement};
use maud::html;

use super::{BASE, render, t};

/// An imported upload: its row count, when, how many balances still come
/// from it, and their student codes when a link can name them.
pub(super) struct Imported {
    rows: i64,
    at: i64,
    still: i64,
    codes: Vec<String>,
}

/// The import of upload `id`, if its Job wrote it and `staff` uploaded it:
/// the uploader rule of [`harpseal::Uploads::open`], which the kept upload
/// followed before the Job deleted it. Imports from before #127 name no
/// uploader and show to no one here.
pub(super) async fn imported(
    s: &AppState,
    staff: &CurrentStaff,
    id: &str,
) -> harpseal::Result<Option<Imported>> {
    let q = Statement::new(
        "SELECT rows, imported_at, \
         (SELECT COUNT(*) FROM fees_balance WHERE upload_id = fees_import.upload_id) AS still \
         FROM fees_import WHERE upload_id = ? AND staff_id = ?",
    )
    .bind(id)
    .bind(staff.id.as_str());
    let rows = s.services.db.query(q).await?;
    let int = |c: &str| rows.get(0, c).and_then(|v| v.as_i64());
    let (Some(n), Some(at), Some(still)) = (int("rows"), int("imported_at"), int("still")) else {
        return Ok(None);
    };
    // The codes only when the file still names its Students exactly and a
    // link could carry them (`ui::MAX_PREFILL`).
    let max = i64::try_from(harpseal::ui::MAX_PREFILL).unwrap_or(i64::MAX);
    let mut codes = Vec::new();
    if still == n && n <= max {
        let q = Statement::new(
            "SELECT student_code FROM fees_balance WHERE upload_id = ? \
             ORDER BY student_code LIMIT ?",
        )
        .bind(id)
        .bind(max + 1);
        let found = s.services.db.query(q).await?;
        codes = (0..found.len())
            .filter_map(|i| found.get(i, "student_code").and_then(|v| v.as_text()))
            .map(str::to_string)
            .collect();
    }
    Ok(Some(Imported {
        rows: n,
        at,
        still,
        codes,
    }))
}

/// The result page, with the next step. When a later import replaced
/// some of these balances, the file no longer names its Students exactly:
/// the draft leaves the Audience to Staff.
pub(super) fn page(s: &AppState, staff: &CurrentStaff, imp: &Imported) -> Response {
    let exact = imp.still == imp.rows;
    let named: &[String] = &imp.codes;
    let n = harpseal::ui::number(imp.rows);
    let when = harpseal::ui::local_datetime(s, imp.at);
    let fact = s.catalog.tf(
        &staff.locale,
        "fees.imported",
        &[("n", &n), ("time", &when)],
    );
    let back = t(s, staff, "fees.back_to_list");
    let body = html! {
        h1 { (t(s, staff, "fees.imported_title")) }
        div class="flash ok" role="status" id="imported" { (fact) }
        @if !exact { p class="muted" id="superseded" { (t(s, staff, "fees.superseded")) } }
        (harpseal::ui::next_step(s, staff, named, (&back, BASE)))
    };
    render(s, staff, StatusCode::OK, body)
}

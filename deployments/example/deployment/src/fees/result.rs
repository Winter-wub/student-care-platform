//! The result of a confirmed upload once its Job imported it (#127 review):
//! read from the Module's own tables, because the Job deletes the kept
//! upload ([`harpseal::KeptUpload::finish`]) and `Uploads::open` no longer
//! finds it. Only here is "แจ้งนักศึกษา" offered: the balances are written.

use axum::http::StatusCode;
use axum::response::Response;
use harpseal::{AppState, CurrentStaff, Statement};
use maud::html;

use super::{BASE, render, t};

/// An imported upload: its row count, when, and the student codes whose
/// balance still comes from it.
pub(super) struct Imported {
    rows: i64,
    at: i64,
    codes: Vec<String>,
}

/// The import of upload `id`, if its Job wrote it.
pub(super) async fn imported(s: &AppState, id: &str) -> harpseal::Result<Option<Imported>> {
    let q =
        Statement::new("SELECT rows, imported_at FROM fees_import WHERE upload_id = ?").bind(id);
    let rows = s.services.db.query(q).await?;
    let int = |c: &str| rows.get(0, c).and_then(|v| v.as_i64());
    let (Some(n), Some(at)) = (int("rows"), int("imported_at")) else {
        return Ok(None);
    };
    let q = Statement::new(
        "SELECT student_code FROM fees_balance WHERE upload_id = ? ORDER BY student_code",
    )
    .bind(id);
    let found = s.services.db.query(q).await?;
    let codes = (0..found.len())
        .filter_map(|i| found.get(i, "student_code").and_then(|v| v.as_text()))
        .map(str::to_string)
        .collect();
    Ok(Some(Imported { rows: n, at, codes }))
}

/// The result page, with the next step. When a later import replaced
/// some of these balances, the file no longer names its Students exactly:
/// the draft leaves the Audience to Staff.
pub(super) fn page(s: &AppState, staff: &CurrentStaff, imp: &Imported) -> Response {
    let exact = i64::try_from(imp.codes.len()).is_ok_and(|n| n == imp.rows);
    let named: &[String] = if exact { &imp.codes } else { &[] };
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

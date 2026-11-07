//! The problems of an import (#8, #71): the table of a refused file and of
//! the warnings of a file that imports, the count per class, and the
//! downloadable `report.csv` that lists both.

use std::collections::BTreeMap;

use axum::extract::{Path, State};
use axum::response::Response;
use maud::{Markup, html};

use super::store::{self, Import};
use super::validate::Problem;
use super::view::{BASE, View, csv_body, download};
use crate::{AppState, CurrentStaff};

/// Problems shown on a page; the CSV has all that were kept.
const SHOWN: usize = 50;

/// Whether the report names sheets (an XLSX's problems do; a CSV's,
/// and reports stored before #70, do not, and keep their three columns).
pub(super) fn has_sheets(problems: &[Problem]) -> bool {
    problems.iter().any(|p| !p.sheet.is_empty())
}

/// Whether any problem is a warning (reports stored before #71 have none).
fn has_warnings(problems: &[Problem]) -> bool {
    problems.iter().any(|p| !p.severity.is_blocking())
}

/// The column, with the cell when one is at fault: `เกรดเฉลี่ย (H2)`.
fn column(p: &Problem) -> String {
    if p.cell.is_empty() {
        p.column.clone()
    } else {
        format!("{} ({})", p.column, p.cell)
    }
}

/// The problems of `imp` the report keeps and how many there were: a
/// refused file's (warnings included), else the warnings of a file that
/// imports.
fn listed(imp: &Import) -> (&[Problem], u64) {
    match &imp.failure {
        Some(f) if !f.problems.is_empty() => (&f.problems, f.total),
        _ => (&imp.counts.warnings.problems, imp.counts.warnings.total),
    }
}

/// How many problems each class has, as a list.
pub(super) fn classes(v: &View<'_>, classes: &BTreeMap<String, u64>) -> Markup {
    html! {
        h4 { (v.t("kernel.roster.by_class")) }
        ul class="classes" {
            @for (class, n) in classes {
                li data-class=(class) data-count=(n) {
                    (v.t(&format!("kernel.roster.reason_{class}"))) ": " strong { (n) }
                }
            }
        }
    }
}

/// The first problems as a table, with the sheet and the level when the
/// problems have them.
pub(super) fn table(v: &View<'_>, problems: &[Problem]) -> Markup {
    let (sheets, levels) = (has_sheets(problems), has_warnings(problems));
    html! {
        table class="rows" {
            thead { tr {
                @if sheets { th { (v.t("kernel.roster.report_sheet")) } }
                th { (v.t("kernel.roster.report_row")) }
                th { (v.t("kernel.roster.report_column")) }
                th { (v.t("kernel.roster.report_reason")) }
                @if levels { th { (v.t("kernel.roster.report_severity")) } }
            } }
            tbody {
                @for p in problems.iter().take(SHOWN) {
                    tr data-severity=(if p.severity.is_blocking() { "blocking" } else { "warning" }) {
                        @if sheets {
                            td data-label=(v.t("kernel.roster.report_sheet")) { (p.sheet) }
                        }
                        td data-label=(v.t("kernel.roster.report_row")) { (p.row) }
                        td data-label=(v.t("kernel.roster.report_column")) { (column(p)) }
                        td data-label=(v.t("kernel.roster.report_reason")) {
                            (v.t(&format!("kernel.roster.reason_{}", p.reason)))
                        }
                        @if levels {
                            td data-label=(v.t("kernel.roster.report_severity")) {
                                (v.t(p.severity.key()))
                            }
                        }
                    }
                }
            }
        }
    }
}

/// The warnings of a previewed (or imported) file; nothing when none.
pub(super) fn warnings_card(v: &View<'_>, imp: &Import) -> Markup {
    let w = &imp.counts.warnings;
    if w.is_empty() {
        return html! {};
    }
    html! {
        div class="card" id="roster-warnings" data-warnings=(w.total) {
            h3 { (v.t("kernel.roster.warnings")) " " strong data-count="warnings" { (w.total) } }
            p class="muted" { (v.t("kernel.roster.warnings_help")) }
            (classes(v, &w.classes))
            (table(v, &w.problems))
            @if w.problems.len().min(SHOWN) < w.total as usize {
                p class="muted" {
                    (v.t("kernel.roster.warnings_shown")
                        .replace("{shown}", &w.problems.len().min(SHOWN).to_string())
                        .replace("{total}", &w.total.to_string()))
                }
            }
            p { a class="btn secondary" href=(format!("{BASE}/{}/report.csv", imp.id)) {
                (v.t("kernel.roster.warnings_report"))
            } }
        }
    }
}

/// The report as CSV (row, column, problem in the Staff member's Locale,
/// and the level when there are warnings), read from `roster_import.error`
/// or, for a file that imports, `counts.warnings`.
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
    let (problems, total) = listed(&imp);
    let skipped = &imp.counts.skipped;
    let sheets = has_sheets(problems) || skipped.iter().any(|s| !s.sheet.is_empty());
    let levels = has_warnings(problems);
    let mut head = vec![
        v.t("kernel.roster.report_row"),
        v.t("kernel.roster.report_column"),
        v.t("kernel.roster.report_reason"),
    ];
    if levels {
        head.push(v.t("kernel.roster.report_severity"));
    }
    if sheets {
        head.insert(0, v.t("kernel.roster.report_sheet"));
    }
    let head: Vec<&str> = head.iter().map(String::as_str).collect();
    let mut lines = vec![super::csv::line(&head)];
    for p in problems {
        let row = p.row.to_string();
        let reason = v.t(&format!("kernel.roster.reason_{}", p.reason));
        let column = column(p);
        let level = v.t(p.severity.key());
        let mut fields = vec![row.as_str(), column.as_str(), reason.as_str()];
        if levels {
            fields.push(level.as_str());
        }
        if sheets {
            fields.insert(0, &p.sheet);
        }
        lines.push(super::csv::line(&fields));
    }
    for f in super::skipped::report_fields(&v, skipped, sheets) {
        lines.push(super::csv::line(
            &f.iter().map(String::as_str).collect::<Vec<_>>(),
        ));
    }
    if (problems.len() as u64) < total {
        let note = v
            .t("kernel.roster.report_truncated")
            .replace("{shown}", &problems.len().to_string())
            .replace("{total}", &total.to_string());
        lines.push(super::csv::line(&[&note]));
    }
    let name = format!("roster-{id}-report.csv");
    download("text/csv; charset=utf-8", &name, &name, csv_body(lines))
}

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

fn has_sheets_in(problems: &[&Problem]) -> bool {
    problems.iter().any(|p| !p.sheet.is_empty())
}

/// Whether any problem is a warning (reports stored before #71 have none).
fn has_warnings(problems: &[Problem]) -> bool {
    problems.iter().any(|p| !p.severity.is_blocking())
}

/// Reasons that are about the header row: the page lists the headers the
/// template expects under them (#125).
const HEADER_REASONS: [&str; 3] = ["missing_header", "unexpected_header", "no_header_row"];

/// The row as Staff read it: a problem with the whole file or sheet has no
/// row (stored as 0), so it says "ทั้งไฟล์" or "ทั้งชีต" rather than "แถว 0"
/// (#125).
/// `grouped` writes the page's `1,234`; the CSV report keeps the digits
/// (`1234`) so a spreadsheet reads them as a number.
fn row(v: &View<'_>, p: &Problem, grouped: bool) -> String {
    if p.row == 0 && !p.sheet.is_empty() {
        v.t("kernel.roster.report_whole_sheet")
    } else if p.row == 0 {
        v.t("kernel.roster.report_whole_file")
    } else if grouped {
        crate::ui::number(p.row)
    } else {
        p.row.to_string()
    }
}

/// The headers the template expects, as a sentence, when any problem is
/// about the header row; else nothing.
pub(super) fn expected(v: &View<'_>, problems: &[Problem]) -> Markup {
    if !problems
        .iter()
        .any(|p| HEADER_REASONS.contains(&p.reason.as_str()))
    {
        return html! {};
    }
    let headers = super::file::expected_headers(&v.s.config.roster).join(", ");
    html! {
        div class="card" id="expected-headers" {
            p { (v.t("kernel.roster.expected_headers")) }
            p { strong { (headers) } }
            p class="muted" { (v.t("kernel.roster.expected_help")) " "
                a href=(format!("{BASE}/template.csv")) { (v.t("kernel.roster.template")) } }
            // The upload page's help (#127), so the fix is on this page.
            (super::page_help::panel(v, false))
        }
    }
}

/// The column, with the cell when one is at fault: `เกรดเฉลี่ย (H2)`.
fn column(p: &Problem) -> String {
    if p.cell.is_empty() {
        p.column.clone()
    } else {
        format!("{} ({})", p.column, p.cell)
    }
}

/// What the cell of an out-of-range attribute, or an off-list one with
/// labels, may hold: the range, or each value with its label (#81). Read from the current
/// config by column, so it is the same in the page and in `report.csv`.
fn what_fits(v: &View<'_>, p: &Problem) -> Option<String> {
    let roster = &v.s.config.roster;
    let spec = roster.attributes.values().find(|a| a.column == p.column)?;
    match p.reason.as_str() {
        "out_of_range" => spec.range_text(),
        // Without labels the report reads as it always did.
        "not_in_list" if !spec.value_labels.is_empty() => {
            // The default Locale's labels: the ones the import reads.
            let default = &v.s.config.default_locale;
            let all: Vec<String> = spec
                .values
                .iter()
                .map(|x| spec.labelled(x, default, default))
                .collect();
            Some(all.join(", "))
        }
        _ => None,
    }
}

/// The problems of `imp` the report keeps and how many there were: the
/// blocking problems of a refused file, then its warnings (or those of a
/// file that imports).
fn listed(imp: &Import) -> (Vec<&Problem>, u64) {
    let (blocking, total) = imp
        .failure
        .as_ref()
        .map_or((&[][..], 0), |f| (f.problems.as_slice(), f.total));
    let w = &imp.counts.warnings;
    (
        blocking.iter().chain(&w.problems).collect(),
        total + w.total,
    )
}

/// The reason in the Staff member's Locale; a problem standing for several
/// cells says how many and which rows come first. A header problem names
/// the headers expected, so the CSV report says it too (#125).
fn reason(v: &View<'_>, p: &Problem) -> String {
    let text = v.t(&format!("kernel.roster.reason_{}", p.reason));
    let text = if p.reason == "no_header_row" {
        let headers = super::file::expected_headers(&v.s.config.roster).join(", ");
        format!(
            "{text} ({}: {headers})",
            v.t("kernel.roster.expected_short")
        )
    } else {
        text
    };
    let text = match what_fits(v, p) {
        Some(fits) => format!("{text}: {fits}"),
        None => text,
    };
    if p.count < 2 {
        return text;
    }
    let rows = p
        .rows
        .iter()
        .map(usize::to_string)
        .collect::<Vec<_>>()
        .join(", ");
    let more = if (p.rows.len() as u64) < p.count {
        ", …"
    } else {
        ""
    };
    let of = v
        .t("kernel.roster.rows_of")
        .replace("{count}", &p.count.to_string())
        .replace("{rows}", &format!("{rows}{more}"));
    format!("{text} ({of})")
}

/// How many problems each class has, as a list.
pub(super) fn classes(v: &View<'_>, classes: &BTreeMap<String, u64>) -> Markup {
    html! {
        h3 { (v.t("kernel.roster.by_class")) }
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
                        td data-label=(v.t("kernel.roster.report_row")) { (row(v, p, true)) }
                        td data-label=(v.t("kernel.roster.report_column")) { (column(p)) }
                        td data-label=(v.t("kernel.roster.report_reason")) {
                            (reason(v, p))
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
            h2 { (v.t("kernel.roster.warnings")) " " strong data-count="warnings" { (crate::ui::number(w.total)) } }
            p class="muted" { (v.t("kernel.roster.warnings_help")) }
            (classes(v, &w.classes))
            (table(v, &w.problems))
            @let shown: u64 = w.problems.iter().take(SHOWN).map(Problem::weight).sum();
            @if shown < w.total {
                p class="muted" {
                    (v.t("kernel.roster.warnings_shown")
                        .replace("{shown}", &shown.to_string())
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
    let sheets = has_sheets_in(&problems) || skipped.iter().any(|s| !s.sheet.is_empty());
    let levels = problems.iter().any(|p| !p.severity.is_blocking());
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
    for p in &problems {
        let row = row(&v, p, false);
        let reason = reason(&v, p);
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
    let shown: u64 = problems.iter().map(|p| p.weight()).sum();
    if shown < total {
        let note = v
            .t("kernel.roster.report_truncated")
            .replace("{shown}", &shown.to_string())
            .replace("{total}", &total.to_string());
        lines.push(super::csv::line(&[&note]));
    }
    let name = format!("roster-{id}-report.csv");
    download("text/csv; charset=utf-8", &name, &name, csv_body(lines))
}

//! Rows and sheets an import left out (#70): footer rows below the data
//! and hidden sheets ([`Skipped`]), listed on the import page and in
//! `report.csv` so nothing is dropped unseen; and the may-be-empty
//! columns a file leaves out (#154, [`Absent`]), on the preview.

use maud::{Markup, html};

use super::file::{Absent, Skipped};
use super::view::View;

/// Skipped rows listed on the page; the report has all.
const SHOWN: usize = 50;

/// The "skipped, not imported" card; nothing when none were skipped.
pub(super) fn card(v: &View<'_>, skipped: &[Skipped]) -> Markup {
    if skipped.is_empty() {
        return html! {};
    }
    html! {
        div class="card" data-skipped=(skipped.len()) {
            h2 { (v.t("kernel.roster.skipped")) }
            ul {
                @for (first, last, s) in ranges(skipped).into_iter().take(SHOWN) {
                    li {
                        (v.t(&format!("kernel.roster.skip_{}", s.reason)))
                        @if !s.sheet.is_empty() { " (" (s.sheet) ")" }
                        @if first > 0 {
                            " " (v.t("kernel.roster.report_row")) " "
                            @if first == last { (first) } @else { (first) "–" (last) }
                        }
                    }
                }
            }
            ul class="muted" {
                @for s in skipped.iter().filter(|s| !s.text.is_empty()).take(SHOWN) {
                    li {
                        @if !s.sheet.is_empty() { (s.sheet) " · " }
                        (v.t("kernel.roster.report_row")) " " (s.row) ": " (s.text)
                    }
                }
            }
        }
    }
}

/// The "columns not in the file" card (#154): each may-be-empty column a
/// sheet leaves out, in template order, with the sheets that leave it out
/// (none named for a CSV, or when every sheet does), and what happens to
/// it. Nothing when every column is there.
pub(super) fn absent_card(v: &View<'_>, absent: &[Absent]) -> Markup {
    if absent.is_empty() {
        return html! {};
    }
    let order = super::file::expected_headers(&v.s.config.roster);
    let all_sheets = absent.iter().all(|a| a.sheet.is_empty());
    let mut columns: Vec<(&str, Vec<&str>)> = Vec::new();
    for h in &order {
        let sheets: Vec<&str> = absent
            .iter()
            .filter(|a| a.columns.contains(h))
            .map(|a| a.sheet.as_str())
            .filter(|s| !s.is_empty())
            .collect();
        if absent.iter().any(|a| a.columns.contains(h)) {
            columns.push((h, if all_sheets { Vec::new() } else { sheets }));
        }
    }
    html! {
        section class="card" id="roster-absent" data-absent=(columns.len()) aria-labelledby="roster-absent-title" {
            h2 id="roster-absent-title" { (v.t("kernel.roster.absent_title")) }
            p class="muted" { (v.t("kernel.roster.absent_help")) }
            ul class="list header-list" {
                @for (h, sheets) in &columns {
                    li data-header="absent" {
                        span class="name code-head" { (h) }
                        @if !sheets.is_empty() { " (" (sheets.join(", ")) ")" }
                    }
                }
            }
        }
    }
}

/// Report lines, one per skipped row or sheet: `[sheet,] row, text,
/// reason` under the report's columns.
pub(super) fn report_fields<'a>(
    v: &View<'_>,
    skipped: &'a [Skipped],
    sheets: bool,
) -> Vec<Vec<String>> {
    skipped
        .iter()
        .map(|s: &'a Skipped| {
            // A whole sheet has no row: "ทั้งชีต", never "0" (#125).
            let row = if s.row == 0 {
                v.t("kernel.roster.report_whole_sheet")
            } else {
                s.row.to_string()
            };
            let mut f = vec![
                row,
                s.text.clone(),
                v.t(&format!("kernel.roster.skip_{}", s.reason)),
            ];
            if sheets {
                f.insert(0, s.sheet.clone());
            }
            f
        })
        .collect()
}

/// Consecutive rows of one sheet and reason as `(first, last, entry)`.
fn ranges(skipped: &[Skipped]) -> Vec<(usize, usize, &Skipped)> {
    let mut out: Vec<(usize, usize, &Skipped)> = Vec::new();
    for s in skipped {
        match out.last_mut() {
            Some((_, last, e))
                if e.sheet == s.sheet && e.reason == s.reason && s.row == *last + 1 =>
            {
                *last = s.row;
            }
            _ => out.push((s.row, s.row, s)),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(sheet: &str, row: usize) -> Skipped {
        Skipped {
            sheet: sheet.into(),
            row,
            reason: "footer".into(),
            text: String::new(),
        }
    }

    #[test]
    fn consecutive_rows_of_a_sheet_make_one_range() {
        let list = [s("ปี 1", 12), s("ปี 1", 13), s("ปี 2", 14), s("ปี 2", 16)];
        let got: Vec<_> = ranges(&list)
            .into_iter()
            .map(|(a, b, e)| (a, b, e.sheet.as_str()))
            .collect();
        assert_eq!(got, vec![(12, 13, "ปี 1"), (14, 14, "ปี 2"), (16, 16, "ปี 2")]);
    }
}

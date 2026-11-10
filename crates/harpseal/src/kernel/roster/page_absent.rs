//! The "columns not in the file" card of the preview (#154): each
//! may-be-empty column a sheet leaves out, in template order, with the
//! sheets that leave it out (none named for a CSV, or when every sheet
//! does), and what happens to it. In a `full` import it is set in caution
//! and says how to clear a value instead; a left-out Tags column says the
//! roster Tags (and the Audiences that select them) stay; a left-out email
//! column counts the Students it leaves without one.

use maud::{Markup, html};

use super::file::Absent;
use super::store::Import;
use super::view::View;
use crate::ui::number;

/// The card; nothing when every column is there.
pub(super) fn card(v: &View<'_>, imp: &Import) -> Markup {
    let absent = &imp.counts.absent;
    if absent.is_empty() {
        return html! {};
    }
    let roster = &v.s.config.roster;
    let order = super::file::expected_headers(roster);
    let columns = columns(&order, absent);
    let gone = |h: &str| columns.iter().any(|(c, _)| *c == h);
    let tags = roster.tags.as_ref().is_some_and(|t| gone(&t.column));
    let email = gone(&roster.email);
    let full = imp.mode == "full";
    let no_email = imp.counts.preview.no_email_absent;
    html! {
        section class=(if full { "card absent-full" } else { "card" }) id="roster-absent"
            data-absent=(columns.len()) aria-labelledby="roster-absent-title" {
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
            @if tags { p { (v.t("kernel.roster.absent_tags")) } }
            @if email && no_email > 0 {
                p class="note warn-note" data-count="no_email_absent" {
                    (v.t("kernel.roster.absent_no_email").replace("{n}", &number(no_email)))
                }
            }
            @if full {
                p class="note warn-note" data-absent-full { (v.t("kernel.roster.absent_full")) }
            }
        }
    }
}

/// Each left-out column in `order`, with the sheets that leave it out
/// (none when no sheet is named).
fn columns<'a>(order: &'a [String], absent: &'a [Absent]) -> Vec<(&'a str, Vec<&'a str>)> {
    let all_sheets = absent.iter().all(|a| a.sheet.is_empty());
    order
        .iter()
        .filter(|h| absent.iter().any(|a| a.columns.contains(h)))
        .map(|h| {
            let sheets = absent
                .iter()
                .filter(|a| a.columns.contains(h) && !a.sheet.is_empty())
                .map(|a| a.sheet.as_str())
                .collect();
            (h.as_str(), if all_sheets { Vec::new() } else { sheets })
        })
        .collect()
}

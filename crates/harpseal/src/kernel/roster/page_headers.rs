//! The header error of a refused file (#134): the problem stated once,
//! the columns found in the file beside the ones expected (required and
//! may-be-empty apart), each marked as matching, missing or unknown, and
//! an upload form on the same page. A failure stored before #134 (no
//! header row kept) keeps the problem table and the expected headers.

use std::collections::BTreeSet;

use maud::{Markup, html};

use super::file::FoundHeaders;
use super::store::{Failure, Import};
use super::view::{BASE, IMPORT, View};

/// Whether this page draws the header error for `f`: every problem is
/// about the header row, and the file's header rows were kept (or no row
/// was found at all).
pub(super) fn applies(f: &Failure) -> bool {
    let header = |r: &str| matches!(r, "missing_header" | "unexpected_header" | "no_header_row");
    !f.problems.is_empty()
        && f.problems.iter().all(|p| header(&p.reason))
        && (!f.headers.is_empty() || f.problems.iter().all(|p| p.reason == "no_header_row"))
}

/// The whole failed page for a header error.
pub(super) fn page(v: &View<'_>, imp: &Import, f: &Failure) -> Markup {
    // A column is missing unless every header row found holds it.
    let present =
        |h: &str| !f.headers.is_empty() && f.headers.iter().all(|s| s.found.iter().any(|x| x == h));
    let help = super::page_help::columns(v, &v.s.config.roster);
    let reupload = v.staff.can(IMPORT) && super::has_mapping(&v.s.config.roster);
    let expected: Vec<&str> = help.iter().map(|c| c.header.as_str()).collect();
    html! {
        // The problem, once (#129: the flash announces after the upload).
        div class="warn" role=[v.alert()] { (v.t("kernel.roster.headers_title")) }
        section class="card" id="expected-headers" aria-labelledby="roster-headers-title" {
            h2 id="roster-headers-title" { (v.t("kernel.roster.expected")) }
            p class="muted" { (v.t("kernel.roster.headers_help")) }
            div class="cols header-compare" {
                div {
                    @if f.headers.is_empty() {
                        h3 { (v.t("kernel.roster.headers_found")) }
                        p { (v.t("kernel.roster.no_header_found")) }
                    }
                    @for sheet in &f.headers { (found(v, sheet, &expected)) }
                }
                div {
                    @for (required, key) in [(true, "kernel.roster.headers_required"), (false, "kernel.roster.headers_optional")] {
                        @let cols: Vec<_> = help.iter().filter(|c| c.required == required).collect();
                        @if !cols.is_empty() {
                            h3 { (v.t(key)) }
                            ul class="list header-list" {
                                @for c in cols {
                                    @let gone = !present(&c.header);
                                    li data-header=(if gone { "missing" } else { "ok" }) {
                                        span class="name code-head" { (c.header) }
                                        @if gone {
                                            span class="chip err" { (v.t("kernel.roster.header_missing")) }
                                        } @else {
                                            span class="chip ok" { (v.t("kernel.roster.header_ok")) }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
            // The template, unless the upload help below offers it.
            @if !reupload {
                p { a class="btn secondary" href=(format!("{BASE}/template.csv")) download {
                    (v.t("kernel.upload.template"))
                } }
            }
        }
        // A new upload on the same page, with its help beside it (#127).
        @if reupload {
            div class="cols upload-pair" id="roster-reupload" {
                (super::page_list::upload_form(v))
                aside class="card" id="upload-help" aria-label=(v.t("kernel.upload.help_title")) {
                    (super::page_help::panel(v, true))
                }
            }
        }
        (super::skipped::card(v, &imp.counts.skipped))
        (super::page_problems::warnings_card(v, imp))
    }
}

/// One sheet's header row as found, each header marked as matching or
/// unknown (a repeat counts as unknown, as the reader does).
fn found(v: &View<'_>, sheet: &FoundHeaders, expected: &[&str]) -> Markup {
    let title = if sheet.sheet.is_empty() {
        v.t("kernel.roster.headers_found")
    } else {
        v.t("kernel.roster.headers_found_sheet")
            .replace("{sheet}", &sheet.sheet)
    };
    let mut seen = BTreeSet::new();
    html! {
        h3 { (title) }
        ul class="list header-list" {
            @for h in &sheet.found {
                @let known = expected.contains(&h.as_str()) && seen.insert(h.as_str());
                li data-header=(if known { "ok" } else { "unknown" }) {
                    span class="name code-head" { (h) }
                    @if known {
                        span class="chip ok" { (v.t("kernel.roster.header_ok")) }
                    } @else {
                        span class="chip err" { (v.t("kernel.roster.header_unknown")) }
                    }
                }
            }
        }
    }
}

//! The preview of a `previewed` import (#24, #134): it leads with what
//! changes. New, updated and leaving stand as three large figures under
//! one heading (leaving in Refused Ink when anyone leaves, with the chat
//! Links it cuts beside it); the other counts follow on one quieter line.
//! A warned preview opens with a specific alarm ("48 จาก 50 คน (96%) จะพ้น
//! สภาพ…"), which the typed confirmation names with `aria-describedby`.
//! Then the per-Student lists (`page_changes`), the warnings and the
//! confirm and cancel buttons.

use maud::{Markup, html};

use super::page_problems as problems;
use super::reach;
use super::store::{Import, Preview};
use super::view::{BASE, IMPORT, View};
use crate::ui::number;

/// The id of the alarm, which the typed field names (#134).
pub(super) const ALARM_ID: &str = "roster-alarm";
/// The id of the leaving figure.
pub(super) const LEAVING_ID: &str = "roster-leaving";

pub(super) fn preview(v: &View<'_>, imp: &Import, wrong: bool) -> Markup {
    let p = &imp.counts.preview;
    // One alert (#129): a mode no longer allowed blocks the import, so it
    // is the one announced, not the leave warning.
    let allowed = v.s.config.roster.allows_mode(&imp.mode);
    html! {
        @if p.warning {
            div class="warn" id=(ALARM_ID) role=[v.alert().filter(|_| allowed)] { (alarm(v, imp)) }
        }
        (figures(v, imp))
        (super::page_changes::leaving(v, imp))
        (super::skipped::card(v, &imp.counts.skipped))
        (problems::warnings_card(v, imp))
        @if v.staff.can(IMPORT) {
            @if allowed { (reach::typed_field(v, imp, wrong)) }
            div class="actions" {
                @if allowed {
                    (reach::confirm_form(v, imp))
                } @else {
                    div class="warn" role=[v.alert()] { (v.t("kernel.roster.mode_no_longer")) }
                }
                form method="post" action=(format!("{BASE}/{}/cancel", imp.id)) {
                    (v.staff.csrf_field())
                    button class="secondary" type="submit" { (v.t("kernel.roster.cancel")) }
                }
            }
        }
    }
}

/// The alarm in numbers: how many of the active Students leave and the
/// Links that go with them, else how much shorter the file is.
fn alarm(v: &View<'_>, imp: &Import) -> String {
    let p = &imp.counts.preview;
    if !p.detailed {
        // A preview stored before #134 has no Link count.
        return v.t("kernel.roster.warning");
    }
    let left = reach::leaving(imp);
    if left * 5 > p.active_before {
        return v
            .t("kernel.roster.alarm_left")
            .replace("{left}", &number(left))
            .replace("{active}", &number(p.active_before))
            .replace("{pct}", &percent(left, p.active_before))
            .replace("{links}", &number(p.links_cut));
    }
    v.t("kernel.roster.alarm_short")
        .replace("{rows}", &number(imp.total_rows))
        .replace("{active}", &number(p.active_before))
}

/// `part` of `whole` as a whole percentage, rounded down so 199 of 200
/// never reads "100".
fn percent(part: u64, whole: u64) -> String {
    (part.saturating_mul(100) / whole.max(1)).to_string()
}

/// The three figures, the quieter counts and the change list.
fn figures(v: &View<'_>, imp: &Import) -> Markup {
    let p = &imp.counts.preview;
    let full = imp.mode == "full";
    let left = reach::leaving(imp);
    html! {
        section class="card" aria-labelledby="roster-what" {
            h2 id="roster-what" { (v.t("kernel.roster.what_changes")) }
            dl class="figures" {
                div class="figure" data-figure="new" {
                    dt { (v.t("kernel.roster.new")) } dd { (number(p.tally.new)) }
                }
                div class="figure" data-figure="updated" {
                    dt { (v.t("kernel.roster.updated")) } dd { (number(p.tally.updated)) }
                }
                div class=(if left > 0 { "figure leaving" } else { "figure" }) id=(LEAVING_ID) data-figure="leaving" {
                    dt { (v.t("kernel.roster.will_become_left")) }
                    @if full {
                        dd data-count="will_become_left" { (number(p.will_become_left)) }
                        @if p.detailed && left > 0 {
                            dd class="figure-note" data-count="links_cut" {
                                (v.t("kernel.roster.links_cut").replace("{n}", &number(p.links_cut)))
                            }
                        }
                    } @else {
                        dd { (number(0)) }
                        dd class="figure-note" { (v.t("kernel.roster.no_leaving_upsert")) }
                    }
                }
            }
            (secondary(v, imp, p))
            (super::page_changes::changes(v, imp))
        }
    }
}

/// Unchanged and the other counts, on one quieter line.
fn secondary(v: &View<'_>, imp: &Import, p: &Preview) -> Markup {
    let mut items = vec![
        ("unchanged", "kernel.roster.unchanged", p.tally.unchanged),
        ("returning", "kernel.roster.returning", p.tally.returning),
        (
            "skipped_by_override",
            "kernel.roster.skipped_by_override",
            p.tally.skipped_by_override,
        ),
    ];
    if imp.mode == "full" {
        items.push((
            "kept_by_status",
            "kernel.roster.kept_by_status",
            p.kept_by_status,
        ));
    }
    let total = u64::try_from(imp.total_rows).unwrap_or(0);
    items.push(("total", "kernel.roster.total", total));
    items.push((
        "active_before",
        "kernel.roster.active_before",
        p.active_before,
    ));
    html! {
        ul class="counts-line" {
            @for (name, key, n) in items {
                li { (v.t(key)) " " strong data-count=(name) { (number(n)) } }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::percent;

    #[test]
    fn a_share_rounds_down_and_never_divides_by_zero() {
        assert_eq!(percent(48, 50), "96");
        assert_eq!(percent(199, 200), "99");
        assert_eq!(percent(0, 0), "0");
    }
}

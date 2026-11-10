//! The edit form's "notify recipients of the edit" box (#77) and what it
//! costs: the recipients on Channels that cannot edit (at most: only those
//! whose text changes are sent to), this month's metered Pushes against
//! the Quota, and a warning when the notice would go over what is left.
//! The Quota warns, it never blocks: the box stays usable.

use maud::{Markup, html};

use super::form::Fields;
use super::page_count::month_used;
use super::store::Row;
use super::view::View;
use crate::kernel::students::channel_label;
use crate::kernel::text::fill;
use crate::kernel::usage::{self, MonthSummary};
use crate::{Deployment, Result};

/// Recipients per Channel and this month's usage of those Channels.
#[derive(Debug, Default)]
pub(super) struct Cost {
    per_channel: Vec<(String, i64)>,
    used: Vec<(String, MonthSummary)>,
}

/// The cost of a notice for `row`'s next version (a sent one only).
pub(super) async fn cost(d: &Deployment, row: &Row) -> Result<Option<Cost>> {
    if row.status != "sent" || !super::notice::open(row, d.services().now_ms()) {
        return Ok(None);
    }
    let per_channel = super::notice::reach(d, &row.id, row.version + 1, None).await?;
    let kinds: Vec<String> = per_channel.iter().map(|(k, _)| k.clone()).collect();
    let used = usage::month_summaries(d, &kinds).await?;
    Ok(Some(Cost { per_channel, used }))
}

/// The box, its help, the cost per Channel and any over-Quota warning.
pub(super) fn field(v: &View<'_>, f: &Fields, cost: Option<&Cost>, err: Markup) -> Markup {
    html! {
        div id="notify-field" {
            (err)
            label class="check" {
                input type="checkbox" name="notify" value="1" checked[f.notify];
                " " (v.t("announcements.form.notify"))
            }
            p class="muted" { (v.t("announcements.form.notify_help")) }
            @if let Some(c) = cost { (cost_lines(v, c)) }
        }
    }
}

fn cost_lines(v: &View<'_>, c: &Cost) -> Markup {
    html! {
        ul class="list" id="notify-cost" aria-live="polite" {
            @for (kind, n) in &c.per_channel {
                @let count = crate::ui::number(*n);
                @let month = c.used.iter().find(|(k, _)| k == kind).map(|(_, m)| m);
                li {
                    (channel_label(kind)) ": "
                    (fill(&v.t("announcements.form.notify_reach"), &[("count", &count)]))
                    @if let Some(m) = month {
                        " · " span class="month-used" { (month_used(v, m)) }
                    }
                    @if let Some(left) = month.and_then(|m| over(m, *n)) {
                        // Inside the polite `#notify-cost`: no alert of its own (#129).
                        p class="field-error" {
                            (fill(&v.t("announcements.form.notify_over"), &[("left", &crate::ui::number(left))]))
                        }
                    }
                }
            }
        }
    }
}

/// What is left of the Quota when `n` Pushes would go over it.
fn over(m: &MonthSummary, n: i64) -> Option<i64> {
    let cap = i64::try_from(m.cap?).unwrap_or(i64::MAX);
    let left = (cap - m.used).max(0);
    (n > left).then_some(left)
}

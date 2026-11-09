//! The 30-day chart: inline SVG bars (metered, failed stacked on top),
//! one per local day, server-rendered with maud (no JavaScript), with the
//! same numbers as a `<table>` below it for screen readers and copying.

use maud::{Markup, html};

use super::quota::Totals;
use crate::ui::{number, thai_day};

/// Width of one day's slot in the SVG's user units.
const SLOT: usize = 10;
/// Height of the plot area.
const HEIGHT: f64 = 100.0;

/// What the chart's labels say.
pub(super) struct Labels {
    pub(super) caption: String,
    pub(super) table: String,
    pub(super) day: String,
    pub(super) metered: String,
    pub(super) free: String,
    pub(super) failed: String,
}

/// `days[i]` is the local day of `totals[i]`, oldest first.
pub(super) fn chart(kind: &str, days: &[String], totals: &[Totals], l: &Labels) -> Markup {
    let top = totals
        .iter()
        .map(|t| t.metered.max(0) + t.failed.max(0))
        .max()
        .unwrap_or(0)
        .max(1);
    let scale = |n: i64| n.max(0) as f64 * HEIGHT / top as f64;
    let width = SLOT * days.len().max(1);
    let label = format!("{} ({kind})", l.caption);
    html! {
        figure class="chart" {
            figcaption class="muted" { (l.caption) " · " span class="key m" { (l.metered) } " " span class="key f" { (l.failed) } }
            svg viewBox=(format!("0 0 {width} {HEIGHT}")) preserveAspectRatio="none"
                role="img" aria-label=(label) xmlns="http://www.w3.org/2000/svg" {
                @for (i, (day, t)) in days.iter().zip(totals).enumerate() {
                    @let x = i * SLOT + 1;
                    @let hm = scale(t.metered);
                    @let hf = scale(t.failed);
                    g data-day=(day) {
                        title { (thai_day(day)) ": " (l.metered) " " (number(t.metered)) ", " (l.failed) " " (number(t.failed)) }
                        rect class="bar-m" x=(x) y=(fmt(HEIGHT - hm)) width=(SLOT - 2) height=(fmt(hm)) {}
                        rect class="bar-f" x=(x) y=(fmt(HEIGHT - hm - hf)) width=(SLOT - 2) height=(fmt(hf)) {}
                    }
                }
            }
            details {
                summary { (l.table) }
                table class="rows chart-table" aria-label=(label) {
                    thead { tr {
                        th { (l.day) } th { (l.metered) } th { (l.free) } th { (l.failed) }
                    } }
                    tbody {
                        @for (day, t) in days.iter().zip(totals).rev() {
                            tr data-day=(day) {
                                td data-label=(l.day) { (thai_day(day)) }
                                td data-label=(l.metered) { (number(t.metered)) }
                                td data-label=(l.free) { (number(t.free)) }
                                td data-label=(l.failed) { (number(t.failed)) }
                            }
                        }
                    }
                }
            }
        }
    }
}

fn fmt(v: f64) -> String {
    format!("{v:.1}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_bar_per_day_scaled_to_the_highest() {
        let days: Vec<String> = (1..=30).map(|d| format!("2026-11-{d:02}")).collect();
        let mut totals = vec![Totals::default(); 30];
        totals[29] = Totals {
            metered: 30,
            free: 0,
            failed: 10,
        };
        totals[0] = Totals {
            metered: 20,
            free: 5,
            failed: 0,
        };
        let l = Labels {
            caption: "c".into(),
            table: "t".into(),
            day: "d".into(),
            metered: "m".into(),
            free: "f".into(),
            failed: "x".into(),
        };
        let svg = chart("line", &days, &totals, &l).into_string();
        assert_eq!(svg.matches("class=\"bar-m\"").count(), 30);
        assert_eq!(svg.matches("<tr data-day=").count(), 30);
        // The highest day fills the plot: 30 + 10 of 40.
        assert!(svg.contains("x=\"291\" y=\"25.0\" width=\"8\" height=\"75.0\""));
        assert!(svg.contains("x=\"291\" y=\"0.0\" width=\"8\" height=\"25.0\""));
        assert!(svg.contains("x=\"1\" y=\"50.0\" width=\"8\" height=\"50.0\""));
    }
}

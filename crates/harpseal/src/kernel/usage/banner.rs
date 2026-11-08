//! The Quota banner: one line per Channel at `Warn` (yellow) or `Over`
//! (red), rendered into the `#usage-banner` slot of every back-office page
//! for any Staff member (the guard reads [`this_month`] once per full-page
//! GET and renders it; other requests never read it).

use maud::{Markup, html};

use super::quota::{self, ChannelMonth, Level};
use super::{FULL_PATH, store};
use crate::PageState;
use crate::kernel::students::view::channel_label;
use crate::kernel::text::fill;
use crate::message::Locale;

/// This month's usage of every Channel (one query), or `None` when the
/// read failed. The guard reads it once per full-page GET for the
/// banner, and keeps it for the home's quota card (#123).
pub(crate) async fn this_month(s: &PageState) -> Option<Vec<ChannelMonth>> {
    store::this_month(&s.deployment).await.ok().map(|(_, m)| m)
}

/// The banner for a Staff member in `locale`, or `None` when no Channel
/// in `months` is at 80 % of its Quota. `link`: the Staff member holds
/// `kernel.usage.view`, so each line links to the usage page (plain text
/// otherwise). The guard passes [`this_month`]; when that read failed it
/// shows no banner rather than failing the page (the Core has no logger;
/// the next request reads again).
pub(crate) fn render(
    s: &PageState,
    locale: &Locale,
    months: &[ChannelMonth],
    link: bool,
) -> Option<Markup> {
    let shown: Vec<&ChannelMonth> = months.iter().filter(|m| m.level >= Level::Warn).collect();
    if shown.is_empty() {
        return None;
    }
    let t = |k: &str| s.catalog.t(locale, k);
    let level = quota::highest(months).as_str();
    Some(html! {
        div class=(format!("usage-banner {level}")) data-level=(level) {
            @for m in shown {
                @let key = if m.level == Level::Over { "kernel.usage.banner_over" } else { "kernel.usage.banner_warn" };
                @let percent = m.percent().unwrap_or(0).to_string();
                @let text = fill(&t(key), &[("channel", &channel_label(&m.kind)), ("percent", &percent)]);
                p class=(format!("usage-line {}", m.level.as_str())) role="status"
                    data-channel=(m.kind) data-level=(m.level.as_str()) {
                    @if link {
                        a href=(FULL_PATH) { (text) }
                    } @else {
                        (text)
                    }
                }
            }
        }
    })
}

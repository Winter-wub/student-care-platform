//! `GET /admin/usage` (`kernel.usage.view`): one card per Channel with
//! this month's metered Pushes against the cap (a bar coloured by level),
//! free replies and failed sends, the 30-day chart, and the last 6 months.

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{Html, IntoResponse, Response};
use maud::{Markup, html};

use super::chart::{Labels, chart};
use super::quota::{self, ChannelMonth, Level, Row};
use super::store;
use super::{package, package_page};
use crate::kernel::students::view::channel_label;
use crate::kernel::text::fill;
use crate::ui::{number, thai_month};
use crate::{AppState, CurrentStaff, PageState};

/// Days in the chart: today and the 29 before.
const DAYS: i64 = 30;
/// Months in the history table, this one included.
const MONTHS: u32 = 6;

pub(super) async fn page(State(s): State<AppState>, staff: CurrentStaff) -> Response {
    let t = |k: &str| s.catalog.t(&staff.locale, k);
    let offset = s.config.utc_offset_minutes().unwrap_or(0);
    let now = s.services.now_ms();
    let days = quota::last_days(now, offset, DAYS);
    let month = quota::month_at(now, offset);
    let months = quota::last_months(&month, MONTHS);
    let oldest_month = months.last().map_or(month.clone(), |m| format!("{m}-01"));
    let since = days.first().cloned().unwrap_or_default().min(oldest_month);
    let (rows, cur) = match store::usage_and_package(&s.services, &s.config, &since).await {
        Ok(read) => read,
        Err(_) => {
            let text = t("kernel.auth.error");
            let body = html! { div class="card" { p { (text) } } };
            return respond(&s, &staff, StatusCode::INTERNAL_SERVER_ERROR, &text, body);
        }
    };
    let kinds = store::kinds(&s.deployment);
    let caps = package::caps_with(&s.config, &cur);
    let this = quota::month(&rows, &kinds, &caps, &month);
    let title = t("kernel.usage.title");
    let body = html! {
        h1 { (title) }
        p class="muted" { (t("kernel.usage.intro")) }
        @for m in &this {
            (card(&s, &staff, m, &month, &rows, &days, &months, &cur))
        }
        // Raw details (#123): where the Quota and the numbers come from.
        (crate::ui::technical(&s, &staff, html! {
            p class="code" { (t("kernel.usage.tech_config")) }
            p class="code" { (t("kernel.usage.tech_source")) }
        }))
    };
    respond(&s, &staff, StatusCode::OK, &title, body)
}

fn respond(
    s: &PageState,
    staff: &CurrentStaff,
    status: StatusCode,
    title: &str,
    body: Markup,
) -> Response {
    let page = crate::ui::back_office(s, staff, title, body);
    (status, Html(page.into_string())).into_response()
}

#[allow(clippy::too_many_arguments)]
fn card(
    s: &PageState,
    staff: &CurrentStaff,
    m: &ChannelMonth,
    month: &str,
    rows: &[Row],
    days: &[String],
    months: &[String],
    cur: &package::Current,
) -> Markup {
    let t = |k: &str| s.catalog.t(&staff.locale, k);
    let used = number(m.used());
    let level_key = match m.level {
        Level::Ok => "kernel.usage.level_ok",
        Level::Warn => "kernel.usage.level_warn",
        Level::Over => "kernel.usage.level_over",
    };
    let labels = Labels {
        caption: t("kernel.usage.chart"),
        table: t("kernel.usage.chart_table"),
        day: t("kernel.usage.day"),
        metered: t("kernel.usage.metered"),
        free: t("kernel.usage.free"),
        failed: t("kernel.usage.failed"),
    };
    let daily = quota::per_day(rows, &m.kind, days);
    html! {
        section class="card usage" id=(format!("usage-{}", m.kind))
            data-channel=(m.kind) data-level=(m.level.as_str()) {
            h2 { (channel_label(&m.kind)) }
            p class="muted" { (fill(&t("kernel.usage.this_month"), &[("month", &thai_month(month))])) }
            @match (m.cap, m.percent()) {
                (Some(cap), Some(percent)) => {
                    @let cap = number(cap);
                    @let pct = percent.to_string();
                    p class="used" {
                        (fill(&t("kernel.usage.used_of"), &[("used", &used), ("cap", &cap), ("percent", &pct)]))
                        " " span class=(format!("chip level {}", m.level.as_str())) { (t(level_key)) }
                    }
                    div class=(format!("meter {}", m.level.as_str())) role="progressbar"
                        aria-valuemin="0" aria-valuemax="100" aria-valuenow=(percent.min(100))
                        aria-label=(t("kernel.usage.metered")) {
                        span style=(format!("width:{}%", percent.min(100))) {}
                    }
                }
                _ => {
                    p class="used" { (fill(&t("kernel.usage.no_quota"), &[("used", &used)])) }
                }
            }
            @if m.kind == package::KIND { (package_page::section(s, staff, cur)) }
            dl class="kv usage-kv" {
                dt { (t("kernel.usage.metered")) } dd class="metered" { (number(m.totals.metered)) }
                dt { (t("kernel.usage.free")) } dd class="free" { (number(m.totals.free)) }
                dt { (t("kernel.usage.failed")) } dd class="failed" { (number(m.totals.failed)) }
            }
            (chart(&m.kind, days, &daily, &labels))
            h3 { (t("kernel.usage.months")) }
            table class="rows months" aria-label=(format!("{} ({})", t("kernel.usage.months"), m.kind)) {
                thead { tr {
                    th { (t("kernel.usage.month")) } th { (labels.metered) }
                    th { (labels.free) } th { (labels.failed) }
                } }
                tbody {
                    @for mo in months {
                        @let total = quota::in_month(rows, &m.kind, mo);
                        tr data-month=(mo) {
                            td data-label=(t("kernel.usage.month")) { (thai_month(mo)) }
                            td data-label=(labels.metered) { (number(total.metered)) }
                            td data-label=(labels.free) { (number(total.free)) }
                            td data-label=(labels.failed) { (number(total.failed)) }
                        }
                    }
                }
            }
        }
    }
}

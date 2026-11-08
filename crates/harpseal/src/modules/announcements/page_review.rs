//! `GET /admin/announcements/{id}/review` (#120): the step between
//! "ยืนยันส่ง" and the send. A calm summary of a draft: who it reaches
//! (counted as the form's count box counts), the message quota it uses,
//! when it goes out, and the bubble; then "ยืนยันส่ง" and "กลับไปแก้ไข".
//! A draft to everyone confirms with "ส่งถึงทุกคน N คน" in the danger
//! style, and the confirm POST must carry that N: [`everyone_ok`] counts
//! again on the server and refuses a stale or missing one.

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::Response;
use maud::{Markup, html};

use super::form::is_everyone;
use super::page_count::{metered, reach};
use super::store::{self, Row};
use super::view::View;
use crate::audience::Counts;
use crate::kernel::students::channel_label;
use crate::kernel::text::fill;
use crate::kernel::usage::MonthSummary;
use crate::{AppState, CurrentStaff, Deployment, Result};

/// The posted field carrying the reach confirmed (everyone only), and the
/// one saying the review step was seen.
pub(super) const REACH: &str = "reach";
pub(super) const REVIEWED: &str = "reviewed";

pub(super) fn path(id: &str) -> String {
    format!("/admin/announcements/{id}/review")
}

/// `None` when `row` does not go to everyone. Otherwise whether `posted`
/// (the `reach` field) is the number of Students it reaches now.
pub(super) async fn everyone_ok(
    d: &Deployment,
    row: &Row,
    posted: Option<&str>,
) -> Result<Option<bool>> {
    if !is_everyone(&row.audience) {
        return Ok(None);
    }
    let now = crate::audience::count(d, &row.audience).await?.students;
    Ok(Some(
        posted.and_then(|p| p.trim().parse::<usize>().ok()) == Some(now),
    ))
}

/// The danger button of a send to everyone, carrying its reach.
pub(super) fn everyone_button(v: &View<'_>, n: usize) -> Markup {
    let label = fill(
        &v.t("announcements.review.everyone_button"),
        &[("n", &n.to_string())],
    );
    html! {
        input type="hidden" name=(REACH) value=(n);
        (crate::ui::danger_button(&label))
    }
}

pub(super) async fn page(
    State(s): State<AppState>,
    staff: CurrentStaff,
    Path(id): Path<String>,
) -> Response {
    let v = View::new(&s, &staff);
    let svc = &s.services;
    let (row, texts) = match store::load(svc, &id).await {
        Ok(Some(row)) => match store::texts(svc, &id).await {
            Ok(t) => (row, t),
            Err(e) => return v.error(&e),
        },
        Ok(None) => return v.not_found(),
        Err(e) => return v.error(&e),
    };
    let detail = format!("/admin/announcements/{id}");
    if row.status != "draft" {
        return v.redirect(&detail, false, "announcements.review.not_draft");
    }
    let (counts, used) = reach(&s.deployment, &row.audience).await;
    let Some(counts) = counts else {
        return v.error(&crate::Error::Invalid("audience".into()));
    };
    let (title, body) = texts.0.get("th").cloned().unwrap_or_default();
    let heading = v.t("announcements.review.title");
    let page = html! {
        h1 { (heading) }
        p class="muted" { (v.t("announcements.review.lead")) }
        div class="cols review" {
            div class="card" { (summary(&v, &row, &counts, &used)) }
            (super::page_detail::preview(&v, &title, &body, row.require_ack))
        }
        div class="actions" {
            form method="post" action=(format!("{detail}/confirm")) class="inline" {
                (v.staff.csrf_field())
                input type="hidden" name="version" value=(row.version);
                input type="hidden" name=(REVIEWED) value="1";
                @if is_everyone(&row.audience) {
                    (everyone_button(&v, counts.students))
                } @else {
                    button type="submit" { (v.t("announcements.review.confirm")) }
                }
            }
            a class="btn secondary" href=(format!("{detail}/edit")) { (v.t("announcements.review.back")) }
        }
    };
    v.page(StatusCode::OK, &heading, page)
}

fn summary(v: &View<'_>, row: &Row, c: &Counts, used: &[(String, MonthSummary)]) -> Markup {
    let n = |k: &str, n: usize| fill(&v.t(k), &[("n", &n.to_string())]);
    let a = &row.audience;
    let d = v.s.deployment.as_ref();
    let when = match row.send_at {
        Some(t) if t > v.s.services.now_ms() => fill(
            &v.t("announcements.review.later"),
            &[("time", &v.time(Some(t)))],
        ),
        _ => v.t("announcements.review.now"),
    };
    let quota: Vec<String> = c
        .per_channel
        .iter()
        .filter(|(k, _)| metered(d, k))
        .flat_map(|(k, (active, _))| quota_lines(v, k, *active, used))
        .collect();
    html! {
        dl class="facts" {
            dt { (v.t("announcements.review.who")) }
            dd {
                @if is_everyone(a) {
                    (v.t("announcements.review.everyone"))
                } @else {
                    @if !a.attributes.is_empty() || !a.tags.is_empty() {
                        (v.t("announcements.review.groups"))
                        @if !a.students.is_empty() { " · " }
                    }
                    @if !a.students.is_empty() { (n("announcements.review.picked", a.students.len())) }
                }
            }
            dt { (v.t("announcements.review.count")) }
            dd data-count="students" { strong { (n("announcements.review.people", c.students)) } }
            dt { (v.t("announcements.review.channels")) }
            dd {
                @if c.per_channel.is_empty() { span class="muted" { (v.t("announcements.review.no_links")) } }
                @for (kind, (active, unreachable)) in &c.per_channel {
                    div data-channel=(kind) {
                        (fill(&v.t("announcements.review.channel"),
                            &[("channel", &channel_label(kind)), ("n", &active.to_string())]))
                        @if *unreachable > 0 {
                            span class="muted" { " · " (n("announcements.review.skipped", *unreachable)) }
                        }
                    }
                }
            }
            dt { (v.t("announcements.review.quota")) }
            dd data-quota {
                @if quota.is_empty() { (v.t("announcements.review.quota_none")) }
                @for line in &quota { div { (line) } }
            }
            dt { (v.t("announcements.review.when")) }
            dd { (when) }
        }
    }
}

/// "จะใช้ข้อความ LINE ประมาณ N ข้อความ จากโควตาเดือนนี้ที่เหลือ M", and
/// how far over what is left, if it is.
fn quota_lines(
    v: &View<'_>,
    kind: &str,
    units: usize,
    used: &[(String, MonthSummary)],
) -> Vec<String> {
    let channel = channel_label(kind);
    let n = units.to_string();
    let cap = used
        .iter()
        .find(|(k, _)| k == kind)
        .and_then(|(_, m)| m.cap.map(|cap| (cap, m.used)));
    let Some((cap, spent)) = cap else {
        let text = v.t("announcements.review.quota_no_cap");
        return vec![fill(&text, &[("channel", &channel), ("n", &n)])];
    };
    let left = cap.saturating_sub(u64::try_from(spent).unwrap_or(0));
    let text = v.t("announcements.review.quota_left");
    let mut lines = vec![fill(
        &text,
        &[
            ("channel", &channel),
            ("n", &n),
            ("left", &left.to_string()),
        ],
    )];
    let over = (units as u64).saturating_sub(left);
    if over > 0 {
        let text = v.t("announcements.review.quota_over");
        lines.push(fill(&text, &[("over", &over.to_string())]));
    }
    lines
}

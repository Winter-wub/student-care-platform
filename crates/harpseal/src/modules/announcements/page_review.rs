//! `GET /admin/announcements/{id}/review` (#120): the step between
//! "ยืนยันส่ง" and the send. A calm summary of a draft: who it reaches
//! (counted as the form's count box counts), the message quota it uses,
//! when it goes out, and the bubble; then "ยืนยันส่ง" and "กลับไปแก้ไข".
//! A draft to everyone confirms with "ส่งถึงทุกคน N คน" in the danger
//! style, and the confirm POST must carry that N: [`everyone_ok`] counts
//! again on the server and refuses a stale or missing one. A draft that
//! reaches no chat account (no Student linked, or every Link unreachable)
//! cannot be sent now (#135): the button is disabled with the reason, and
//! the confirm refuses it ([`nobody`]). A schedule still ahead is
//! confirmable with a caution note: its recipients resolve when it goes
//! out ([`goes_now`]).

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::http::header::LOCATION;
use axum::response::{IntoResponse, Response};
use maud::{Markup, html};

use super::form::is_everyone;
use super::page_count::{metered, reach};
use super::store::{self, Row};
use super::view::View;
use crate::audience::Counts;
use crate::kernel::students::channel_label;
use crate::kernel::text::fill;
use crate::kernel::usage::{LINE_KIND, MonthSummary};
use crate::{AppState, CurrentStaff, Deployment, Result};

/// The posted field carrying the reach confirmed (everyone only), and the
/// one saying the review step was seen.
pub(super) const REACH: &str = "reach";
pub(super) const REVIEWED: &str = "reviewed";

pub(super) fn path(id: &str) -> String {
    format!("/admin/announcements/{id}/review")
}

/// Saves a scheduled announcement whose Audience changed (#120): its
/// schedule is cancelled (back to a draft, audit `announcement.cancel`)
/// and the edit saved on the draft (`announcement.update`), so it goes out
/// again only through this review step and its confirm, with the danger
/// button and the recount when it now reaches everyone. Answers the
/// review step with its "held" note; a cancel that fails leaves the
/// schedule as it was (conflict / not editable); an update that fails
/// after the cancel leaves a draft with the old content, and says so.
#[allow(clippy::too_many_arguments)]
pub(super) async fn hold(
    v: &View<'_>,
    d: &Deployment,
    id: &str,
    version: i64,
    draft: super::service::Draft,
    actor: &crate::Actor,
    ctx: &crate::RequestCtx,
) -> Response {
    let detail = format!("/admin/announcements/{id}");
    match super::service::cancel(d, id, version, actor, ctx).await {
        Ok(()) => {}
        Err(crate::Error::Conflict(_)) => {
            return v.redirect(&detail, false, "announcements.flash.conflict");
        }
        Err(crate::Error::Invalid(_)) => {
            return v.redirect(&detail, false, "announcements.flash.not_editable");
        }
        Err(e) => return v.error(&e),
    }
    match super::service::update(d, id, version + 1, draft, actor, ctx).await {
        Ok(()) => (
            StatusCode::SEE_OTHER,
            [(LOCATION, format!("{}?held=1", path(id)))],
        )
            .into_response(),
        // The schedule is gone either way: say it is a draft now.
        Err(_) => v.redirect(&detail, false, "announcements.review.held_failed"),
    }
}

/// `None` when `row` does not go to everyone. Otherwise whether `posted`
/// (the `reach` field) is the number of Students it reaches now, `c` (its
/// Audience counted now).
pub(super) fn everyone_ok(row: &Row, c: &Counts, posted: Option<&str>) -> Option<bool> {
    if !is_everyone(&row.audience) {
        return None;
    }
    Some(posted.and_then(|p| p.trim().parse::<usize>().ok()) == Some(c.students))
}

/// Whether confirming `row` at `now` (server clock, ms) sends it at once:
/// no `send_at`, or one already past. A schedule still ahead resolves its
/// recipients when it goes out, so reaching nobody yet does not block it
/// (#135).
pub(super) fn goes_now(row: &Row, now: i64) -> bool {
    row.send_at.is_none_or(|t| t <= now)
}

/// Why a send that reaches `c` cannot go (#135): the catalog key of the
/// reason when no chat account would get it, `None` otherwise. Unreachable
/// Links are skipped by the send, so they do not count.
pub(super) fn nobody(c: &Counts) -> Option<&'static str> {
    if c.per_channel.values().any(|(active, _)| *active > 0) {
        return None;
    }
    Some(if c.students == 0 {
        "announcements.review.nobody"
    } else {
        "announcements.review.nobody_reachable"
    })
}

/// How many of `a`'s Students have a live Link, when `a` also counts
/// unreachable ones (`None` otherwise): the danger label names both.
pub(super) async fn reachable(
    d: &Deployment,
    a: &crate::audience::Audience,
) -> Result<Option<usize>> {
    if !a.include_unreachable {
        return Ok(None);
    }
    let mut live = a.clone();
    live.include_unreachable = false;
    Ok(Some(crate::audience::count(d, &live).await?.students))
}

/// The danger button of a send to everyone, carrying its reach `n`;
/// "(ติดต่อได้ M)" when `reachable` is fewer.
pub(super) fn everyone_button(v: &View<'_>, n: usize, reachable: Option<usize>) -> Markup {
    let label = match reachable.filter(|m| *m != n) {
        Some(m) => fill(
            &v.t("announcements.review.everyone_button_reach"),
            &[("n", &n.to_string()), ("m", &m.to_string())],
        ),
        None => fill(
            &v.t("announcements.review.everyone_button"),
            &[("n", &n.to_string())],
        ),
    };
    html! {
        input type="hidden" name=(REACH) value=(n);
        (crate::ui::danger_button(&label))
    }
}

pub(super) async fn page(
    State(s): State<AppState>,
    staff: CurrentStaff,
    Path(id): Path<String>,
    Query(q): Query<Vec<(String, String)>>,
) -> Response {
    // `?held=1`: a schedule whose Audience changed came back here (`hold`).
    let held = q.iter().any(|(k, val)| k == "held" && val == "1");
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
        // The Audience no longer counts (e.g. the roster schema changed).
        let edit = format!("{detail}/edit");
        return v.redirect(&edit, false, "announcements.form.audience_invalid");
    };
    let everyone = is_everyone(&row.audience);
    let live = match everyone {
        true => reachable(&s.deployment, &row.audience)
            .await
            .unwrap_or(None),
        false => None,
    };
    // Nobody to reach blocks only a send that goes now (#135).
    let empty = nobody(&counts);
    let blocked = empty.filter(|_| goes_now(&row, svc.now_ms()));
    let extra = crate::kernel::usage::line_extra(svc, &s.config)
        .await
        .unwrap_or(None);
    let (title, body) = texts.0.get("th").cloned().unwrap_or_default();
    let heading = v.t("announcements.review.title");
    let page = html! {
        h1 { (heading) }
        @if held { p class="note" role="status" { (v.t("announcements.review.held")) } }
        p class="muted" { (v.t("announcements.review.lead")) }
        div class="cols review" {
            div class="card" { (summary(&v, &row, &counts, &used, extra)) }
            (super::page_detail::preview(&v, &title, &body, row.require_ack))
        }
        @if empty.is_some() && blocked.is_none() {
            p class="note warn-note" { (v.t("announcements.review.nobody_later")) }
        }
        div class="actions" {
            @if let Some(why) = blocked {
                (crate::ui::blocked_button(&v.t("announcements.review.confirm"), &v.t(why), "review-blocked"))
            } @else {
                form method="post" action=(format!("{detail}/confirm")) class="inline" {
                    (v.staff.csrf_field())
                    input type="hidden" name="version" value=(row.version);
                    input type="hidden" name=(REVIEWED) value="1";
                    // A schedule ahead to everyone with nobody yet: the plain
                    // label beside the caution note, never "ส่งถึงทุกคน 0
                    // คน"; the reach still goes for the recount.
                    @if everyone && empty.is_none() {
                        (everyone_button(&v, counts.students, live))
                    } @else if everyone {
                        input type="hidden" name=(REACH) value=(counts.students);
                        button type="submit" { (v.t("announcements.review.confirm")) }
                    } @else {
                        button type="submit" { (v.t("announcements.review.confirm")) }
                    }
                }
            }
            a class="btn secondary" href=(format!("{detail}/edit")) { (v.t("announcements.review.back")) }
        }
    };
    v.page(StatusCode::OK, &heading, page)
}

/// `extra`: whether the LINE package buys extra messages (`None`: unknown).
fn summary(
    v: &View<'_>,
    row: &Row,
    c: &Counts,
    used: &[(String, MonthSummary)],
    extra: Option<bool>,
) -> Markup {
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
        .flat_map(|(k, (active, _))| quota_lines(v, k, *active, used, extra))
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
/// how far over what is left, if it is, with what then happens: LINE
/// Free refuses the rest (announcements never fall back to email), a
/// package with extra messages costs more, any other Quota only warns.
fn quota_lines(
    v: &View<'_>,
    kind: &str,
    units: usize,
    used: &[(String, MonthSummary)],
    extra: Option<bool>,
) -> Vec<String> {
    let channel = channel_label(kind);
    let n = crate::ui::number(units);
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
            ("left", &crate::ui::number(left)),
        ],
    )];
    let over = (units as u64).saturating_sub(left);
    if over > 0 {
        let key = match (kind == LINE_KIND, extra) {
            (true, Some(true)) => "announcements.review.quota_over",
            (true, Some(false)) => "announcements.review.quota_over_free",
            (true, None) => "announcements.review.quota_over_unknown",
            (false, _) => "announcements.review.quota_over_warn",
        };
        let text = v.t(key);
        lines.push(fill(&text, &[("over", &crate::ui::number(over))]));
    }
    lines
}

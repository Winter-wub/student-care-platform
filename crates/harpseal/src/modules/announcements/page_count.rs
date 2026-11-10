//! The live Audience count `POST /admin/announcements/audience` (an HTMX
//! fragment): Students, Links per Channel, the metered Push estimate and,
//! for a hand-picked list, the ids that were not found or have no live
//! Link (listed, never sent to).
//!
//! The box itself is not a live region (#156): it is replaced on every
//! change. One short line ("กลุ่มผู้รับนี้มีนักศึกษา 12 คน", or what is
//! missing) goes to the persistent `#count-said` region instead, and only
//! when it differs from the line before (`count_was`, carried by the box).

use axum::Form;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{Html, IntoResponse, Response};
use maud::{Markup, html};

use super::form::Fields;
use super::view::View;
use crate::audience::{self, Audience, Counts, HandPicked, MAX_STUDENTS};
use crate::kernel::students::channel_label;
use crate::kernel::text::fill;
use crate::kernel::usage::{self, MonthSummary};
use crate::{AppState, CurrentStaff, Deployment};

type Posted = Form<Vec<(String, String)>>;

const PATH: &str = "/admin/announcements/audience";
/// Count again after a form change or after the picker added a Student
/// (its response carries `HX-Trigger: students-changed`).
const TRIGGER: &str = "change from:closest form, students-changed from:body";
/// Ids named in one list of the preview; the rest are counted.
const LISTED: usize = 100;
/// `count_was` of the box before its first answer: say nothing then.
const FIRST: &str = "-";

pub(super) async fn fragment(
    State(s): State<AppState>,
    staff: CurrentStaff,
    Form(pairs): Posted,
) -> Response {
    let v = View::new(&s, &staff);
    let f = Fields::from_pairs(&pairs);
    let was = pairs
        .iter()
        .find(|(k, _)| k == "count_was")
        .map_or(FIRST, |(_, x)| x.as_str());
    // #120: no count of everyone until everyone is chosen. #156: a range
    // that cannot be used says why, as the draft would.
    let range_problem = || {
        let errors = f.audience_errors(&s.config.roster);
        errors
            .into_iter()
            .map(|(_, k)| k)
            .find(|k| k.starts_with("announcements.form.range_"))
    };
    let (shown, line) = if let Some(key) = f.scope_problem().or_else(range_problem) {
        let line = v.t(key);
        (boxed(html! { p class="muted" { (line) } }, &line), line)
    } else {
        let a = f.audience();
        let (counts, used) = reach(&s.deployment, &a).await;
        let picked = match (&counts, a.students.is_empty()) {
            (Some(_), false) => audience::hand_picked(&s.deployment, &a).await.ok(),
            _ => None,
        };
        count_box(&v, &a, counts.as_ref(), picked.as_ref(), &used)
    };
    let said = (was != FIRST && was != line).then(|| said(&line, true));
    let out = html! { (shown) @if let Some(s) = said { (s) } };
    (StatusCode::OK, Html(out.into_string())).into_response()
}

/// The live region of the count's short line (`#count-said`, read
/// aloud, not shown); `oob` when an answer updates it out of band.
fn said(line: &str, oob: bool) -> Markup {
    html! {
        div id="count-said" class="visually-hidden" role="status" aria-live="polite"
            hx-swap-oob=[oob.then_some("true")] { (line) }
    }
}

/// The empty `#count-said`, once per page, outside the fieldset (which a
/// saved Audience swaps whole).
pub(super) fn said_region() -> Markup {
    said("", false)
}

/// `count_was` as the box's `hx-vals`.
fn vals(line: &str) -> String {
    serde_json::json!({ "count_was": line }).to_string()
}

/// What `a` reaches, as the count box and the review step (#120) both
/// show it: the counts (`None` when the Audience is not valid) and this
/// month's usage of each Channel it reaches.
pub(super) async fn reach(
    d: &Deployment,
    a: &Audience,
) -> (Option<Counts>, Vec<(String, MonthSummary)>) {
    let counts = audience::count(d, a).await.ok();
    // This month's usage beside each Channel, as the Quota banner: a
    // dev-only Channel's only in dev mode (#131). Outside dev it reaches
    // no one either (#133): `audience::count` skips it, as every send does.
    let kinds: Vec<String> = counts
        .iter()
        .flat_map(|c| c.per_channel.keys().map(|k| k.to_string()))
        .filter(|k| d.staff_sees_channel(k))
        .collect();
    let used = usage::month_summaries(d, &kinds).await.unwrap_or_default();
    (counts, used)
}

/// Whether a send on Channel `kind` is metered: one unit per recipient
/// (one Card is one Push request).
pub(super) fn metered(d: &Deployment, kind: &str) -> bool {
    d.channel(kind).is_some_and(|ch| ch.multicast().is_some())
        || d.config().quotas.contains_key(kind)
}

/// The count box around `inner`, counting again on every form change;
/// `line` is what its live line said.
fn boxed(inner: Markup, line: &str) -> Markup {
    shell(inner, line, TRIGGER)
}

fn shell(inner: Markup, line: &str, trigger: &str) -> Markup {
    html! {
        div id="audience-count" class="card count" hx-post=(PATH)
            hx-trigger=(trigger) hx-include="closest form" hx-params=(super::form_images::NOT_FILES)
            hx-vals=(vals(line)) hx-target="this" hx-swap="outerHTML" { (inner) }
    }
}

/// Count, per Channel, and "จะใช้ push ประมาณ x ครั้ง": one metered unit per
/// recipient on a metered Channel (one Card is one Push request).
/// `used`: this month's usage per Channel (#26), shown beside the estimate.
/// With the box, its short live line.
fn count_box(
    v: &View<'_>,
    a: &Audience,
    counts: Option<&Counts>,
    picked: Option<&HandPicked>,
    used: &[(String, MonthSummary)],
) -> (Markup, String) {
    let Some(c) = counts else {
        let why = if a.students.len() > MAX_STUDENTS {
            "announcements.form.students_too_many"
        } else {
            "announcements.form.audience_invalid"
        };
        let line = v.t(why);
        return (
            boxed(html! { p class="muted field-error" { (line) } }, &line),
            line,
        );
    };
    let line = fill(
        &v.t("announcements.form.count_said"),
        &[("n", &crate::ui::number(c.students))],
    );
    let d = v.s.deployment.as_ref();
    let metered = |kind: &str| metered(d, kind);
    let units: usize = c
        .per_channel
        .iter()
        .filter(|(k, _)| metered(k))
        .map(|(_, (active, _))| *active)
        .sum();
    let inner = html! {
            p { strong { (v.t("announcements.form.students")) " " (crate::ui::number(c.students)) } }
            @if let Some(p) = picked { (hand_picked(v, a, c, p)) }
            ul class="list" {
                @for (kind, (active, unreachable)) in &c.per_channel {
                    li {
                        (channel_label(kind)) ": " (crate::ui::number(*active)) " " (v.t("announcements.form.links"))
                        @if *unreachable > 0 {
                            " (" (unreachable) " " (v.t("announcements.form.unreachable_skipped")) ")"
                        }
                        @if metered(kind) {
                            " · " (v.t("announcements.form.estimate_channel")) " " (crate::ui::number(*active))
                        }
                        @if let Some((_, m)) = used.iter().find(|(k, _)| k == kind) {
                            " · " span class="month-used" { (month_used(v, m)) }
                        }
                    }
                }
            }
            p class="estimate" { (v.t("announcements.form.estimate")) " " (crate::ui::number(units)) " " (v.t("announcements.form.times")) }
    };
    (boxed(inner, &line), line)
}

/// The hand-picked list: how many ids, and which of them will not be sent.
fn hand_picked(v: &View<'_>, a: &Audience, c: &Counts, p: &HandPicked) -> Markup {
    let filtered = p.linked.saturating_sub(c.students);
    let ids = |key: &str, id: &'static str, list: &[String]| -> Markup {
        html! {
            @if !list.is_empty() {
                p id=(id) class="field-error" {
                    (v.t(key)) " (" (list.len()) "): "
                    // Each id whole (#140): the list wraps between ids.
                    @for (i, id) in list.iter().take(LISTED).enumerate() {
                        @if i > 0 { ", " } span class="id" { (id) }
                    }
                    @if list.len() > LISTED {
                        " " (v.t("announcements.form.hp_more")) " " (list.len() - LISTED)
                    }
                }
            }
        }
    };
    html! {
        div id="hand-picked-report" {
            p { (v.t("announcements.form.hp_listed")) " " (a.students.len()) }
            (ids("announcements.form.hp_unknown", "hand-picked-unknown", &p.unknown))
            (ids("announcements.form.hp_unlinked", "hand-picked-unlinked", &p.unlinked))
            @if filtered > 0 {
                p id="hand-picked-filtered" class="muted" {
                    (v.t("announcements.form.hp_filtered")) " " (filtered)
                }
            }
        }
    }
}

/// "ใช้ไปแล้ว X/Y เดือนนี้" (or without a Quota, "ใช้ไปแล้ว X เดือนนี้").
pub(super) fn month_used(v: &View<'_>, m: &MonthSummary) -> String {
    let used = crate::ui::number(m.used);
    match m.cap {
        Some(cap) => fill(
            &v.t("kernel.usage.send_used"),
            &[("used", &used), ("cap", &crate::ui::number(cap))],
        ),
        None => fill(&v.t("kernel.usage.send_used_no_cap"), &[("used", &used)]),
    }
}

/// The count box before its first HTMX load.
pub(super) fn placeholder(v: &View<'_>) -> Markup {
    let inner = html! { p class="muted" { (v.t("announcements.form.counting")) } };
    shell(inner, FIRST, &format!("load, {TRIGGER}"))
}

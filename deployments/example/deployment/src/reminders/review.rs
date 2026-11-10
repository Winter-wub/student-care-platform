//! The review step of "ตรวจก่อนส่งเตือน" (#135), on the pattern of the
//! Core's announcement review: who gets the reminder (the count and the
//! first names), by which channel and how many chat messages that uses,
//! who is skipped and why, and the message; then "ยืนยันส่งเตือน N คน"
//! and "กลับไปแก้ไข". The plan reads the Students and live Links the way
//! [`harpseal::PageState::notify`] routes them: chat on every active
//! Link, else email when there is an address, else skipped. Nothing is
//! sent from here: the confirm posts the same form with `reviewed=1` and
//! the count seen in `reach`, and [`super::send_now`] plans again and
//! sends only when the count still matches and is not 0.

use std::collections::BTreeMap;

use axum::response::{Html, IntoResponse, Response};
use harpseal::{AppState, CurrentStaff, MAX_NOTIFY, Result, Statement, Value, ui};
use maud::{Markup, html};

use super::PATH;

/// The posted field saying the review step was seen.
pub(super) const REVIEWED: &str = "reviewed";
/// The posted field carrying the number of recipients the review showed.
pub(super) const REACH: &str = "reach";
/// How many names the review lists before "และอีก N คน".
const NAMES: usize = 5;
/// Who may see Students' names: others see their codes (#135).
const STUDENT_VIEW: &str = "kernel.student.view";

/// Why a pasted id gets nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Skip {
    Unknown,
    Left,
    NoWay,
}

impl Skip {
    fn key(self) -> &'static str {
        match self {
            Skip::Unknown => "reminders.skip_unknown",
            Skip::Left => "reminders.skip_left",
            Skip::NoWay => "reminders.skip_no_way",
        }
    }
}

/// Who a send of the pasted ids reaches, and how.
#[derive(Debug, Default)]
pub(super) struct Plan {
    /// The ids as pasted (parsed), the form's `ids` again.
    pub(super) ids: Vec<String>,
    /// (code, display name) of the Students who get it, in pasted order.
    pub(super) people: Vec<(String, String)>,
    /// Channel kind → active Links that get it by chat.
    pub(super) chat: BTreeMap<String, usize>,
    /// Students who get it by email (no active Link, an address).
    pub(super) email: usize,
    /// Pasted ids that get nothing, and why.
    pub(super) skipped: Vec<(String, Skip)>,
}

impl Plan {
    /// Students who get the reminder, by chat or by email.
    pub(super) fn reach(&self) -> usize {
        self.people.len()
    }

    /// The catalog key of why it cannot be sent, if it cannot.
    pub(super) fn blocked(&self) -> Option<&'static str> {
        if self.ids.is_empty() {
            Some("reminders.no_ids")
        } else if self.ids.len() > MAX_NOTIFY {
            Some("reminders.too_many_now")
        } else if self.reach() == 0 {
            Some("reminders.nobody")
        } else {
            None
        }
    }
}

fn marks(n: usize) -> String {
    vec!["?"; n].join(", ")
}

/// Plans a send to `ids` (at most [`MAX_NOTIFY`] are read; more are
/// refused by [`Plan::blocked`] before any query).
pub(super) async fn plan(s: &AppState, ids: Vec<String>) -> Result<Plan> {
    let mut p = Plan {
        ids,
        ..Plan::default()
    };
    if p.ids.is_empty() || p.ids.len() > MAX_NOTIFY {
        return Ok(p);
    }
    let mut q = Statement::new(format!(
        "SELECT id, student_code, display_name, email, status FROM student \
         WHERE student_code IN ({})",
        marks(p.ids.len())
    ));
    for id in &p.ids {
        q = q.bind(id.clone());
    }
    let rows = s.services.db.query(q).await?;
    let text = |i: usize, c: &str| {
        rows.get(i, c)
            .and_then(Value::as_text)
            .unwrap_or_default()
            .to_string()
    };
    let students: Vec<[String; 5]> = (0..rows.len())
        .map(|i| ["id", "student_code", "display_name", "email", "status"].map(|c| text(i, c)))
        .collect();
    let mut q = Statement::new(format!(
        "SELECT l.student_id, ca.channel FROM link l \
         JOIN channel_account ca ON ca.id = l.channel_account_id \
         WHERE l.status = 'active' AND l.student_id IN ({})",
        marks(students.len().max(1))
    ));
    for st in &students {
        q = q.bind(st[0].clone());
    }
    if students.is_empty() {
        q = q.bind(String::new());
    }
    let links = s.services.db.query(q).await?;
    let link_rows: Vec<(String, String)> = (0..links.len())
        .map(|i| {
            let t = |c: &str| {
                links
                    .get(i, c)
                    .and_then(Value::as_text)
                    .unwrap_or_default()
                    .to_string()
            };
            (t("student_id"), t("channel"))
        })
        .collect();
    let mut seen = std::collections::HashSet::new();
    for code in p.ids.clone() {
        if !seen.insert(code.clone()) {
            continue;
        }
        let Some([id, _, name, email, status]) = students.iter().find(|s| s[1] == code) else {
            p.skipped.push((code, Skip::Unknown));
            continue;
        };
        if status == "left" {
            p.skipped.push((code, Skip::Left));
            continue;
        }
        let mine: Vec<&String> = link_rows
            .iter()
            .filter(|(sid, _)| sid == id)
            .map(|(_, ch)| ch)
            .collect();
        if !mine.is_empty() {
            for ch in mine {
                *p.chat.entry(ch.clone()).or_default() += 1;
            }
        } else if !email.trim().is_empty() {
            p.email += 1;
        } else {
            p.skipped.push((code, Skip::NoWay));
            continue;
        }
        p.people.push((code, name.clone()));
    }
    Ok(p)
}

/// `v` percent-encoded for a query value (unreserved bytes kept).
fn encode(v: &str) -> String {
    v.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                (b as char).to_string()
            }
            _ => format!("%{b:02X}"),
        })
        .collect()
}

/// A Channel kind as Staff know it.
fn channel_name(kind: &str) -> String {
    match kind {
        "line" => "LINE".into(),
        "telegram" => "Telegram".into(),
        other => other.into(),
    }
}

/// The review page of `plan`, posting `key` back; `changed`: the count
/// moved since the last review.
pub(super) async fn page(
    s: &AppState,
    staff: &CurrentStaff,
    key: &str,
    plan: &Plan,
    changed: bool,
) -> Response {
    let t = |k: &str| s.catalog.t(&staff.locale, k);
    let n = |k: &str, n: usize| t(k).replace("{n}", &ui::number(n));
    let title = t("reminders.review_title");
    let action = format!("/admin{PATH}");
    let (th_subject, th_text) = super::NOTICE_TH;
    let ids = plan.ids.join("\n");
    // "กลับไปแก้ไข" is a link back to the page with the list, unless the
    // list is too long for a URL (Cloudflare refuses 16 KB): then the
    // list form is on this page, filled in.
    let back_query = format!("ids={}", encode(&ids));
    let inline_edit = back_query.len() > ui::MAX_PREFILL_QUERY;
    let edit_form = if inline_edit {
        match super::form::list_form(s, staff, key, plan.ids.clone()).await {
            Ok(f) => f,
            Err(e) => return ui::service_error(s, staff, &e),
        }
    } else {
        html! {}
    };
    let label = match plan.blocked() {
        Some(_) => t("reminders.confirm_plain"),
        None => n("reminders.confirm", plan.reach()),
    };
    let body = html! {
        h1 { (title) }
        @if changed { p class="note" role="status" { (t("reminders.changed")) } }
        p class="muted" { (t("reminders.review_lead")) }
        div class="cols review" {
            div class="card" { (facts(s, staff, plan)) }
            div class="card preview" aria-label=(t("reminders.review_preview")) {
                p class="muted" { (t("reminders.review_preview")) }
                div class="bubble" { p { (th_text) } }
                p class="muted" { (t("reminders.review_subject").replace("{subject}", th_subject)) }
            }
        }
        div class="actions" {
            @if let Some(why) = plan.blocked() {
                @let reason = t(why).replace("{max}", &MAX_NOTIFY.to_string());
                (ui::blocked_button(&label, &reason, "reminders-blocked"))
            } @else {
                form method="post" action=(action) class="inline" {
                    (staff.csrf_field())
                    input type="hidden" name="key" value=(key);
                    input type="hidden" name="ids" value=(ids);
                    input type="hidden" name=(REVIEWED) value="1";
                    input type="hidden" name=(REACH) value=(plan.reach());
                    button type="submit" { (label) }
                }
            }
            @if inline_edit {
                a class="btn secondary" href="#edit-list" { (t("reminders.back")) }
            } @else {
                a class="btn secondary" href=(format!("{action}?{back_query}")) { (t("reminders.back")) }
            }
        }
        @if inline_edit {
            section class="card" id="edit-list" {
                h2 { (t("reminders.edit_title")) }
                (edit_form)
            }
        }
    };
    Html(ui::back_office(s, staff, &title, body).into_string()).into_response()
}

fn facts(s: &AppState, staff: &CurrentStaff, p: &Plan) -> Markup {
    let t = |k: &str| s.catalog.t(&staff.locale, k);
    let n = |k: &str, n: usize| t(k).replace("{n}", &ui::number(n));
    let more = p.people.len().saturating_sub(NAMES);
    // Names only for Staff who may view Students; codes otherwise.
    let names = staff.can(STUDENT_VIEW);
    let shown: Vec<&str> = p
        .people
        .iter()
        .take(NAMES)
        .map(|(code, name)| if names { name.as_str() } else { code.as_str() })
        .collect();
    html! {
        dl class="facts" {
            dt { (t("reminders.review_who")) }
            dd data-count="recipients" { strong { (n("reminders.review_people", p.reach())) } }
            @if !p.people.is_empty() {
                dt { (t(if names { "reminders.review_names" } else { "reminders.review_codes" })) }
                dd data-names {
                    (shown.join(", "))
                    @if more > 0 { " " (n("reminders.review_more", more)) }
                }
            }
            dt { (t("reminders.review_channels")) }
            dd {
                @if p.chat.is_empty() && p.email == 0 { "-" }
                @for (kind, count) in &p.chat {
                    div data-channel=(kind) {
                        (t("reminders.review_chat")
                            .replace("{channel}", &channel_name(kind))
                            .replace("{n}", &ui::number(*count)))
                    }
                }
                @if p.email > 0 { div data-channel="email" { (n("reminders.review_email", p.email)) } }
            }
            @if !p.skipped.is_empty() {
                dt { (t("reminders.review_skipped")) }
                dd {
                    ul class="list" {
                        @for (code, why) in &p.skipped {
                            li { span class="code" { (code) } ": " (t(why.key())) }
                        }
                    }
                }
            }
        }
    }
}

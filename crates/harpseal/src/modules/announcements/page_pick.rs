//! The Student picker beside the pasted ids (#76): `GET
//! /admin/announcements/pick?q=` lists Students from the Students search
//! (an HTMX fragment); `POST` adds one id to the hand-picked list and
//! returns the whole field, so the list on the page and the one stored
//! are the same text. Staff need `kernel.student.view`, as for the
//! Students list.

use axum::Form;
use axum::extract::{Query, State};
use axum::http::{HeaderValue, StatusCode};
use axum::response::{Html, IntoResponse, Response};
use maud::{Markup, html};
use serde::Deserialize;

use super::form::Fields;
use super::view::View;
use crate::audience::parse_student_ids;
use crate::kernel::students::{channel_label, search};
use crate::{AppState, CurrentStaff};

const PATH: &str = "/admin/announcements/pick";
const VIEW: &str = "kernel.student.view";
/// Most characters of a search (the Students search cuts it again to fit
/// the 50-byte `LIKE` limit of D1).
const MAX_QUERY: usize = 100;

#[derive(Debug, Default, Deserialize)]
pub(super) struct PickQuery {
    #[serde(default)]
    q: String,
}

/// The hand-picked field: the textarea, and the picker for Staff allowed
/// to see Students. Everything the browser swaps on "add".
pub(super) fn field(v: &View<'_>, f: &Fields) -> Markup {
    html! {
        div id="hand-picked" class="field" {
            label for="students" { (v.t("announcements.form.hand_picked")) }
            p class="muted" { (v.t("announcements.form.hand_picked_help")) }
            textarea id="students" name="students" rows="4" { (f.students) }
            @if v.staff.can(VIEW) {
                // `form` names no form, which detaches the box from the page
                // form: Enter in it must search, not submit the announcement.
                label for="pick-q" { (v.t("announcements.form.pick_search")) }
                input id="pick-q" type="search" name="q" form="pick-detached" autocomplete="off"
                    hx-get=(PATH) hx-trigger="input changed delay:250ms, search"
                    hx-target="#pick-results" hx-swap="innerHTML";
                div id="pick-results" aria-live="polite" {}
            }
        }
    }
}

pub(super) async fn results(
    State(s): State<AppState>,
    staff: CurrentStaff,
    Query(q): Query<PickQuery>,
) -> Response {
    let v = View::new(&s, &staff);
    if !staff.can(VIEW) {
        return StatusCode::FORBIDDEN.into_response();
    }
    let query: String = q.q.trim().chars().take(MAX_QUERY).collect();
    if query.is_empty() {
        return Html(String::new()).into_response();
    }
    let found = match search(&s.services, &query, 1).await {
        Ok(f) => f,
        Err(e) => return v.error(&e),
    };
    let shown = html! {
        @if found.rows.is_empty() {
            p class="muted" { (v.t("announcements.form.pick_none")) }
        } @else {
            ul class="list" {
                @for r in &found.rows {
                    @let vals = serde_json::json!({ "add": r.code }).to_string();
                    li {
                        strong { (r.code) } " " (r.name) " "
                        @for (kind, status) in &r.links {
                            span class="chip" { (channel_label(kind)) " " (status) } " "
                        }
                        @if r.status == "left" { span class="chip" { (v.t("kernel.students.status_left")) } " " }
                        button type="button" class="secondary" hx-post=(PATH) hx-vals=(vals)
                            hx-include="closest form" hx-target="#hand-picked" hx-swap="outerHTML" {
                            (v.t("announcements.form.pick_add"))
                        }
                    }
                }
            }
            @if found.total > found.rows.len() as i64 {
                p class="muted" { (v.t("announcements.form.pick_more")) }
            }
        }
    };
    Html(shown.into_string()).into_response()
}

/// Adds `add` to the posted list; the response is the new field.
pub(super) async fn add(
    State(s): State<AppState>,
    staff: CurrentStaff,
    Form(pairs): Form<Vec<(String, String)>>,
) -> Response {
    if !staff.can(VIEW) {
        return StatusCode::FORBIDDEN.into_response();
    }
    let v = View::new(&s, &staff);
    let mut f = Fields::from_pairs(&pairs);
    let ids = pairs
        .iter()
        .filter(|(k, _)| k == "add")
        .map(|(_, id)| id.as_str());
    f.students = parse_student_ids(&format!(
        "{}\n{}",
        f.students,
        ids.collect::<Vec<_>>().join("\n")
    ))
    .join("\n");
    let mut resp = Html(field(&v, &f).into_string()).into_response();
    resp.headers_mut()
        .insert("HX-Trigger", HeaderValue::from_static("students-changed"));
    resp
}

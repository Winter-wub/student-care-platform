//! `GET /admin/students`: the roster, 50 per page, searchable by name or
//! code, with a filter for Students with missing data (`?missing=1`, #71)
//! and a badge on each of them. An HTMX request (the search box, `hx-get` on input) gets only the
//! `#student-results` fragment. Cards below 900 px, a table from 900 px
//! (`table.rows`).

use axum::extract::{Query, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::{Html, IntoResponse, Response};
use maud::{Markup, html};
use serde::Deserialize;

use super::missing::Missing;
use super::read::{self, Found};
use super::view::{View, is_htmx};
use crate::config::AttributeSpec;
use crate::{AppState, CurrentStaff};

/// `?q=..&page=..`.
#[derive(Debug, Default, Deserialize)]
pub(super) struct ListQuery {
    #[serde(default)]
    q: String,
    #[serde(default)]
    page: String,
    /// `1` lists only Students with missing data.
    #[serde(default)]
    missing: String,
}

pub(super) async fn page(
    State(s): State<AppState>,
    staff: CurrentStaff,
    headers: HeaderMap,
    Query(q): Query<ListQuery>,
) -> Response {
    let v = View::new(&s, &staff);
    let query = q.q.trim().chars().take(100).collect::<String>();
    let page = q.page.parse::<i64>().unwrap_or(1);
    let missing = matches!(q.missing.as_str(), "1" | "on" | "true");
    let found =
        match read::search_with(&s.services, &query, page, Some(&s.config.roster), missing).await {
            Ok(r) => r,
            Err(e) => return v.error(&e),
        };
    let results = results(&v, &query, missing, &found);
    // A history restore after a cache miss also carries HX-Request but
    // swaps the whole body: it gets the full page.
    let restore = headers.contains_key("hx-history-restore-request");
    let mut resp = if is_htmx(&headers) && !restore {
        (StatusCode::OK, Html(results.into_string())).into_response()
    } else {
        full(&v, &query, missing, results)
    };
    resp.headers_mut()
        .insert(header::VARY, HeaderValue::from_static("HX-Request"));
    resp
}

fn full(v: &View<'_>, query: &str, missing: bool, results: Markup) -> Response {
    let title = v.t("kernel.students.title");
    let body = html! {
        h1 { (title) }
        form method="get" action="/admin/students" class="card search" role="search" {
            label for="student-q" { (v.t("kernel.students.search")) }
            input id="student-q" type="search" name="q" value=(query) autocomplete="off"
                hx-get="/admin/students" hx-trigger="input changed delay:250ms, search"
                hx-target="#student-results" hx-swap="outerHTML" hx-push-url="true"
                hx-include="closest form";
            label class="check" {
                input type="checkbox" name="missing" value="1" checked[missing]
                    hx-get="/admin/students" hx-trigger="change" hx-include="closest form"
                    hx-target="#student-results" hx-swap="outerHTML" hx-push-url="true";
                " " (v.t("kernel.students.missing_filter"))
            }
        }
        (results)
    };
    v.page(StatusCode::OK, &title, body)
}

/// The fragment HTMX swaps: count, rows, pager.
fn results(v: &View<'_>, query: &str, missing: bool, found: &Found) -> Markup {
    let (rows, total, page, pages) = (&found.rows, found.total, found.page, found.pages);
    let attrs: Vec<(&str, &AttributeSpec)> =
        v.s.config
            .roster
            .attributes_in_order()
            .into_iter()
            .filter(|(_, a)| a.shown_in_list)
            .collect();
    let kinds = v.s.deployment.channel_kinds();
    let href = |p: i64| {
        let q = super::form::urlencode(query);
        let m = if missing { "&missing=1" } else { "" };
        format!("/admin/students?q={q}{m}&page={p}")
    };
    let import_label = v.t("kernel.empty.students_action");
    let import = v
        .staff
        .can("kernel.roster.import")
        .then_some((import_label.as_str(), "/admin/roster"));
    html! {
        div id="student-results" {
            p class="muted count" { (v.t("kernel.students.total")) " " (total) }
            @if rows.is_empty() && query.trim().is_empty() && !missing {
                // Nobody yet (#127): the roster brings them.
                (crate::ui::empty_state(&v.t("kernel.empty.students"), import))
            } @else if rows.is_empty() {
                (crate::ui::no_match(&v.t("kernel.empty.students_no_match"), (&v.t("kernel.empty.clear_search"), "/admin/students")))
            } @else {
                // Columns grow with the roster's attributes: a wide table
                // scrolls inside this region, never the page (#129).
                div class="table-scroll" role="region" tabindex="0" aria-label=(v.t("kernel.students.title")) {
                table class="rows students" {
                    thead { tr {
                        th { (v.t("kernel.students.name")) }
                        th { (v.t("kernel.students.code")) }
                        @for (k, a) in &attrs { th { (v.label(k, a)) } }
                        th { (v.t("kernel.students.links")) }
                    } }
                    tbody {
                        @for r in rows {
                            tr {
                                td data-label=(v.t("kernel.students.name")) {
                                    a class="name" href=(format!("/admin/students/{}", r.id)) { (r.name) }
                                    @if r.status == "left" { " " (v.status_chip(&r.status)) }
                                    (missing_badge(v, &r.missing))
                                }
                                td class="code" data-label=(v.t("kernel.students.code")) { (r.code) }
                                @for (k, a) in &attrs {
                                    td data-label=(v.label(k, a)) {
                                        (v.shown(a, r.attributes.get(*k)))
                                    }
                                }
                                td class="icons" data-label=(v.t("kernel.students.links")) {
                                    @for kind in &kinds {
                                        @let st = r.links.iter().find(|(c, _)| c == kind.as_str());
                                        (v.link_icon(kind.as_str(), st.map(|(_, s)| s.as_str())))
                                        " "
                                    }
                                }
                            }
                        }
                    }
                }
                }
            }
            @if pages > 1 {
                nav class="pager" aria-label=(v.t("kernel.students.page")) {
                    @if page > 1 {
                        a class="btn secondary" rel="prev" href=(href(page - 1)) { (v.t("kernel.students.prev")) }
                    }
                    span class="muted" { (v.t("kernel.students.page")) " " (page) " / " (pages) }
                    @if page < pages {
                        a class="btn secondary" rel="next" href=(href(page + 1)) { (v.t("kernel.students.next")) }
                    }
                }
            }
        }
    }
}

/// The badge of a Student with missing data and what is missing; nothing
/// for a complete Student.
fn missing_badge(v: &View<'_>, missing: &[Missing]) -> Markup {
    if missing.is_empty() {
        return html! {};
    }
    let attrs = &v.s.config.roster.attributes;
    let what: Vec<String> = missing
        .iter()
        .map(|m| match m {
            Missing::NoEmail => v.t("kernel.students.missing_no_email"),
            Missing::InvalidEmail => v.t("kernel.students.missing_bad_email"),
            Missing::Attribute(k) => attrs
                .get(k)
                .map_or(k.clone(), |a| v.label(k, a).to_string()),
        })
        .collect();
    html! {
        " " span class="chip warn" data-missing=(what.len()) { (v.t("kernel.students.missing_badge")) }
        " " span class="muted" { (what.join(", ")) }
    }
}

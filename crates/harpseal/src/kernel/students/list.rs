//! `GET /admin/students`: the roster, 50 per page, searchable by name or
//! code. An HTMX request (the search box, `hx-get` on input) gets only the
//! `#student-results` fragment. Cards below 900 px, a table from 900 px
//! (`table.rows`).

use axum::extract::{Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{Html, IntoResponse, Response};
use maud::{Markup, html};
use serde::Deserialize;

use super::read::{self, ListRow, PAGE_SIZE};
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
}

pub(super) async fn page(
    State(s): State<AppState>,
    staff: CurrentStaff,
    headers: HeaderMap,
    Query(q): Query<ListQuery>,
) -> Response {
    let v = View::new(&s, &staff);
    let query = q.q.trim().chars().take(100).collect::<String>();
    let page = q.page.parse::<i64>().unwrap_or(1).max(1);
    let (rows, total) = match read::search(&s.services, &query, page).await {
        Ok(r) => r,
        Err(e) => return v.error(&e),
    };
    let results = results(&v, &query, page, &rows, total);
    if is_htmx(&headers) {
        return (StatusCode::OK, Html(results.into_string())).into_response();
    }
    let title = v.t("kernel.students.title");
    let body = html! {
        h1 { (title) }
        form method="get" action="/admin/students" class="card search" role="search" {
            label for="student-q" { (v.t("kernel.students.search")) }
            input id="student-q" type="search" name="q" value=(query) autocomplete="off"
                hx-get="/admin/students" hx-trigger="input changed delay:250ms, search"
                hx-target="#student-results" hx-swap="outerHTML" hx-push-url="true";
        }
        (results)
    };
    v.page(StatusCode::OK, &title, body)
}

/// The fragment HTMX swaps: count, rows, pager.
fn results(v: &View<'_>, query: &str, page: i64, rows: &[ListRow], total: i64) -> Markup {
    let attrs: Vec<(&str, &AttributeSpec)> =
        v.s.config
            .roster
            .attributes_in_order()
            .into_iter()
            .filter(|(_, a)| a.shown_in_list)
            .collect();
    let kinds = v.s.deployment.channel_kinds();
    let pages = (total + PAGE_SIZE - 1) / PAGE_SIZE;
    let href = |p: i64| {
        let q = super::form::urlencode(query);
        format!("/admin/students?q={q}&page={p}")
    };
    html! {
        div id="student-results" {
            p class="muted count" { (v.t("kernel.students.total")) " " (total) }
            @if rows.is_empty() {
                div class="card" { p { (v.t("kernel.students.none")) } }
            } @else {
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
                                }
                                td class="code" data-label=(v.t("kernel.students.code")) { (r.code) }
                                @for (k, a) in &attrs {
                                    td data-label=(v.label(k, a)) {
                                        (AttributeSpec::display(r.attributes.get(*k)))
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

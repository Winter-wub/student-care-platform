//! `GET /admin/announcements`: newest first, 50 per page.

use axum::extract::{Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::Response;
use maud::{Markup, html};
use serde::Deserialize;

use super::store::{self, Listed};
use super::view::{View, fragment_or_page};
use crate::audience::Audience;
use crate::{AppState, CurrentStaff};

#[derive(Debug, Default, Deserialize)]
pub(super) struct ListQuery {
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
    let page = q.page.parse::<i64>().unwrap_or(1);
    let (rows, page, pages) = match store::list(&s.services, page).await {
        Ok(r) => r,
        Err(e) => return v.error(&e),
    };
    let results = results(&v, &rows, page, pages);
    fragment_or_page(&headers, results, |results| {
        let title = v.t("announcements.nav");
        let body = html! {
            div class="head-row" {
                h1 { (title) }
                @if staff.can("announcements.send") {
                    div class="actions" {
                        a class="btn" href="/admin/announcements/new" { (v.t("announcements.page.new")) }
                        a class="btn secondary" href=(super::page_saved::LIST) { (v.t("announcements.saved.title")) }
                    }
                }
            }
            (results)
        };
        v.page(StatusCode::OK, &title, body)
    })
}

/// A short description of an Audience, in the words Staff read: each
/// attribute by its label ("ชั้นปี: 1", not `year: 1`) and each value by
/// its label (#125).
pub(crate) fn audience_summary(v: &View<'_>, a: &Audience) -> String {
    let mut parts: Vec<String> = a
        .attributes
        .iter()
        .map(|(k, vals)| {
            let (locale, default) = (&v.staff.locale, &v.s.config.default_locale);
            let spec = v.s.config.roster.attributes.get(k);
            let name = spec.map_or(k.as_str(), |s| s.label_in(k, locale, default));
            let shown: Vec<&str> = vals
                .iter()
                .map(|x| spec.map_or(x.as_str(), |s| s.value_label(x, locale, default)))
                .collect();
            format!("{name}: {}", shown.join(" / "))
        })
        .collect();
    if !a.tags.is_empty() {
        parts.push(format!(
            "{}: {}",
            v.t("announcements.form.tags"),
            a.tags.join(" / ")
        ));
    }
    if !a.students.is_empty() {
        parts.push(format!(
            "{}: {}",
            v.t("announcements.form.hand_picked"),
            crate::ui::number(a.students.len())
        ));
    }
    if parts.is_empty() {
        parts.push(v.t("announcements.form.everyone"));
    }
    if a.include_unreachable {
        parts.push(v.t("announcements.form.include_unreachable"));
    }
    parts.join(" · ")
}

fn results(v: &View<'_>, rows: &[Listed], page: i64, pages: i64) -> Markup {
    let href = |p: i64| format!("/admin/announcements?page={p}");
    let write_label = v.t("kernel.empty.announcements_action");
    let write = v
        .staff
        .can(super::SEND_PERMISSION)
        .then_some((write_label.as_str(), "/admin/announcements/new"));
    html! {
        div id="announcement-results" {
            @if rows.is_empty() {
                // Nothing yet (#127): what goes here, and the way to start.
                (crate::ui::empty_state(&v.t("kernel.empty.announcements"), write))
            } @else {
                table class="rows announcements" {
                    thead { tr {
                        th { (v.t("announcements.form.title")) }
                        th { (v.t("announcements.page.status")) }
                        th { (v.t("announcements.page.time")) }
                        th { (v.t("announcements.form.audience")) }
                        th { (v.t("announcements.page.acked")) }
                    } }
                    tbody {
                        @for l in rows {
                            tr {
                                td data-label=(v.t("announcements.form.title")) {
                                    a class="name" href=(format!("/admin/announcements/{}", l.row.id)) { (l.title) }
                                }
                                td data-label=(v.t("announcements.page.status")) { (v.status_chip(&l.row.status)) (v.expired_chip(&l.row)) }
                                td data-label=(v.t("announcements.page.time")) {
                                    (v.time(l.row.sent_at.or(l.row.send_at).or(Some(l.row.created_at))))
                                }
                                td data-label=(v.t("announcements.form.audience")) { (audience_summary(v, &l.row.audience)) }
                                td data-label=(v.t("announcements.page.acked")) { (crate::ui::number(l.acked)) " / " (crate::ui::number(l.students)) }
                            }
                        }
                    }
                }
            }
            @if pages > 1 {
                nav class="pager" aria-label=(v.t("announcements.page.page")) {
                    @if page > 1 { a class="btn secondary" rel="prev" href=(href(page - 1)) { "‹" } }
                    span class="muted" { (v.t("announcements.page.page")) " " (page) " / " (pages) }
                    @if page < pages { a class="btn secondary" rel="next" href=(href(page + 1)) { "›" } }
                }
            }
        }
    }
}

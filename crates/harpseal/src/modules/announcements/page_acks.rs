//! `GET /admin/announcements/{id}/acks`: the acknowledgement history
//! (`0005_reack`). Each time Staff asked for a new acknowledgement, the
//! version it closed is listed, newest first: when and by whom it was
//! closed, acknowledged / in Audience, and its texts. `?round=<n>` lists
//! who acknowledged version `n` and when (50 per page, `&page=`). The
//! current version's counts are the detail page's.

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::Response;
use maud::{Markup, html};
use serde::Deserialize;
use serde_json::Value as Json;

use super::stats::stats;
use super::store::{self, Row};
use super::view::View;
use crate::kernel::text::fill;
use crate::port_types::Statement;
use crate::{AppState, CurrentStaff, Result, Services};

const PAGE: i64 = 50;

#[derive(Debug, Default, Deserialize)]
pub(super) struct AcksQuery {
    #[serde(default)]
    round: String,
    #[serde(default)]
    page: String,
}

/// One closed version.
struct Closed {
    round: i64,
    closed_at: i64,
    closed_by: String,
    /// (locale, title, body).
    texts: Vec<(String, String, String)>,
    students: i64,
    acked: i64,
}

/// One acknowledgement of a closed version.
struct Acked {
    student_id: String,
    code: String,
    name: String,
    acked_at: i64,
}

pub(super) async fn page(
    State(s): State<AppState>,
    staff: CurrentStaff,
    Path(id): Path<String>,
    Query(q): Query<AcksQuery>,
) -> Response {
    let v = View::new(&s, &staff);
    let svc = &s.services;
    let row = match store::load(svc, &id).await {
        Ok(Some(r)) => r,
        Ok(None) => return v.not_found(),
        Err(e) => return v.error(&e),
    };
    let loaded = async {
        Ok::<_, crate::Error>((
            store::texts(svc, &id).await?,
            stats(&s.deployment, &id).await?,
            closed(svc, &id).await?,
        ))
    };
    let (texts, st, rounds) = match loaded.await {
        Ok(x) => x,
        Err(e) => return v.error(&e),
    };
    let chosen = q
        .round
        .parse::<i64>()
        .ok()
        .and_then(|r| rounds.iter().find(|c| c.round == r));
    let list = match chosen {
        Some(c) => match acked(svc, &id, c.round, q.page.parse().unwrap_or(1), c.acked).await {
            Ok(l) => Some((c, l)),
            Err(e) => return v.error(&e),
        },
        None => None,
    };
    let title = texts.pick(&staff.locale).0.to_string();
    let heading = v.t("announcements.acks.title");
    let base = format!("/admin/announcements/{id}");
    let body = html! {
        p { a href=(base) { "‹ " (title) } }
        h1 { (heading) }
        p class="muted" { (v.t("announcements.acks.help")) }
        div class="card" {
            h2 { (version_name(&v, row.ack_round)) " " span class="chip ok" { (v.t("announcements.acks.current")) } }
            dl class="facts" {
                dt { (v.t("announcements.page.acked")) } dd { strong { (st.acked) " / " (st.in_audience) } }
            }
        }
        @if let Some((c, (rows, page, pages))) = &list {
            (acked_list(&v, &row, c, rows, *page, *pages))
        }
        @if rounds.is_empty() {
            p class="muted" { (v.t("announcements.acks.none")) }
        }
        @for c in &rounds {
            (closed_card(&v, &row, c))
        }
    };
    v.page(StatusCode::OK, &heading, body)
}

/// "ฉบับที่ n" for acknowledgement version `round` (0-based).
pub(super) fn version_name(v: &View<'_>, round: i64) -> String {
    fill(
        &v.t("announcements.acks.version"),
        &[("n", &(round + 1).to_string())],
    )
}

fn closed_card(v: &View<'_>, row: &Row, c: &Closed) -> Markup {
    let href = format!("/admin/announcements/{}/acks?round={}", row.id, c.round);
    html! {
        div class="card" {
            h2 { (version_name(v, c.round)) }
            dl class="facts" {
                dt { (v.t("announcements.page.acked")) } dd { strong { (c.acked) " / " (c.students) } }
                dt { (v.t("announcements.acks.closed_at")) } dd { (v.time(Some(c.closed_at))) }
                dt { (v.t("announcements.acks.closed_by")) } dd { (if c.closed_by.is_empty() { "-" } else { c.closed_by.as_str() }) }
            }
            details {
                summary { (v.t("announcements.acks.texts")) }
                @for (locale, title, body) in &c.texts {
                    p class="muted" { (v.s.catalog.locale_name(&v.staff.locale, locale)) }
                    strong { (title) }
                    p class="pre" { (body) }
                }
            }
            @if c.acked > 0 {
                p { a class="btn secondary" href=(href) { (v.t("announcements.acks.who")) } }
            }
        }
    }
}

fn acked_list(
    v: &View<'_>,
    row: &Row,
    c: &Closed,
    rows: &[Acked],
    page: i64,
    pages: i64,
) -> Markup {
    let href = |p: i64| {
        format!(
            "/admin/announcements/{}/acks?round={}&page={p}",
            row.id, c.round
        )
    };
    html! {
        div class="card" id="acked" {
            h2 { (v.t("announcements.acks.who")) " · " (version_name(v, c.round)) }
            table class="rows" {
                thead { tr {
                    th { (v.t("announcements.page.student")) }
                    th { (v.t("announcements.acks.code")) }
                    th { (v.t("announcements.acks.acked_at")) }
                } }
                tbody {
                    @for r in rows {
                        tr {
                            td data-label=(v.t("announcements.page.student")) {
                                a href=(format!("/admin/students/{}", r.student_id)) { (r.name) }
                            }
                            td data-label=(v.t("announcements.acks.code")) { span class="id" { (r.code) } }
                            td data-label=(v.t("announcements.acks.acked_at")) { (v.time(Some(r.acked_at))) }
                        }
                    }
                }
            }
            @if pages > 1 {
                nav class="pager" {
                    @if page > 1 { a class="btn secondary" href=(href(page - 1)) { "‹" } }
                    span class="muted" { (page) " / " (pages) }
                    @if page < pages { a class="btn secondary" href=(href(page + 1)) { "›" } }
                }
            }
        }
    }
}

/// The closed versions of `id`, newest first.
async fn closed(s: &Services, id: &str) -> Result<Vec<Closed>> {
    let rows =
        s.db.query(
            Statement::new(
                "SELECT r.ack_round, r.closed_at, coalesce(st.display_name, '') AS closed_by, \
                 r.texts, r.students, r.acked FROM announcement_ack_round r \
                 LEFT JOIN staff st ON st.id = r.closed_by \
                 WHERE r.announcement_id = ? ORDER BY r.ack_round DESC",
            )
            .bind(id),
        )
        .await?;
    let n = |i: usize, c: &str| store::int(&rows, i, c).unwrap_or(0);
    Ok((0..rows.len())
        .map(|i| Closed {
            round: n(i, "ack_round"),
            closed_at: n(i, "closed_at"),
            closed_by: store::text(&rows, i, "closed_by"),
            texts: texts_of(&store::text(&rows, i, "texts")),
            students: n(i, "students"),
            acked: n(i, "acked"),
        })
        .collect())
}

/// `[{"locale", "title", "body"}]`, Thai first.
fn texts_of(json: &str) -> Vec<(String, String, String)> {
    let all: Vec<Json> = serde_json::from_str(json).unwrap_or_default();
    let get = |j: &Json, k: &str| {
        j.get(k)
            .and_then(Json::as_str)
            .unwrap_or_default()
            .to_string()
    };
    let mut out: Vec<(String, String, String)> = all
        .iter()
        .map(|j| (get(j, "locale"), get(j, "title"), get(j, "body")))
        .collect();
    out.sort_by_key(|(l, _, _)| (l != "th", l.clone()));
    out
}

/// Page `page` (clamped) of who acknowledged version `round`, by time.
async fn acked(
    s: &Services,
    id: &str,
    round: i64,
    page: i64,
    total: i64,
) -> Result<(Vec<Acked>, i64, i64)> {
    let pages = ((total + PAGE - 1) / PAGE).max(1);
    let page = page.clamp(1, pages);
    let rows =
        s.db.query(
            Statement::new(
                "SELECT h.student_id, st.student_code, st.display_name, h.acked_at \
                 FROM announcement_ack_history h JOIN student st ON st.id = h.student_id \
                 WHERE h.announcement_id = ? AND h.ack_round = ? \
                 ORDER BY h.acked_at, st.student_code LIMIT ? OFFSET ?",
            )
            .bind(id)
            .bind(round)
            .bind(PAGE)
            .bind((page - 1) * PAGE),
        )
        .await?;
    let t = |i: usize, c: &str| store::text(&rows, i, c);
    let list = (0..rows.len())
        .map(|i| Acked {
            student_id: t(i, "student_id"),
            code: t(i, "student_code"),
            name: t(i, "display_name"),
            acked_at: store::int(&rows, i, "acked_at").unwrap_or(0),
        })
        .collect();
    Ok((list, page, pages))
}

//! `GET /admin/announcements/{id}`: texts per Locale, the Audience, the
//! counts per round and Channel, acknowledged / in Audience, the actions,
//! and the deliveries (50 per page, filter by status; an HTMX request gets
//! the `#deliveries` fragment).

use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::Response;
use maud::{Markup, html};
use serde::Deserialize;

use super::page_list::audience_summary;
use super::stats::{Stats, stats};
use super::store::{self, Row, Texts};
use super::view::{View, fragment_or_page};
use crate::kernel::students::channel_label;
use crate::port_types::Statement;
use crate::{AppState, CurrentStaff, Result, Services};

const STATUSES: [&str; 4] = ["pending", "sent", "failed", "skipped"];

#[derive(Debug, Default, Deserialize)]
pub(super) struct DetailQuery {
    #[serde(default)]
    status: String,
    #[serde(default)]
    page: String,
}

pub(super) async fn page(
    State(s): State<AppState>,
    staff: CurrentStaff,
    headers: HeaderMap,
    Path(id): Path<String>,
    Query(q): Query<DetailQuery>,
) -> Response {
    let v = View::new(&s, &staff);
    let svc = &s.services;
    // A round whose Jobs all ended with rows still pending is finished here.
    if let Err(e) = super::sweep::sweep(svc, Some(&id)).await {
        return v.error(&e);
    }
    let row = match store::load(svc, &id).await {
        Ok(Some(r)) => r,
        Ok(None) => return v.not_found(),
        Err(e) => return v.error(&e),
    };
    let filter = STATUSES
        .contains(&q.status.as_str())
        .then_some(q.status.as_str());
    let page = q.page.parse::<i64>().unwrap_or(1);
    let loaded = async {
        Ok::<_, crate::Error>((
            store::texts(svc, &id).await?,
            stats(&s.deployment, &id).await?,
            deliveries(svc, &id, filter, page).await?,
            super::maint::in_progress(svc, &id).await?,
            super::files::list(svc, &id).await?,
        ))
    };
    let (texts, st, list, busy, images) = match loaded.await {
        Ok(x) => x,
        Err(e) => return v.error(&e),
    };
    let fragment = delivery_list(&v, &id, filter, &list);
    fragment_or_page(&headers, fragment, |fragment| {
        let title = texts.pick(&staff.locale).0.to_string();
        let body = html! {
            h1 {
                (title) " " (v.status_chip(&row.status)) (v.expired_chip(&row))
                // Edit, retract or notice Jobs still queued or running (#49).
                @if busy {
                    " " span class="chip" role="status" { (v.t("announcements.page.in_progress")) }
                }
            }
            (summary(&v, &row, &st))
            (hand_picked(&v, &row))
            (actions(&v, &row))
            (texts_card(&v, &texts))
            (images_card(&v, &images, &texts))
            (counts(&v, &st))
            (fragment)
        };
        v.page(StatusCode::OK, &title, body)
    })
}

/// A plain preview of the Card as a Student sees it.
pub(crate) fn preview(v: &View<'_>, title: &str, body: &str, ack: bool) -> Markup {
    // As the chat Card shows it (#108): plain text, the first links as
    // URL buttons.
    let doc = crate::rich_text::parse(body);
    html! {
        div class="card preview" aria-label=(v.t("announcements.form.preview")) {
            p class="muted" { (v.t("announcements.form.preview")) }
            div class="bubble" {
                @if title.trim().is_empty() && body.trim().is_empty() {
                    // The form before anything is typed (#127).
                    p class="preview-placeholder" { (v.t("announcements.editor.card_placeholder")) }
                } @else {
                    strong { (title) }
                    p { (doc.plain()) }
                }
                @if ack { span class="btn secondary" { (v.t("announcements.card.ack")) } " " }
                span class="btn secondary" { (v.t("announcements.card.open")) }
                @for l in doc.first_links(super::chat::URL_BUTTONS) {
                    " " span class="btn secondary" title=(l.url) { (l.text) }
                }
            }
        }
    }
}

fn summary(v: &View<'_>, row: &Row, st: &Stats) -> Markup {
    html! {
        div class="card" {
            dl class="facts" {
                dt { (v.t("announcements.form.audience")) } dd { (audience_summary(v, &row.audience)) }
                dt { (v.t("announcements.page.acked")) } dd { strong { (st.acked) " / " (st.in_audience) } }
                dt { (v.t("announcements.form.require_ack")) }
                dd { (v.t(if row.require_ack { "announcements.page.yes" } else { "announcements.page.no" })) }
                dt { (v.t("announcements.page.send_at")) } dd { (v.time(row.send_at)) }
                dt { (v.t("announcements.page.sent_at")) } dd { (v.time(row.sent_at)) }
                dt { (v.t("announcements.form.expires")) } dd { (v.time(row.expires_at)) }
                dt { (v.t("announcements.page.round")) } dd { (row.round + 1) }
                @if row.retracted_at.is_some() {
                    dt { (v.t("announcements.status.retracted")) } dd { (v.time(row.retracted_at)) }
                }
                @if let Some(r) = &row.edit_report {
                    dt { (v.t("announcements.page.edited")) }
                    dd {
                        (r["edited"])
                        @if let Some(total) = r.get("total") { " / " (total) }
                        " · " (v.t("announcements.page.edit_failed")) " " (r["failed"])
                    }
                }
                // The last edit notice run (#77): sent / recipients.
                @if let Some(r) = &row.notice_report {
                    dt { (v.t("announcements.page.notice")) }
                    dd {
                        (r["sent"])
                        @if let Some(total) = r.get("total").filter(|t| !t.is_null()) { " / " (total) }
                        " · " (v.t("announcements.page.edit_failed")) " " (r["failed"])
                    }
                }
            }
        }
    }
}

/// The ids Staff hand-picked, as saved with the Audience.
fn hand_picked(v: &View<'_>, row: &Row) -> Markup {
    let ids = &row.audience.students;
    html! {
        @if !ids.is_empty() {
            details class="card" id="hand-picked-ids" {
                summary { (v.t("announcements.form.hand_picked")) " (" (ids.len()) ")" }
                p class="pre" { (ids.join(", ")) }
            }
        }
    }
}

fn post_button(v: &View<'_>, row: &Row, path: &str, key: &str, secondary: bool) -> Markup {
    html! {
        form method="post" action=(format!("/admin/announcements/{}/{path}", row.id)) class="inline" {
            (v.staff.csrf_field())
            input type="hidden" name="version" value=(row.version);
            button type="submit" class=[secondary.then_some("secondary")] { (v.t(key)) }
        }
    }
}

fn actions(v: &View<'_>, row: &Row) -> Markup {
    let send = v.staff.can("announcements.send");
    let retract = v.staff.can("announcements.retract");
    let st = row.status.as_str();
    html! {
        div class="actions" {
            @if send && matches!(st, "draft" | "scheduled" | "sent") {
                a class="btn secondary" href=(format!("/admin/announcements/{}/edit", row.id)) { (v.t("announcements.action.edit")) }
            }
            // #120: "ยืนยันส่ง" opens the review step first.
            @if send && st == "draft" {
                a class="btn" href=(super::page_review::path(&row.id)) { (v.t("announcements.action.confirm")) }
            }
            @if send && st == "scheduled" { (post_button(v, row, "cancel", "announcements.action.cancel", true)) }
            // An expired one is not resent (#77): extend the expiry first.
            @if send && st == "sent" && super::notice::open(row, v.s.services.now_ms()) {
                (post_button(v, row, "resend", "announcements.action.resend", true))
            }
            @if retract && matches!(st, "scheduled" | "sending" | "sent") {
                (post_button(v, row, "retract", "announcements.action.retract", true))
            }
            a class="btn secondary" href="/admin/announcements" { (v.t("announcements.page.back")) }
        }
    }
}

/// The saved images (#109) and PDFs (#113), through the back-office
/// route: Staff see them whatever the status (a draft, a retracted one).
fn images_card(v: &View<'_>, files: &[super::files::Stored], texts: &Texts) -> Markup {
    let bodies: Vec<&str> = texts.0.values().map(|(_, b)| b.as_str()).collect();
    let label = v.t("announcements.images.slot");
    let images: Vec<&super::files::Stored> = files
        .iter()
        .filter(|f| f.kind == super::files::IMAGE)
        .collect();
    html! {
        @if super::files::pdfs(files).next().is_some() {
            div class="card" {
                p class="muted" { (v.t("announcements.pdfs.legend")) }
                (super::files_pdf::list_html(files, true))
            }
        }
        @if !images.is_empty() {
            div class="card" {
                p class="muted" { (v.t("announcements.images.legend")) }
                (super::form_images::hero_warning(v, files, &bodies))
                @for f in images {
                    figure {
                        (f.img(true).markup(&format!("{label} {}", f.n)))
                        figcaption class="muted" {
                            (label) " " (f.n) " · ![…](image:" (f.n) ") · "
                            (f.size_bytes / 1024) " KB"
                        }
                    }
                }
            }
        }
    }
}

fn texts_card(v: &View<'_>, texts: &Texts) -> Markup {
    html! {
        div class="card" {
            @for (locale, (title, body)) in &texts.0 {
                section lang=(locale) {
                    p class="muted" { (v.s.catalog.locale_name(&v.staff.locale, locale)) }
                    strong { (title) }
                    p class="pre" { (body) }
                }
            }
        }
    }
}

fn counts(v: &View<'_>, st: &Stats) -> Markup {
    html! {
        @if !st.rounds.is_empty() {
            table class="rows" {
                thead { tr {
                    th { (v.t("announcements.page.round")) } th { (v.t("announcements.page.channel")) }
                    @for s in STATUSES { th { (v.t(&format!("announcements.delivery.{s}"))) } }
                } }
                tbody {
                    @for r in &st.rounds {
                        tr {
                            td data-label=(v.t("announcements.page.round")) { (r.round + 1) }
                            td data-label=(v.t("announcements.page.channel")) { (channel_label(&r.channel)) }
                            td data-label=(v.t("announcements.delivery.pending")) { (r.pending) }
                            td data-label=(v.t("announcements.delivery.sent")) { (r.sent) }
                            td data-label=(v.t("announcements.delivery.failed")) { (r.failed) }
                            td data-label=(v.t("announcements.delivery.skipped")) { (r.skipped) }
                        }
                    }
                }
            }
        }
    }
}

/// One delivery row as listed.
struct DeliveryRow {
    student_id: String,
    name: String,
    channel: String,
    round: i64,
    status: String,
    note: String,
    acked_at: Option<i64>,
}

/// One page of deliveries.
struct Deliveries {
    rows: Vec<DeliveryRow>,
    page: i64,
    pages: i64,
}

const PAGE: i64 = 50;

async fn deliveries(s: &Services, id: &str, status: Option<&str>, page: i64) -> Result<Deliveries> {
    let filter = if status.is_some() {
        " AND d.status = ?"
    } else {
        ""
    };
    let bind = |st: Statement| match status {
        Some(x) => st.bind(id).bind(x),
        None => st.bind(id),
    };
    let total =
        s.db.query(bind(Statement::new(format!(
            "SELECT count(*) AS n FROM announcement_delivery d WHERE d.announcement_id = ?{filter}"
        ))))
        .await?;
    let pages = ((store::int(&total, 0, "n").unwrap_or(0) + PAGE - 1) / PAGE).max(1);
    let page = page.clamp(1, pages);
    let rows =
        s.db.query(
            bind(Statement::new(format!(
                "SELECT d.student_id, st.display_name, d.channel, d.round, d.status, \
                 coalesce(d.error, d.removal, '') AS note, d.acked_at FROM announcement_delivery d \
                 JOIN student st ON st.id = d.student_id WHERE d.announcement_id = ?{filter} \
                 ORDER BY d.round, d.id LIMIT ? OFFSET ?"
            )))
            .bind(PAGE)
            .bind((page - 1) * PAGE),
        )
        .await?;
    let t = |i: usize, c: &str| store::text(&rows, i, c);
    Ok(Deliveries {
        rows: (0..rows.len())
            .map(|i| DeliveryRow {
                student_id: t(i, "student_id"),
                name: t(i, "display_name"),
                channel: t(i, "channel"),
                round: store::int(&rows, i, "round").unwrap_or(0) + 1,
                status: t(i, "status"),
                note: t(i, "note"),
                acked_at: store::int(&rows, i, "acked_at"),
            })
            .collect(),
        page,
        pages,
    })
}

fn delivery_list(v: &View<'_>, id: &str, filter: Option<&str>, d: &Deliveries) -> Markup {
    let base = format!("/admin/announcements/{id}");
    let href = |st: Option<&str>, p: i64| format!("{base}?status={}&page={p}", st.unwrap_or(""));
    html! {
        div id="deliveries" {
            h2 { (v.t("announcements.page.deliveries")) }
            nav class="chips" {
                a class=(if filter.is_none() { "chip ok" } else { "chip" }) href=(href(None, 1))
                    hx-get=(href(None, 1)) hx-target="#deliveries" hx-swap="outerHTML" hx-push-url="true" {
                    (v.t("announcements.page.all"))
                }
                @for s in STATUSES {
                    a class=(if filter == Some(s) { "chip ok" } else { "chip" }) href=(href(Some(s), 1))
                        hx-get=(href(Some(s), 1)) hx-target="#deliveries" hx-swap="outerHTML" hx-push-url="true" {
                        (v.t(&format!("announcements.delivery.{s}")))
                    }
                }
            }
            table class="rows" {
                tbody {
                    @for r in &d.rows {
                        tr {
                            td data-label=(v.t("announcements.page.student")) {
                                a href=(format!("/admin/students/{}", r.student_id)) { (r.name) }
                            }
                            td data-label=(v.t("announcements.page.channel")) { (channel_label(&r.channel)) " #" (r.round) }
                            td data-label=(v.t("announcements.page.status")) {
                                (v.t(&format!("announcements.delivery.{}", r.status))) " " span class="muted" { (r.note) }
                            }
                            td data-label=(v.t("announcements.page.acked")) {
                                @if r.acked_at.is_some() { (v.time(r.acked_at)) }
                            }
                        }
                    }
                }
            }
            @if d.pages > 1 {
                nav class="pager" {
                    @if d.page > 1 { a class="btn secondary" href=(href(filter, d.page - 1)) { "‹" } }
                    span class="muted" { (d.page) " / " (d.pages) }
                    @if d.page < d.pages { a class="btn secondary" href=(href(filter, d.page + 1)) { "›" } }
                }
            }
        }
    }
}

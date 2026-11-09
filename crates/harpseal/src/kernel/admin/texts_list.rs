//! The `/admin/texts` list (#125): every text per Locale with its source,
//! grouped and labelled by the page it shows on, searchable by text, page
//! or key, and filtered by page. The key itself is only on the edit page's
//! technical details.

use axum::extract::{Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::Response;
use maud::{Markup, html};
use serde::Deserialize;

use super::settings::{self, TextRow};
use super::texts_areas::{Area, area};
use super::texts_pages::{consent_card, source_chip};
use super::view::{View, cut};
use crate::{AppState, CurrentStaff};

/// Longest preview of a text in the list, in characters.
const PREVIEW_CHARS: usize = 120;

/// `?q=&area=`.
#[derive(Debug, Default, Deserialize)]
pub(super) struct Search {
    #[serde(default)]
    q: String,
    /// An [`Area`] id; empty for every page.
    #[serde(default)]
    area: String,
}

/// One text in the list: its key, where it shows, one row per Locale.
type Listed = (String, Area, Vec<TextRow>);

/// One text of a group: its key and one row per Locale.
type Row = (String, Vec<TextRow>);

fn preview(t: &str) -> String {
    let mut p: String = t.chars().take(PREVIEW_CHARS).collect();
    if t.chars().count() > PREVIEW_CHARS {
        p.push('…');
    }
    p
}

pub(super) async fn list(
    State(s): State<AppState>,
    staff: CurrentStaff,
    headers: HeaderMap,
    Query(q): Query<Search>,
) -> Response {
    let v = View::new(&s, &staff);
    let query = cut(&q.q, 100);
    let loaded = async {
        Ok::<_, crate::Error>((
            settings::settings(&s.services).await?,
            settings::stored_version(&s.services).await?,
        ))
    };
    let (stored, version) = match loaded.await {
        Ok(r) => r,
        Err(e) => return v.error(&e),
    };
    let locales = settings::locales(&s.catalog, &s.config);
    let needle = query.to_lowercase();
    let all: Vec<Listed> = settings::keys(&s.catalog)
        .into_iter()
        .map(|k| {
            let per: Vec<TextRow> = locales
                .iter()
                .map(|l| settings::row(&s.catalog, &s.config, &stored, &k, l))
                .collect();
            (k.clone(), area(&v, &k), per)
        })
        .collect();
    let mut areas: Vec<Area> = all.iter().map(|(_, a, _)| a.clone()).collect();
    areas.sort_by(|a, b| a.name.cmp(&b.name));
    areas.dedup();
    let rows: Vec<Listed> = all
        .into_iter()
        .filter(|(_, a, _)| q.area.is_empty() || a.id == q.area)
        .filter(|(k, a, per)| {
            needle.is_empty()
                || k.to_lowercase().contains(&needle)
                || a.name.to_lowercase().contains(&needle)
                || per
                    .iter()
                    .any(|r| r.effective().to_lowercase().contains(&needle))
        })
        .collect();
    let results = results(&v, &locales, rows);
    v.listing(&headers, results, |results| {
        let title = v.t("kernel.admin.texts.title");
        let hx = "input changed delay:250ms from:#texts-q, search from:#texts-q, change from:#texts-area";
        let body = html! {
            h1 { (title) }
            p class="muted" { (v.t("kernel.admin.texts.intro")) }
            (consent_card(&v, version.as_deref()))
            form method="get" action="/admin/texts" class="card search" role="search"
                hx-get="/admin/texts" hx-trigger=(hx) hx-target="#texts-results"
                hx-swap="outerHTML" hx-push-url="true" {
                div class="row2" {
                    div class="field" {
                        label for="texts-q" { (v.t("kernel.admin.texts.search")) }
                        input id="texts-q" type="search" name="q" value=(query) autocomplete="off";
                    }
                    div class="field" {
                        label for="texts-area" { (v.t("kernel.admin.texts.area")) }
                        select id="texts-area" name="area" {
                            option value="" { (v.t("kernel.admin.texts.all_areas")) }
                            @for a in &areas {
                                option value=(a.id) selected[a.id == q.area] { (a.name) }
                            }
                        }
                    }
                }
                button class="secondary" type="submit" { (v.t("kernel.admin.texts.find")) }
            }
            (results)
            (crate::ui::technical(v.s, v.staff, html! { p { (v.t("kernel.admin.texts.tech_toml")) } }))
        };
        v.page(StatusCode::OK, &title, body)
    })
}

/// The texts grouped by page, each group a table with one column per
/// Locale; the first Locale's text links to the edit page.
fn results(v: &View<'_>, locales: &[String], mut rows: Vec<Listed>) -> Markup {
    rows.sort_by(|a, b| (&a.1.name, &a.0).cmp(&(&b.1.name, &b.0)));
    let mut groups: Vec<(Area, Vec<Row>)> = Vec::new();
    let total = rows.len();
    for (key, a, per) in rows {
        match groups.last_mut() {
            Some((last, list)) if *last == a => list.push((key, per)),
            _ => groups.push((a, vec![(key, per)])),
        }
    }
    let name = |l: &str| v.s.catalog.locale_name(&v.staff.locale, l);
    html! {
        div id="texts-results" {
            p class="muted count" { (v.t("kernel.admin.total")) " " (crate::ui::number(total)) }
            @if groups.is_empty() {
                div class="card" { p { (v.t("kernel.admin.none")) } }
            }
            @for (a, list) in &groups {
                section class="text-group" data-area=(a.id) {
                    h2 { (a.name) " " span class="muted" { "(" (crate::ui::number(list.len())) ")" } }
                    table class="rows texts" {
                        thead { tr { @for l in locales { th { (name(l)) } } } }
                        tbody {
                            @for (key, per) in list {
                                tr {
                                    @for (i, r) in per.iter().enumerate() {
                                        td data-label=(name(&r.locale)) {
                                            @if i == 0 {
                                                a href=(format!("/admin/texts/{key}")) { (preview(r.effective())) }
                                            } @else {
                                                span { (preview(r.effective())) }
                                            }
                                            " " (source_chip(v, r))
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

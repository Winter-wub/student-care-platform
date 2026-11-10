//! The `/admin/texts` list (#125): every text per Locale with its source,
//! grouped and labelled by the page it shows on, searchable by text, page
//! or key, and filtered by page. The key itself is only on the edit page's
//! technical details.
//!
//! Each page's texts sit behind a disclosure (#142), closed on load. A
//! search or a page filter opens the groups that match, and so does a
//! saved edit that no longer passes [`settings::check`] against its
//! default (a placeholder the default gained or dropped since). The
//! result count is one polite live region that stays on the page: an
//! HTMX update swaps the list and sends the count out of band into it,
//! so it is announced once per update.

use axum::extract::{Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::Response;
use maud::{Markup, html};
use serde::Deserialize;

use super::settings::{self, TextRow};
use super::texts_areas::{Area, Areas};
use super::texts_pages::{consent_card, source_chip};
use super::view::{View, cut, enc};
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

/// A saved edit its default no longer accepts: it needs fixing.
fn to_fix(r: &TextRow) -> bool {
    r.setting
        .as_deref()
        .is_some_and(|s| settings::check(&r.base, s).is_err())
}

/// `key` (with `{n}`) filled with `n`.
fn counted(v: &View<'_>, key: &str, n: usize) -> String {
    v.t(key).replace("{n}", &crate::ui::number(n))
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
    let mut area = Areas::new(&v);
    let all: Vec<Listed> = settings::keys(&s.catalog)
        .into_iter()
        .map(|k| {
            let per: Vec<TextRow> = locales
                .iter()
                .map(|l| settings::row(&s.catalog, &s.config, &stored, &k, l))
                .collect();
            (k.clone(), area.of(&k), per)
        })
        .collect();
    let mut areas: Vec<Area> = all.iter().map(|(_, a, _)| a.clone()).collect();
    areas.sort_by(|a, b| (&a.name, &a.id).cmp(&(&b.name, &b.id)));
    areas.dedup_by(|a, b| a.id == b.id);
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
    let filtered = !needle.is_empty() || !q.area.is_empty();
    let total = rows.len();
    let count = if filtered {
        counted(&v, "kernel.admin.texts.count_found", total)
    } else {
        counted(&v, "kernel.admin.texts.count_all", total)
    };
    let results = results(&v, &locales, rows, filtered);
    // HTMX: the list, then the count into the live region already on
    // the page (its text only, so the region itself stays).
    let fragment = html! {
        (results)
        span id="texts-count" hx-swap-oob="innerHTML" { (count) }
    };
    v.listing(&headers, fragment, |_| {
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
                        input id="texts-q" type="search" name="q" value=(query) autocomplete="off"
                            aria-describedby="texts-count";
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
            p class="muted count" aria-live="polite" { span id="texts-count" { (count) } }
            (results)
            (crate::ui::technical(v.s, v.staff, html! { p { (v.t("kernel.admin.texts.tech_toml")) } }))
        };
        v.page(StatusCode::OK, &title, body)
    })
}

/// The texts grouped by page, each group a closed disclosure holding a
/// table with one column per Locale; the first Locale's text links to
/// the edit page. A group is open when `filtered` (it matched) or when
/// one of its edits needs fixing.
fn results(v: &View<'_>, locales: &[String], mut rows: Vec<Listed>, filtered: bool) -> Markup {
    rows.sort_by(|a, b| (&a.1.name, &a.1.id, &a.0).cmp(&(&b.1.name, &b.1.id, &b.0)));
    let mut groups: Vec<(Area, Vec<Row>)> = Vec::new();
    for (key, a, per) in rows {
        match groups.last_mut() {
            Some((last, list)) if last.id == a.id => list.push((key, per)),
            _ => groups.push((a, vec![(key, per)])),
        }
    }
    let name = |l: &str| v.s.catalog.locale_name(&v.staff.locale, l);
    html! {
        div id="texts-results" {
            @if groups.is_empty() {
                (crate::ui::no_match(&v.t("kernel.empty.filtered"), (&v.t("kernel.empty.clear_filters"), "/admin/texts")))
            }
            @for (a, list) in &groups {
                @let fix = list.iter().filter(|(_, per)| per.iter().any(to_fix)).count();
                details class="text-group" data-area=(a.id) open[filtered || fix > 0] {
                    summary {
                        svg class="icon fold" viewBox="0 0 20 20" aria-hidden="true" focusable="false" {
                            path d="M7 4l6 6-6 6" stroke="currentColor" stroke-width="2"
                                stroke-linecap="round" stroke-linejoin="round" fill="none" {}
                        }
                        h2 { (a.name) " " span class="muted" { "(" (crate::ui::number(list.len())) ")" } }
                        @if fix > 0 {
                            span class="chip err" { (counted(v, "kernel.admin.texts.to_fix", fix)) }
                        }
                    }
                    div class="text-group-body" {
                        table class="rows texts" {
                            thead { tr { @for l in locales { th { (name(l)) } } } }
                            tbody {
                                @for (key, per) in list {
                                    tr {
                                        @for (i, r) in per.iter().enumerate() {
                                            td data-label=(name(&r.locale)) {
                                                @if i == 0 {
                                                    a href=(format!("/admin/texts/{}", enc(key))) { (preview(r.effective())) }
                                                } @else {
                                                    span { (preview(r.effective())) }
                                                }
                                                " " (source_chip(v, r))
                                                @if to_fix(r) {
                                                    p class="field-error" { (v.t("kernel.admin.texts.to_fix_help")) }
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
    }
}

//! `/admin/texts` (`kernel.setting.edit`): every catalog key per Locale
//! with its effective text and source (default / TOML / setting),
//! searchable by key or text; `/admin/texts/{key}` edits one key per
//! Locale (save or reset to default); `POST /admin/texts/consent` sets the
//! Consent notice version.

use axum::Form;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::Response;
use maud::{Markup, html};
use serde::Deserialize;

use super::settings::{self, Refused, TextRow};
use super::texts_api::Written;
use super::view::{View, cut};
use crate::{Actor, AppState, CurrentStaff, RequestCtx};

/// Longest preview of a text in the list, in characters.
const PREVIEW_CHARS: usize = 120;

/// `?q=`.
#[derive(Debug, Default, Deserialize)]
pub(super) struct Search {
    #[serde(default)]
    q: String,
}

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
    let rows: Vec<(String, Vec<TextRow>)> = settings::keys(&s.catalog)
        .into_iter()
        .map(|k| {
            let per: Vec<TextRow> = locales
                .iter()
                .map(|l| settings::row(&s.catalog, &s.config, &stored, &k, l))
                .collect();
            (k, per)
        })
        .filter(|(k, per)| {
            needle.is_empty()
                || k.to_lowercase().contains(&needle)
                || per
                    .iter()
                    .any(|r| r.effective().to_lowercase().contains(&needle))
        })
        .collect();
    let results = results(&v, &locales, &rows);
    v.listing(&headers, results, |results| {
        let title = v.t("kernel.admin.texts.title");
        let body = html! {
            h1 { (title) }
            (consent_card(&v, version.as_deref()))
            form method="get" action="/admin/texts" class="card search" role="search" {
                label for="texts-q" { (v.t("kernel.admin.texts.search")) }
                input id="texts-q" type="search" name="q" value=(query) autocomplete="off"
                    hx-get="/admin/texts" hx-trigger="input changed delay:250ms, search"
                    hx-target="#texts-results" hx-swap="outerHTML" hx-push-url="true";
            }
            (results)
        };
        v.page(StatusCode::OK, &title, body)
    })
}

fn source_chip(v: &View<'_>, r: &TextRow) -> Markup {
    let class = match r.source {
        settings::Source::Setting => "chip override",
        settings::Source::Toml => "chip ok",
        settings::Source::Default => "chip",
    };
    html! { span class=(class) { (v.t(r.source.key())) } }
}

fn results(v: &View<'_>, locales: &[String], rows: &[(String, Vec<TextRow>)]) -> Markup {
    let key_label = v.t("kernel.admin.texts.key");
    html! {
        div id="texts-results" {
            p class="muted count" { (v.t("kernel.admin.total")) " " (rows.len()) }
            @if rows.is_empty() {
                div class="card" { p { (v.t("kernel.admin.none")) } }
            } @else {
                table class="rows texts" {
                    thead { tr {
                        th { (key_label) }
                        @for l in locales { th { (l) } }
                    } }
                    tbody {
                        @for (key, per) in rows {
                            tr {
                                td class="code" data-label=(key_label) {
                                    a href=(format!("/admin/texts/{key}")) { (key) }
                                }
                                @for r in per {
                                    td data-label=(r.locale) {
                                        span { (preview(r.effective())) " " (source_chip(v, r)) }
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

fn consent_card(v: &View<'_>, stored: Option<&str>) -> Markup {
    let current = stored.unwrap_or(&v.s.config.consent.version);
    html! {
        section class="card" id="consent" {
            h2 { (v.t("kernel.admin.texts.consent_title")) }
            p { (v.t("kernel.admin.texts.consent_current")) " " strong class="code" { (current) } }
            p class="muted" { (v.t("kernel.admin.texts.consent_help")) }
            form method="post" action="/admin/texts/consent" {
                (v.staff.csrf_field())
                input type="hidden" name="expected" value=(settings::version(stored));
                div class="field" {
                    label for="consent-version" { (v.t("kernel.admin.texts.consent_new")) }
                    input id="consent-version" name="version" required maxlength="32"
                        pattern="[A-Za-z0-9._\\-]+" autocomplete="off";
                }
                button type="submit" { (v.t("kernel.admin.texts.consent_save")) }
            }
        }
    }
}

/// The Consent version form.
#[derive(Debug, Default, Deserialize)]
pub(super) struct VersionForm {
    #[serde(default)]
    version: String,
    #[serde(default)]
    expected: String,
}

pub(super) async fn consent(
    State(s): State<AppState>,
    staff: CurrentStaff,
    ctx: RequestCtx,
    Form(f): Form<VersionForm>,
) -> Response {
    let v = View::new(&s, &staff);
    let back = "/admin/texts#consent";
    let new = f.version.trim();
    let stored = match settings::stored_version(&s.services).await {
        Ok(v) => v,
        Err(e) => return v.error(&e),
    };
    if settings::version(stored.as_deref()) != f.expected {
        return v.redirect(back, false, "kernel.admin.conflict");
    }
    let expected = stored.as_deref();
    let before = expected.unwrap_or(&s.config.consent.version);
    if !settings::valid_version(new) {
        return v.redirect(back, false, "kernel.admin.texts.consent_invalid");
    }
    if new == before {
        return v.redirect(back, false, "kernel.admin.texts.consent_same");
    }
    let actor = Actor::staff(&staff);
    match settings::set_version(&s.services, &actor, new, expected, before, &ctx).await {
        Ok(true) => v.redirect(back, true, "kernel.admin.texts.consent_saved"),
        Ok(false) => v.redirect(back, false, "kernel.admin.conflict"),
        Err(e) => v.error(&e),
    }
}

pub(super) async fn edit(
    State(s): State<AppState>,
    staff: CurrentStaff,
    Path(key): Path<String>,
) -> Response {
    let v = View::new(&s, &staff);
    render(&v, &key, None, StatusCode::OK).await
}

/// A refused value to show again: (locale, the value, why).
type Rejected<'a> = (&'a str, &'a str, Refused);

async fn render(
    v: &View<'_>,
    key: &str,
    rejected: Option<Rejected<'_>>,
    status: StatusCode,
) -> Response {
    if !settings::editable(&v.s.catalog, key) {
        return v.not_found("/admin/texts");
    }
    let stored = match settings::settings(&v.s.services).await {
        Ok(r) => r,
        Err(e) => return v.error(&e),
    };
    let locales = settings::locales(&v.s.catalog, &v.s.config);
    let action = format!("/admin/texts/{key}");
    let l = |k: &str| v.t(k);
    let body = html! {
        p { a href="/admin/texts" { "← " (l("kernel.admin.texts.back")) } }
        h1 class="code" { (key) }
        @for loc in &locales {
            @let r = settings::row(&v.s.catalog, &v.s.config, &stored, key, loc);
            @let names = settings::placeholders(&r.base);
            @let err = rejected.as_ref().filter(|(rl, _, _)| *rl == loc.as_str());
            section class="card" {
                h2 { (loc) " " (source_chip(v, &r)) }
                p class="muted" { (l("kernel.admin.texts.default")) }
                p class="default-text" { (r.base) }
                @if !names.is_empty() {
                    p class="muted" { (l("kernel.admin.texts.placeholders")) " "
                        @for n in &names { code { "{" (n) "}" } " " } }
                }
                form method="post" action=(action) {
                    (v.staff.csrf_field())
                    input type="hidden" name="locale" value=(loc);
                    input type="hidden" name="version" value=(settings::version(r.setting.as_deref()));
                    div class="field" {
                        label for=(format!("text-{loc}")) { (l("kernel.admin.texts.value")) }
                        textarea id=(format!("text-{loc}")) name="value"
                            aria-invalid=[err.map(|_| "true")] {
                            (err.map_or(r.effective(), |(_, value, _)| value))
                        }
                        @if let Some((_, _, why)) = err {
                            p class="field-error" { (l(why.key)) " "
                                @for n in &why.names { code { "{" (n) "}" } " " } }
                        }
                    }
                    div class="actions" {
                        button type="submit" name="op" value="save" { (l("kernel.admin.save")) }
                        @if r.setting.is_some() {
                            button class="secondary" type="submit" name="op" value="reset" {
                                (l("kernel.admin.texts.reset"))
                            }
                        }
                    }
                }
            }
        }
    };
    v.page(status, key, body)
}

/// One Locale's form.
#[derive(Debug, Default, Deserialize)]
pub(super) struct TextForm {
    #[serde(default)]
    locale: String,
    #[serde(default)]
    value: String,
    #[serde(default)]
    version: String,
    #[serde(default)]
    op: String,
}

pub(super) async fn submit(
    State(s): State<AppState>,
    staff: CurrentStaff,
    ctx: RequestCtx,
    Path(key): Path<String>,
    Form(f): Form<TextForm>,
) -> Response {
    let v = View::new(&s, &staff);
    let back = format!("/admin/texts/{key}");
    let actor = Actor::staff(&staff);
    if f.op == "reset" {
        return reset(&s, &v, &key, &f, &actor, &ctx).await;
    }
    let written = super::texts_api::write(
        &s.deployment,
        &key,
        &f.locale,
        &f.value,
        Some(&f.version),
        &actor,
        &ctx,
    )
    .await;
    match written {
        Ok(Written::NotEditable) => v.not_found("/admin/texts"),
        Ok(Written::Stale | Written::Saved(false)) => {
            v.redirect(&back, false, "kernel.admin.conflict")
        }
        Ok(Written::Unchanged | Written::Saved(true)) => {
            v.redirect(&back, true, "kernel.admin.saved")
        }
        Ok(Written::Refused(why)) => {
            let value = f.value.replace("\r\n", "\n");
            let rejected = Some((f.locale.as_str(), value.as_str(), why));
            render(&v, &key, rejected, StatusCode::UNPROCESSABLE_ENTITY).await
        }
        Err(e) => v.error(&e),
    }
}

/// "Reset to default": deletes the `setting` row the form was rendered
/// from (a compare-and-set on that value).
async fn reset(
    s: &AppState,
    v: &View<'_>,
    key: &str,
    f: &TextForm,
    actor: &Actor,
    ctx: &RequestCtx,
) -> Response {
    let locales = settings::locales(&s.catalog, &s.config);
    if !settings::editable(&s.catalog, key) || !locales.contains(&f.locale) {
        return v.not_found("/admin/texts");
    }
    let back = format!("/admin/texts/{key}");
    let stored = match settings::settings(&s.services).await {
        Ok(r) => r,
        Err(e) => return v.error(&e),
    };
    let current = stored.get(&(key.to_string(), f.locale.clone()));
    let Some(old) = current.filter(|c| settings::version(Some(c.as_str())) == f.version) else {
        return v.redirect(&back, false, "kernel.admin.conflict");
    };
    let row = settings::row(&s.catalog, &s.config, &Default::default(), key, &f.locale);
    let d = &s.deployment;
    let relink = |gate: &str| crate::kernel::menu::on_save(d, &stored, key, &f.locale, None, gate);
    let reset = settings::reset(
        &s.services,
        actor,
        key,
        &f.locale,
        old,
        &row.base,
        ctx,
        &relink,
    );
    match reset.await {
        Ok(true) => v.redirect(&back, true, "kernel.admin.texts.reset_done"),
        Ok(false) => v.redirect(&back, false, "kernel.admin.conflict"),
        Err(e) => v.error(&e),
    }
}

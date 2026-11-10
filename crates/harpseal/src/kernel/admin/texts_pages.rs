//! `/admin/texts` (`kernel.texts.edit`, #165): the list is `texts_list.rs`;
//! `/admin/texts/{key}` edits one key per Locale (save or reset to
//! default), titled by its text and the page it shows on (#125); `POST
//! /admin/texts/consent` sets the Consent notice version, through its
//! review step (`consent_review.rs`, #135). The version is a setting, not
//! a text: its card and route need `kernel.setting.edit`.

use axum::Form;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::Response;
use maud::{Markup, html};
use serde::Deserialize;

use super::consent_review as review;
use super::settings::{self, Refused, TextRow};
use super::texts_api::Written;
use super::texts_areas::Areas;
use super::view::{View, enc};
use crate::{Actor, AppState, CurrentStaff, RequestCtx};

pub(super) fn source_chip(v: &View<'_>, r: &TextRow) -> Markup {
    let class = match r.source {
        settings::Source::Setting => "chip override",
        settings::Source::Toml => "chip ok",
        settings::Source::Default => "chip",
    };
    html! { span class=(class) { (v.t(r.source.key())) } }
}

pub(super) fn consent_card(v: &View<'_>, stored: Option<&str>) -> Markup {
    if !v.staff.can(crate::kernel::SETTING_EDIT) {
        return html! {};
    }
    let current = stored.unwrap_or(&v.s.config.consent.version);
    html! {
        section class="card" id="consent" {
            h2 { (v.t("kernel.admin.texts.consent_title")) }
            p { (v.t("kernel.admin.texts.consent_current")) " " strong class="code" { (current) } }
            p class="muted" { (v.t("kernel.admin.texts.consent_help")) }
            form method="post" action="/admin/texts/consent" {
                (v.staff.csrf_field())
                input type="hidden" name="expected" value=(settings::version(stored));
                (review::field(v))
                button type="submit" { (v.t("kernel.admin.consent.review")) }
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
    /// `1` once the review step was seen (#135).
    #[serde(default)]
    reviewed: String,
    /// The number of Students the review step showed.
    #[serde(default)]
    reach: String,
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
    // The review step (#135): shown until it is confirmed with the
    // number it showed, counted again here.
    let reach = match review::asked_again(&s.services, new).await {
        Ok(n) => n,
        Err(e) => return v.error(&e),
    };
    let seen = f.reach.trim().parse::<u64>().ok();
    if f.reviewed != "1" || seen != Some(reach) {
        let change = review::Change {
            before,
            new,
            expected: &f.expected,
            reach,
            changed: f.reviewed == "1",
        };
        return review::page(&v, &change);
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
    let action = format!("/admin/texts/{}", enc(key));
    let l = |k: &str| v.t(k);
    let shown = Areas::new(v).of(key);
    // The title is the text in use (an edit included), in the Staff
    // member's Locale when the page lists it.
    let title_locale = locales
        .iter()
        .find(|l| **l == v.staff.locale.0)
        .or(locales.first())
        .cloned()
        .unwrap_or_default();
    let in_use = settings::row(&v.s.catalog, &v.s.config, &stored, key, &title_locale);
    let title = preview_short(in_use.effective());
    let body = html! {
        p { a class="back" href="/admin/texts" { "← " (l("kernel.admin.texts.back")) } }
        h1 { (title) }
        p class="muted" { (l("kernel.admin.texts.shown_on")) " "
            a href=(format!("/admin/texts?area={}", enc(&shown.id))) { (shown.name) } }
        @for loc in &locales {
            @let r = settings::row(&v.s.catalog, &v.s.config, &stored, key, loc);
            @let names = settings::placeholders(&r.base);
            @let err = rejected.as_ref().filter(|(rl, _, _)| *rl == loc.as_str());
            section class="card" {
                h2 { (v.s.catalog.locale_name(&v.staff.locale, loc)) " " (source_chip(v, &r)) }
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
        // Raw details (#123): the key and where a TOML text comes from.
        (crate::ui::technical(v.s, v.staff, html! {
            p { (l("kernel.admin.texts.tech_key")) " " code { (key) } }
            p { (l("kernel.admin.texts.tech_toml")) }
        }))
    };
    v.page(status, &title, body)
}

/// A text short enough for a page title.
fn preview_short(t: &str) -> String {
    let line = t.lines().next().unwrap_or("");
    let mut p: String = line.chars().take(60).collect();
    if line.chars().count() > 60 || t.lines().nth(1).is_some() {
        p.push('…');
    }
    p
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
    let back = format!("/admin/texts/{}", enc(&key));
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
    let back = format!("/admin/texts/{}", enc(key));
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
        Some(&row.base),
        ctx,
        &relink,
    );
    match reset.await {
        Ok(true) => v.redirect(&back, true, "kernel.admin.texts.reset_done"),
        Ok(false) => v.redirect(&back, false, "kernel.admin.conflict"),
        Err(e) => v.error(&e),
    }
}

//! `/admin/settings` (#75): every Module setting
//! ([`crate::ModuleRegistrar::setting`]), grouped by Module.
//!
//! The route is open to every signed-in Staff member; the page itself
//! decides. A setting is shown to Staff holding its own Permission or
//! `kernel.setting.edit`, and has a Save button only for its own
//! Permission: `kernel.setting.edit` alone shows the value, never edits
//! it. Nothing shown is a 403. A POST without the setting's Permission is
//! a 403 and writes nothing. A save is a compare-and-set on the form's
//! token (a stale form gets the conflict flash), audited as
//! `setting.update` with the value before and after.

use axum::Form;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::Response;
use maud::{Markup, html};
use serde::Deserialize;

use super::settings;
use super::view::View;
use crate::module::{SettingKind, SettingSpec, Settings};
use crate::{Actor, AppState, CurrentStaff, RequestCtx};

/// The page, under `/admin`.
pub(crate) const PATH: &str = "/settings";
/// The full path.
const FULL: &str = "/admin/settings";
/// Shows every Module setting (read-only without its own Permission).
const VIEW_ALL: &str = "kernel.setting.edit";
const TITLE: &str = "kernel.admin.module_settings.title";

/// Navigation entries: one per Permission that shows a setting, so
/// every holder sees the link (the guard lists a path once).
pub(crate) fn nav(specs: &[SettingSpec]) -> Vec<(&'static str, crate::module::NavSpec)> {
    let mut perms: Vec<&'static str> = Vec::new();
    for p in specs.iter().map(|s| s.permission).chain([VIEW_ALL]) {
        if !specs.is_empty() && !perms.contains(&p) {
            perms.push(p);
        }
    }
    perms
        .into_iter()
        .map(|permission| {
            let n = crate::module::NavSpec {
                permission,
                path: PATH,
                title_key: TITLE,
                order: 85,
            };
            (crate::kernel::NAME, n)
        })
        .collect()
}

fn visible(staff: &CurrentStaff, spec: &SettingSpec) -> bool {
    staff.can(VIEW_ALL) || staff.can(spec.permission)
}

/// A refused value to show again: (key, the value, catalog key of why).
type Rejected<'a> = (&'a str, &'a str, &'static str);

pub(super) async fn page(State(s): State<AppState>, staff: CurrentStaff) -> Response {
    let v = View::new(&s, &staff);
    render(&v, &s.settings(), None).await
}

async fn render(v: &View<'_>, all: &Settings, rejected: Option<Rejected<'_>>) -> Response {
    let shown: Vec<&SettingSpec> = all.specs().iter().filter(|x| visible(v.staff, x)).collect();
    if shown.is_empty() {
        return crate::auth::forbidden(v.s, v.staff, VIEW_ALL);
    }
    let mut cards = Vec::with_capacity(shown.len());
    for spec in &shown {
        let stored = match all.stored(spec.key).await {
            Ok(r) => r,
            Err(e) => return v.error(&e),
        };
        let err = rejected.filter(|(k, _, _)| *k == spec.key);
        cards.push(card(v, spec, stored, err));
    }
    let mut modules: Vec<&'static str> = shown.iter().map(|x| x.module).collect();
    modules.dedup();
    let title = v.t(TITLE);
    let body = html! {
        h1 { (title) }
        p class="muted" { (v.t("kernel.admin.module_settings.intro")) }
        @for m in &modules {
            h2 class="code" { (m) }
            @for (spec, c) in shown.iter().zip(&cards) {
                @if spec.module == *m { (c) }
            }
        }
    };
    let status = match rejected {
        Some(_) => StatusCode::UNPROCESSABLE_ENTITY,
        None => StatusCode::OK,
    };
    v.page(status, &title, body)
}

fn card(
    v: &View<'_>,
    spec: &SettingSpec,
    stored: Option<crate::module::settings::Stored>,
    err: Option<Rejected<'_>>,
) -> Markup {
    let key = spec.key;
    let id = format!("setting-{}", key.replace('.', "-"));
    let current = stored.as_ref().map_or(spec.default, |(v, _, _)| v.as_str());
    let shown = err.map_or(current, |(_, value, _)| value);
    let can = v.staff.can(spec.permission);
    // A saved value its kind now refuses (a restored row, a tightened
    // check): Students get "not set" until it is saved again.
    let broken = stored
        .as_ref()
        .is_some_and(|(s, _, _)| spec.kind.check(s).as_deref() != Ok(s.as_str()));
    html! {
        section class="card" id=(id) {
            h3 { (v.t(spec.label_key)) " "
                @if stored.is_some() {
                    span class="chip override" { (v.t("kernel.admin.module_settings.saved_value")) }
                } @else {
                    span class="chip" { (v.t("kernel.admin.module_settings.default_value")) }
                }
            }
            @if broken {
                div class="flash err" role="status" { (v.t("kernel.admin.module_settings.broken")) }
            }
            p class="muted" { (v.t(spec.help_key)) }
            p class="muted" { code { (key) } " · " (v.t(spec.kind.label_key())) }
            @if let Some((_, at, _)) = &stored {
                p class="muted" { (v.t("kernel.admin.module_settings.updated")) " " (v.time(*at)) }
            }
            @if can {
                form method="post" action=(FULL) {
                    (v.staff.csrf_field())
                    input type="hidden" name="key" value=(key);
                    input type="hidden" name="version" value=(settings::version(stored.as_ref().map(|(v, _, _)| v.as_str())));
                    div class="field" {
                        label for=(format!("{id}-value")) { (v.t("kernel.admin.module_settings.value")) }
                        (input(spec.kind, &format!("{id}-value"), shown, err.is_some() || broken))
                        @if let Some((_, _, why)) = err {
                            p class="field-error" { (v.t(why)) }
                        }
                    }
                    button type="submit" { (v.t("kernel.admin.save")) }
                }
            } @else {
                @if spec.kind == SettingKind::MultilineText {
                    pre class="setting-value" { (current) }
                } @else {
                    p class="setting-value" { strong { (current) } }
                }
                p class="muted" { (v.t("kernel.admin.module_settings.read_only")) " " code { (spec.permission) } }
            }
        }
    }
}

fn input(kind: SettingKind, id: &str, value: &str, invalid: bool) -> Markup {
    let bad = invalid.then_some("true");
    let max = crate::module::MAX_LINE_CHARS.to_string();
    match kind {
        SettingKind::MultilineText => html! {
            textarea id=(id) name="value" required rows="5"
                maxlength=(crate::module::MAX_MULTILINE_CHARS) aria-invalid=[bad] { (value) }
        },
        SettingKind::AccountNumber => html! {
            input id=(id) name="value" type="text" inputmode="numeric" required maxlength="64"
                pattern="[0-9 \\-]+" autocomplete="off" value=(value) aria-invalid=[bad];
        },
        SettingKind::Number => html! {
            input id=(id) name="value" type="number" step="1" required value=(value) aria-invalid=[bad];
        },
        SettingKind::Date => html! {
            input id=(id) name="value" type="date" required value=(value) aria-invalid=[bad];
        },
        SettingKind::Url => html! {
            input id=(id) name="value" type="url" required maxlength=(max)
                pattern="https://.+" value=(value) aria-invalid=[bad];
        },
        _ => html! {
            input id=(id) name="value" type="text" required maxlength=(max) value=(value) aria-invalid=[bad];
        },
    }
}

/// One setting's form.
#[derive(Debug, Default, Deserialize)]
pub(super) struct SettingForm {
    #[serde(default)]
    key: String,
    #[serde(default)]
    value: String,
    #[serde(default)]
    version: String,
}

pub(super) async fn submit(
    State(s): State<AppState>,
    staff: CurrentStaff,
    ctx: RequestCtx,
    Form(f): Form<SettingForm>,
) -> Response {
    let v = View::new(&s, &staff);
    let all = s.settings();
    let Some(spec) = all.spec(&f.key).cloned() else {
        return v.not_found(FULL);
    };
    // The setting's own Permission, never `kernel.setting.edit` alone.
    if !staff.can(spec.permission) {
        return crate::auth::forbidden(&s, &staff, spec.permission);
    }
    let back = format!("{FULL}#setting-{}", spec.key.replace('.', "-"));
    let stored = match all.stored(spec.key).await {
        Ok(r) => r.map(|(value, _, _)| value),
        Err(e) => return v.error(&e),
    };
    let expected = stored.as_deref();
    if settings::version(expected) != f.version {
        return v.redirect(&back, false, "kernel.admin.conflict");
    }
    let value = match spec.kind.check(&f.value) {
        Ok(value) => value,
        Err(why) => {
            let raw = f.value.replace("\r\n", "\n");
            return render(&v, &all, Some((spec.key, &raw, why))).await;
        }
    };
    let before = expected.unwrap_or(spec.default);
    if value == before {
        return v.redirect(&back, true, "kernel.admin.saved");
    }
    let actor = Actor::staff(&staff);
    let none = |_: &str| Ok(Vec::new());
    let saved = settings::save(
        &s.services,
        &actor,
        spec.key,
        "",
        &value,
        expected,
        before,
        &ctx,
        &none,
    );
    match saved.await {
        Ok(true) => v.redirect(&back, true, "kernel.admin.saved"),
        Ok(false) => v.redirect(&back, false, "kernel.admin.conflict"),
        Err(e) => v.error(&e),
    }
}

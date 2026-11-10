//! Saved Audiences on the editor (#144), inside the Audience fieldset
//! ([`super::page_audience`]):
//!
//! - the picker at the top (`GET /admin/announcements/audiences/picker`,
//!   loaded by HTMX): choosing one and "ใช้กลุ่มนี้" posts the form to
//!   `POST …/audiences/use`, which answers with the whole fieldset drawn
//!   from the saved rules; its count box then counts the reach again;
//! - "บันทึกกลุ่มผู้รับนี้" with a name at the bottom (`POST
//!   /admin/announcements/audiences`): the fieldset's current rules,
//!   checked as a draft's Audience is, saved under the name;
//! - [`stale`]: attribute values `[roster]` no longer has, kept marked
//!   under a warning so they are refused, never dropped (dropping one
//!   would widen the Audience).
//!
//! Both need HTMX: without it the picker never loads and the save box
//! stays `hidden` ([`SCRIPT`] shows it). What happened is said in a
//! persistent live region ([`status`], before the fieldset, updated out of
//! band) and shown beside the control; focus moves to the note after a
//! pick and back to the name box or button after a save. Enter in the name
//! box saves the Audience, never submits the announcement.

use axum::Form;
use axum::extract::State;
use axum::http::HeaderValue;
use axum::response::{Html, IntoResponse, Response};
use maud::{Markup, PreEscaped, html};

use super::form::Fields;
use super::form_images::NOT_FILES;
use super::saved::{self, Refused, Saved};
use super::view::View;
use crate::audience::Audience;
use crate::kernel::text::fill;
use crate::{Actor, AppState, CurrentStaff, RequestCtx, Services};

pub(super) const LIST: &str = "/admin/announcements/audiences";
const PICKER: &str = "/admin/announcements/audiences/picker";
const USE: &str = "/admin/announcements/audiences/use";
/// Sent after a save, so the picker lists the new one.
const CHANGED: &str = "saved-audiences-changed";

type Posted = Form<Vec<(String, String)>>;

/// Shows `[data-needs-js]` once HTMX runs; Enter in the name box presses
/// "บันทึกกลุ่มผู้รับนี้"; after a pick focus goes to the note, after a save
/// to the name box (refused) or the button (saved).
const SCRIPT: &str = r#"(function(){if(window.hsSavedAudiences)return;window.hsSavedAudiences=1;
function show(r){if(!r||!r.querySelectorAll)return;if(r.matches('[data-needs-js]'))r.hidden=false;r.querySelectorAll('[data-needs-js]').forEach(function(x){x.hidden=false;});}
document.addEventListener('htmx:load',function(e){show(e.detail.elt);});
document.addEventListener('keydown',function(e){if(e.key==='Enter'&&e.target&&e.target.id==='saved-name'){e.preventDefault();var b=document.getElementById('saved-save-btn');if(b)b.click();}});
document.addEventListener('htmx:afterSettle',function(e){var p=((e.detail.pathInfo||{}).requestPath)||'';var f=null;
if(p.slice(-14)==='/audiences/use'){f=document.getElementById('saved-note');}
else if(p.slice(-10)==='/audiences'){var n=document.getElementById('saved-name');f=n&&n.getAttribute('aria-invalid')?n:document.getElementById('saved-save-btn');}
if(f)f.focus();});})();"#;

/// The live region before the fieldset (`#saved-status`, read aloud, not
/// shown); `oob` when an HTMX answer updates it out of band.
pub(super) fn status(text: &str, oob: bool) -> Markup {
    html! {
        div id="saved-status" class="visually-hidden" role="status" aria-live="polite"
            hx-swap-oob=[oob.then_some("true")] { (text) }
    }
}

/// The page parts outside the fieldset: the live region and [`SCRIPT`].
pub(super) fn page_parts() -> Markup {
    html! { (status("", false)) script { (PreEscaped(SCRIPT)) } }
}

/// The note at the top of the fieldset after a pick (`#saved-note`, the
/// focus target): `ok` as a success, else as a caution.
pub(super) fn note(text: &str, ok: bool) -> Markup {
    let class = if ok { "flash ok" } else { "note warn-note" };
    html! { @if !text.is_empty() { p id="saved-note" class=(class) tabindex="-1" { (text) } } }
}

/// Where the picker loads, at the top of the fieldset.
pub(super) fn picker_slot() -> Markup {
    html! {
        div id="saved-pick" hx-get=(PICKER) hx-trigger=(format!("load, {CHANGED} from:body"))
            hx-swap="innerHTML" {}
    }
}

/// The name box and "บันทึกกลุ่มผู้รับนี้" (hidden until HTMX runs), with
/// what the last save did.
pub(super) fn save_field(v: &View<'_>, name: &str, said: Option<(bool, &str)>) -> Markup {
    let failed = said.is_some_and(|(ok, _)| !ok);
    html! {
        div id="saved-save" class="field saved-save" data-needs-js hidden {
            label for="saved-name" { (v.t("announcements.saved.name")) }
            div class="saved-row" {
                input id="saved-name" name="saved_name" autocomplete="off"
                    maxlength=(saved::MAX_NAME) value=(name)
                    aria-invalid=[failed.then_some("true")]
                    aria-describedby=[failed.then_some("saved-said")];
                button id="saved-save-btn" type="button" class="secondary" hx-post=(LIST)
                    hx-params=(NOT_FILES) hx-target="#saved-save" hx-swap="outerHTML" {
                    (v.t("announcements.saved.save"))
                }
            }
            @match said {
                Some((true, text)) => p id="saved-said" class="flash ok" { (text) },
                Some((false, text)) => p id="saved-said" class="field-error" { (text) },
                None => {},
            }
        }
    }
}

/// Attribute values of the form that `[roster]` no longer has, marked,
/// under a warning: posted again, they keep the draft and the count
/// refusing until Staff remove the mark.
pub(super) fn stale(v: &View<'_>, f: &Fields) -> Markup {
    if matches!(f.scope.as_str(), "everyone" | "students") {
        return html! {};
    }
    let mut a = Audience::everyone();
    a.attributes = f.attrs.clone();
    let gone = saved::stale(&a, &v.s.config.roster);
    html! {
        @if !gone.is_empty() {
            div id="stale-values" class="note warn-note" role="note" {
                p { (v.t("announcements.saved.stale")) }
                @for (key, value) in &gone {
                    label class="check" {
                        input type="checkbox" name=(format!("attr.{key}")) value=(value) checked;
                        " " (label_of(v, key)) ": " (value)
                    }
                }
            }
        }
    }
}

/// An attribute's label for Staff, else its key.
pub(super) fn label_of(v: &View<'_>, key: &str) -> String {
    let (locale, default) = (&v.staff.locale, &v.s.config.default_locale);
    v.s.config.roster.attributes.get(key).map_or_else(
        || key.to_string(),
        |s| s.label_in(key, locale, default).to_string(),
    )
}

/// `GET …/audiences/picker`: the select of the readable saved Audiences,
/// or nothing when none is saved yet.
pub(super) async fn picker(State(s): State<AppState>, staff: CurrentStaff) -> Response {
    let v = View::new(&s, &staff);
    let list = match saved::list(&s.services).await {
        Ok(l) => l,
        Err(e) => return v.error(&e),
    };
    let usable: Vec<&Saved> = list.iter().filter(|one| one.audience.is_some()).collect();
    let shown = html! {
        @if !usable.is_empty() {
            div class="field saved-pick" {
                label for="saved-audience" { (v.t("announcements.saved.pick")) }
                div class="saved-row" {
                    select id="saved-audience" name="saved_audience" {
                        option value="" { (v.t("announcements.saved.pick_none")) }
                        @for one in &usable { option value=(one.id) { (one.name) } }
                    }
                    button type="button" class="secondary" hx-post=(USE)
                        hx-params=(NOT_FILES) hx-target="#audience" hx-swap="outerHTML" {
                        (v.t("announcements.saved.use"))
                    }
                }
                p class="muted" { a href=(LIST) { (v.t("announcements.saved.manage")) } }
            }
        }
    };
    Html(shown.into_string()).into_response()
}

/// The form's Audience fields set to the saved rules.
fn apply(f: &mut Fields, scope: &str, a: &Audience) {
    f.scope = scope.to_string();
    f.attrs = a.attributes.clone();
    f.tags = a.tags.join(", ");
    f.students = a.students.join("\n");
    f.unreachable = a.include_unreachable;
}

/// Fills `f` from the saved Audience `id` (`""`: none chosen) and returns
/// what to tell Staff: which one filled it (`true`), or why nothing
/// changed (`false`: gone or unreadable).
pub(super) async fn fill_from(
    v: &View<'_>,
    s: &Services,
    id: &str,
    f: &mut Fields,
) -> crate::Result<Option<(String, bool)>> {
    if id.is_empty() {
        return Ok(None);
    }
    Ok(Some(match saved::load(s, id).await? {
        Some(Saved {
            name,
            scope,
            audience: Some(a),
            ..
        }) => {
            apply(f, &scope, &a);
            (
                fill(&v.t("announcements.saved.used"), &[("name", &name)]),
                true,
            )
        }
        Some(_) => (v.t("announcements.saved.use_unreadable"), false),
        None => (v.t("announcements.saved.use_gone"), false),
    }))
}

fn posted<'a>(pairs: &'a [(String, String)], key: &str) -> &'a str {
    pairs
        .iter()
        .find(|(k, _)| k == key)
        .map_or("", |(_, x)| x.trim())
}

/// `POST …/audiences/use`: the fieldset filled from the chosen saved
/// Audience (or, if it is gone or unreadable, as posted, with a note),
/// and the live region out of band.
pub(super) async fn use_saved(
    State(s): State<AppState>,
    staff: CurrentStaff,
    Form(pairs): Posted,
) -> Response {
    let v = View::new(&s, &staff);
    let mut f = Fields::from_pairs(&pairs);
    let id = posted(&pairs, "saved_audience");
    let said = match fill_from(&v, &s.services, id, &mut f).await {
        Ok(said) => said.unwrap_or_default(),
        Err(e) => return v.error(&e),
    };
    let shown = html! {
        (super::page_audience::fieldset_noted(&v, &f, &[], note(&said.0, said.1)))
        (status(&said.0, true))
    };
    Html(shown.into_string()).into_response()
}

/// `POST /admin/announcements/audiences`: saves the form's Audience under
/// `saved_name`; answers with the save field, and the live region out of
/// band.
pub(super) async fn save(
    State(s): State<AppState>,
    staff: CurrentStaff,
    ctx: RequestCtx,
    Form(pairs): Posted,
) -> Response {
    let v = View::new(&s, &staff);
    let typed = posted(&pairs, "saved_name").to_string();
    let f = Fields::from_pairs(&pairs);
    let roster = &s.deployment.config().roster;
    let refuse = |key: &str| {
        let text = v.t(key);
        fragment(save_field(&v, &typed, Some((false, &text))), &text, false)
    };
    let Some(name) = saved::clean_name(&typed) else {
        return refuse("announcements.saved.bad_name");
    };
    // A choice no page offers is no choice (the column only takes these).
    if !saved::SCOPES.contains(&f.scope.as_str()) {
        return refuse("announcements.form.scope_required");
    }
    let a = f.audience();
    if !saved::stale(&a, roster).is_empty() {
        return refuse("announcements.saved.stale");
    }
    if let Some((_, key)) = f.audience_errors(roster).first() {
        return refuse(key);
    }
    let actor = Actor::staff(&staff);
    match saved::create(&s.services, &name, &f.scope, &a, &actor, &ctx).await {
        Ok(Ok(_)) => {
            let text = fill(&v.t("announcements.saved.saved"), &[("name", &name)]);
            fragment(save_field(&v, "", Some((true, &text))), &text, true)
        }
        Ok(Err(Refused::Taken)) => refuse("announcements.saved.taken"),
        Ok(Err(_)) => refuse("announcements.saved.full"),
        Err(e) => v.error(&e),
    }
}

fn fragment(field: Markup, said: &str, changed: bool) -> Response {
    let shown = html! { (field) (status(said, true)) };
    let mut resp = Html(shown.into_string()).into_response();
    if changed {
        resp.headers_mut()
            .insert("HX-Trigger", HeaderValue::from_static(CHANGED));
    }
    resp
}

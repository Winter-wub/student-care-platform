//! `GET /admin/branding` and `POST /admin/branding/save` (#114): the
//! display name, primary colour and logo with a preview. `op=preview`
//! renders the back-office and Mini App headers with the submitted values
//! and saves nothing (a new logo travels on as a base64 field, so a save
//! after the preview needs no second upload); `op=save` checks and saves;
//! `op=reset` returns every value to `deployment.toml`.

use std::collections::HashMap;

use axum::extract::{Multipart, State};
use axum::http::StatusCode;
use axum::response::Response;
use maud::{Markup, html};

use super::check::{self, LogoKind};
use super::save::{self, Logo, Values};
use super::view::{View, data_url};
use super::{COLOR_KEY, LOGO_KEY, NAME_KEY, SAVE_PATH, Stored, menu_page};
use crate::backup::value::base64_decode;
use crate::{Actor, AppState, CurrentStaff, PageState, RequestCtx};

/// The name, colour and logo form as shown.
#[derive(Debug, Default)]
pub(super) struct LookForm {
    name: String,
    color: String,
    /// A logo uploaded with this form, not saved yet.
    draft: Option<(Vec<u8>, LogoKind)>,
    drop_logo: bool,
    /// Errors by field.
    errors: HashMap<&'static str, String>,
    /// Show the preview of these values.
    preview: bool,
}

impl LookForm {
    /// The form with the values in use.
    pub(super) fn current(s: &PageState, stored: &Stored) -> Self {
        let c = super::Chrome::of(&s.config, stored);
        Self {
            name: c.name,
            color: c.color,
            ..Self::default()
        }
    }
}

pub(super) async fn show(State(s): State<AppState>, staff: CurrentStaff) -> Response {
    let v = View::new(&s, &staff);
    render(&v, None, menu_page::MenuForm::default(), StatusCode::OK).await
}

/// The whole page: `look` (else the values in use) and `menu`.
pub(super) async fn render(
    v: &View<'_>,
    look: Option<LookForm>,
    menu: menu_page::MenuForm,
    status: StatusCode,
) -> Response {
    let Ok(stored) = Stored::load(&v.s.services).await else {
        return v.error();
    };
    let look = look.unwrap_or_else(|| LookForm::current(v.s, &stored));
    let menus = match menu_page::section(v, menu).await {
        Ok(m) => m,
        Err(_) => return v.error(),
    };
    let body = html! {
        h1 { (v.t("kernel.branding.title")) }
        p class="muted" { (v.t("kernel.branding.intro")) }
        (look_section(v, &stored, &look))
        (menus)
    };
    v.page(status, body)
}

fn source(v: &View<'_>, stored: &Stored, key: &str) -> Markup {
    let k = if stored.get(key).is_some() {
        "kernel.branding.source_setting"
    } else {
        "kernel.branding.source_toml"
    };
    html! { span class="chip" { (v.t(k)) } }
}

fn look_section(v: &View<'_>, stored: &Stored, f: &LookForm) -> Markup {
    let err = |k: &str| f.errors.get(k);
    let saved_logo = stored.get(LOGO_KEY).map(super::logo::url);
    let logo_src = match (&f.draft, f.drop_logo) {
        (Some((bytes, kind)), _) => Some(data_url(kind.content_type(), bytes)),
        (None, true) => v.s.config.branding.logo.clone(),
        (None, false) => saved_logo
            .clone()
            .or_else(|| v.s.config.branding.logo.clone()),
    };
    let draft = f
        .draft
        .as_ref()
        .map(|(b, _)| crate::backup::value::base64_encode(b));
    let shown_color =
        check::color(&f.color).unwrap_or_else(|_| super::Chrome::of(&v.s.config, stored).color);
    html! {
        section class="card" {
            h2 { (v.t("kernel.branding.look")) }
            @if !f.errors.is_empty() {
                p class="flash err" role="alert" { (v.t("kernel.branding.err_form")) }
            }
            form method="post" action=(format!("/admin{SAVE_PATH}")) enctype="multipart/form-data" {
                (v.staff.csrf_field())
                div class="field" {
                    label for="b-name" { (v.t("kernel.branding.name")) " " (source(v, stored, NAME_KEY)) }
                    input id="b-name" name="name" value=(f.name) maxlength="200"
                        aria-invalid=[err("name").map(|_| "true")];
                    @if let Some(e) = err("name") { p class="field-error" { (e) } }
                }
                div class="field" {
                    label for="b-color" { (v.t("kernel.branding.color")) " " (source(v, stored, COLOR_KEY)) }
                    input id="b-color" name="color" value=(f.color) maxlength="7"
                        aria-invalid=[err("color").map(|_| "true")];
                    p class="muted" { (v.t("kernel.branding.color_help")) }
                    @if let Some(e) = err("color") { p class="field-error" { (e) } }
                }
                div class="field" {
                    label for="b-logo" { (v.t("kernel.branding.logo")) " " (source(v, stored, LOGO_KEY)) }
                    input id="b-logo" type="file" name="logo" accept="image/png,image/jpeg"
                        aria-invalid=[err("logo").map(|_| "true")];
                    p class="muted" { (v.t("kernel.branding.logo_help")) }
                    @if let Some(e) = err("logo") { p class="field-error" { (e) } }
                    @if let Some(d) = &draft { input type="hidden" name="logo_draft" value=(d); }
                    @if saved_logo.is_some() {
                        label { input type="checkbox" name="drop_logo" value="1" checked[f.drop_logo];
                            " " (v.t("kernel.branding.logo_drop")) }
                    }
                }
                @if f.preview && f.errors.is_empty() {
                    h3 { (v.t("kernel.branding.preview_title")) }
                    (crate::ui::preview_bars(
                        f.name.trim(),
                        &shown_color,
                        logo_src.as_deref(),
                        &v.t("kernel.branding.preview_office"),
                        &v.t("kernel.branding.preview_mini"),
                    ))
                }
                div class="actions" {
                    button class="secondary" type="submit" name="op" value="preview" { (v.t("kernel.branding.preview")) }
                    button type="submit" name="op" value="save" { (v.t("kernel.branding.save")) }
                    @if [NAME_KEY, COLOR_KEY, LOGO_KEY].iter().any(|k| stored.get(k).is_some()) {
                        button class="secondary" type="submit" name="op" value="reset" { (v.t("kernel.branding.reset")) }
                    }
                }
            }
        }
    }
}

/// The fields of the form.
#[derive(Default)]
struct Posted {
    op: String,
    name: String,
    color: String,
    logo: Vec<u8>,
    draft: String,
    drop_logo: bool,
}

/// Longest text field read, in bytes (the logo draft is base64).
const FIELD_MAX: usize = 400 * 1024;

async fn read(mut m: Multipart) -> Option<Posted> {
    let mut p = Posted::default();
    while let Some(field) = m.next_field().await.ok()? {
        let name = field.name().unwrap_or_default().to_string();
        let bytes = field.bytes().await.ok()?;
        let limit = if name == "logo" {
            check::LOGO_MAX_BYTES + 1
        } else {
            FIELD_MAX
        };
        if bytes.len() > limit {
            // Over the limit: kept just over it, refused by the check.
            if name == "logo" {
                p.logo = bytes[..limit].to_vec();
            }
            continue;
        }
        let text = || String::from_utf8_lossy(&bytes).into_owned();
        match name.as_str() {
            "op" => p.op = text(),
            "name" => p.name = text(),
            "color" => p.color = text(),
            "logo" => p.logo = bytes.to_vec(),
            "logo_draft" => p.draft = text(),
            "drop_logo" => p.drop_logo = true,
            _ => {}
        }
    }
    Some(p)
}

pub(super) async fn submit(
    State(s): State<AppState>,
    staff: CurrentStaff,
    ctx: RequestCtx,
    m: Multipart,
) -> Response {
    let v = View::new(&s, &staff);
    let Some(p) = read(m).await else {
        return render(&v, None, Default::default(), StatusCode::BAD_REQUEST).await;
    };
    let Ok(stored) = Stored::load(&s.services).await else {
        return v.error();
    };
    let actor = Actor::staff(&staff);
    if p.op == "reset" {
        let done = save::save(&s.deployment, &stored, Values::default(), &actor, &ctx).await;
        return flash(&v, done, "kernel.branding.reset_done");
    }
    let mut f = LookForm {
        name: p.name.clone(),
        color: p.color.clone(),
        drop_logo: p.drop_logo,
        preview: p.op == "preview",
        ..LookForm::default()
    };
    let name = check::name(&p.name).map_err(|r| f.errors.insert("name", v.refused(&r)));
    let color = check::color(&p.color).map_err(|r| f.errors.insert("color", v.refused(&r)));
    let upload = if !p.logo.is_empty() {
        Some(p.logo)
    } else {
        base64_decode(p.draft.trim()).ok().filter(|b| !b.is_empty())
    };
    if let Some(bytes) = upload {
        match check::logo(&bytes) {
            Ok(kind) => f.draft = Some((bytes, kind)),
            Err(r) => {
                f.errors.insert("logo", v.refused(&r));
            }
        }
    }
    let (Ok(name), Ok(color)) = (name, color) else {
        return render(
            &v,
            Some(f),
            Default::default(),
            StatusCode::UNPROCESSABLE_ENTITY,
        )
        .await;
    };
    if !f.errors.is_empty() {
        return render(
            &v,
            Some(f),
            Default::default(),
            StatusCode::UNPROCESSABLE_ENTITY,
        )
        .await;
    }
    if f.preview {
        f.name = name;
        f.color = color;
        return render(&v, Some(f), Default::default(), StatusCode::OK).await;
    }
    // A value equal to `deployment.toml`'s is no setting: it follows the file.
    let toml = super::Chrome::of(&s.config, &Stored::default());
    let logo = match (f.draft, p.drop_logo, stored.get(LOGO_KEY)) {
        (Some((bytes, kind)), _, _) => Logo::New(bytes, kind),
        (None, false, Some(key)) => Logo::Keep(key.to_string()),
        _ => Logo::None,
    };
    let values = Values {
        name: (name != toml.name).then_some(name),
        color: (color != toml.color.to_ascii_lowercase()).then_some(color),
        logo,
    };
    let done = save::save(&s.deployment, &stored, values, &actor, &ctx).await;
    flash(&v, done, "kernel.branding.saved")
}

fn flash(v: &View<'_>, done: crate::Result<save::Outcome>, ok: &str) -> Response {
    match done {
        Ok(save::Outcome::Saved) => v.redirect(true, ok),
        Ok(save::Outcome::Unchanged) => v.redirect(true, "kernel.branding.unchanged"),
        Ok(save::Outcome::Stale) => v.redirect(false, "kernel.admin.conflict"),
        Err(_) => v.error(),
    }
}

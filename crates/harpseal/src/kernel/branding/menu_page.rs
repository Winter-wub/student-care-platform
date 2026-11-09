//! The Menu images part of `/admin/branding` (#114).
//!
//! The section shows each Menu's image in use (saved, or the Channel's
//! configured one) with its tap areas drawn over it. `POST
//! /admin/branding/menu`: `op=preview` checks the uploaded images
//! ([`super::apply::check`]) and draws their tap areas for every Locale's
//! Menu, saving nothing (the images travel on as base64 fields);
//! `op=apply` saves them through [`super::apply::save`] (stage on the
//! platform, then one batch, then the re-link Jobs); `op=reset` returns to
//! the configured images the same way. `GET /admin/branding/menu/{slot}`
//! is the image in use, for the section.

use std::collections::HashMap;

use axum::extract::{Multipart, Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use maud::{Markup, html};

use super::apply::{self, Change, MenuImagesSaved};
use super::logo::image as image_response;
use super::store::Saved;
use super::view::{View, areas, data_url};
use super::{MENU_PATH, page};
use crate::backup::value::{base64_decode, base64_encode};
use crate::channel::{MenuArt, MenuSlot};
use crate::kernel::menu::RELINK;
use crate::port_types::{Statement, Value};
use crate::{Actor, AppState, CurrentStaff, Error, PageState, RequestCtx};

/// The Menu images form as shown.
#[derive(Debug, Default)]
pub(super) struct MenuForm {
    /// Uploaded with this form, checked, not saved yet.
    drafts: HashMap<MenuSlot, Vec<u8>>,
    errors: HashMap<MenuSlot, String>,
    /// A form-wide error (catalog key).
    alert: Option<&'static str>,
}

const SLOTS: [(MenuSlot, &str, &str); 2] = [
    (MenuSlot::Locked, "locked", "kernel.branding.menu_locked"),
    (
        MenuSlot::Unlocked,
        "unlocked",
        "kernel.branding.menu_unlocked",
    ),
];

fn art(s: &PageState) -> Option<&dyn MenuArt> {
    s.deployment.channels().find_map(|c| c.menu_art())
}

/// Re-link and re-lock Jobs not finished yet.
async fn pending(s: &PageState) -> crate::Result<i64> {
    let rows = s
        .services
        .db
        .query(
            Statement::new(
                "SELECT count(*) AS n FROM job WHERE kind IN (?, ?) AND state IN ('queued', 'running')",
            )
            .bind(RELINK)
            .bind(super::RELOCK),
        )
        .await?;
    Ok(rows.get(0, "n").and_then(Value::as_i64).unwrap_or(0))
}

/// The section, for `f`.
pub(super) async fn section(v: &View<'_>, f: MenuForm) -> crate::Result<Markup> {
    let s = v.s;
    let Some(art) = art(s) else {
        return Ok(html! {
            section class="card" {
                h2 { (v.t("kernel.branding.menu_title")) }
                p class="muted" { (v.t("kernel.branding.menu_none")) }
            }
        });
    };
    let saved = Saved::load(&s.services).await?;
    let (_, menus) = apply::menus(&s.deployment).await?;
    let menus: Vec<_> = menus.into_iter().map(|(_, m)| m).collect();
    let jobs = pending(s).await?;
    let mut figures = Vec::new();
    for (slot, name, label) in SLOTS {
        let (src, bytes) = match f.drafts.get(&slot) {
            Some(b) => (data_url(art.content_type(), b), b.clone()),
            None => {
                let current = current(s, art, &saved, slot).await?;
                (format!("/admin{MENU_PATH}/{name}"), current)
            }
        };
        let state = if saved.key(slot).is_some() {
            "kernel.branding.menu_saved"
        } else {
            "kernel.branding.menu_configured"
        };
        let layouts = art.check(slot, &bytes, &menus).unwrap_or_default();
        figures.push((slot, name, label, state, src, layouts));
    }
    let caption = v.t("kernel.branding.menu_areas");
    Ok(html! {
        section class="card" {
            h2 { (v.t("kernel.branding.menu_title")) }
            p class="muted" { (v.t("kernel.branding.menu_help")) }
            @if jobs > 0 {
                p class="flash ok" role="status" {
                    (v.t("kernel.branding.menu_pending").replace("{n}", &jobs.to_string()))
                }
            }
            @if let Some(a) = f.alert { p class="flash err" role="alert" { (v.t(a)) } }
            form method="post" action=(format!("/admin{MENU_PATH}")) enctype="multipart/form-data" {
                (v.staff.csrf_field())
                @for (slot, name, label, state, src, layouts) in &figures {
                    div class="field" {
                        // The heading names the file input (#129).
                        h3 id=(format!("m-{name}-title")) { (v.t(label)) " " span class="chip" { (v.t(state)) } }
                        @for l in layouts { (areas(src, l, &caption)) }
                        @if f.drafts.contains_key(slot) {
                            input type="hidden" name=(format!("{name}_draft"))
                                value=(base64_encode(&f.drafts[slot]));
                        }
                        input id=(format!("m-{name}")) type="file" name=(name) accept="image/png"
                            aria-labelledby=(format!("m-{name}-title"))
                            aria-invalid=[f.errors.get(slot).map(|_| "true")]
                            aria-describedby=[f.errors.get(slot).map(|_| format!("m-{name}-error"))];
                        @if let Some(e) = f.errors.get(slot) { p class="field-error" id=(format!("m-{name}-error")) { (e) } }
                    }
                }
                div class="actions" {
                    button class="secondary" type="submit" name="op" value="preview" { (v.t("kernel.branding.menu_preview")) }
                    button type="submit" name="op" value="apply" { (v.t("kernel.branding.menu_apply")) }
                    @if saved.locked.is_some() || saved.unlocked.is_some() {
                        button class="secondary" type="submit" name="op" value="reset" { (v.t("kernel.branding.menu_reset")) }
                    }
                }
            }
        }
    })
}

/// The image in use for `slot`: saved, else configured.
async fn current(
    s: &PageState,
    art: &dyn MenuArt,
    saved: &Saved,
    slot: MenuSlot,
) -> crate::Result<Vec<u8>> {
    if let Some(key) = saved.key(slot)
        && let Some(b) = s.deployment.branding().images.get(&s.services, key).await?
    {
        return Ok(b.to_vec());
    }
    Ok(art.configured_image(slot).to_vec())
}

pub(super) async fn image(State(s): State<AppState>, Path(slot): Path<String>) -> Response {
    let slot = match slot.as_str() {
        "locked" => MenuSlot::Locked,
        "unlocked" => MenuSlot::Unlocked,
        _ => return StatusCode::NOT_FOUND.into_response(),
    };
    let Some(art) = art(&s) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let Ok(saved) = Saved::load(&s.services).await else {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    };
    match current(&s, art, &saved, slot).await {
        Ok(b) if !b.is_empty() => image_response(b, art.content_type(), "no-store"),
        Ok(_) => StatusCode::NOT_FOUND.into_response(),
        Err(_) => StatusCode::SERVICE_UNAVAILABLE.into_response(),
    }
}

/// Largest field read: a 1 MB image as base64, and some.
const FIELD_MAX: usize = 1_500_000;

async fn read(mut m: Multipart) -> Option<(String, HashMap<MenuSlot, Vec<u8>>)> {
    let mut op = String::new();
    let mut files: HashMap<MenuSlot, Vec<u8>> = HashMap::new();
    while let Some(field) = m.next_field().await.ok()? {
        let name = field.name().unwrap_or_default().to_string();
        let bytes = field.bytes().await.ok()?;
        let bytes = &bytes[..bytes.len().min(FIELD_MAX)];
        for (slot, n, _) in SLOTS {
            if name == n && !bytes.is_empty() {
                files.insert(slot, bytes.to_vec());
            } else if name == format!("{n}_draft") && !files.contains_key(&slot) {
                let text = String::from_utf8_lossy(bytes);
                if let Ok(b) = base64_decode(text.trim()).map(|b| (!b.is_empty()).then_some(b))
                    && let Some(b) = b
                {
                    files.insert(slot, b);
                }
            }
        }
        if name == "op" {
            op = String::from_utf8_lossy(bytes).into_owned();
        }
    }
    Some((op, files))
}

pub(super) async fn submit(
    State(s): State<AppState>,
    staff: CurrentStaff,
    ctx: RequestCtx,
    m: Multipart,
) -> Response {
    let v = View::new(&s, &staff);
    if art(&s).is_none() {
        return page::render(&v, None, MenuForm::default(), StatusCode::NOT_FOUND).await;
    }
    let Some((op, files)) = read(m).await else {
        return page::render(&v, None, MenuForm::default(), StatusCode::BAD_REQUEST).await;
    };
    let actor = Actor::staff(&staff);
    let d = &s.deployment;
    if op == "reset" {
        let change = Change {
            locked: None,
            unlocked: None,
            reset: true,
        };
        let done = apply::save(d, change, &actor, &ctx).await;
        return outcome(
            &v,
            done,
            "kernel.branding.menu_reset_done",
            MenuForm::default(),
        )
        .await;
    }
    let mut f = MenuForm::default();
    if files.is_empty() {
        f.alert = Some("kernel.branding.err_menu_none");
        return page::render(&v, None, f, StatusCode::UNPROCESSABLE_ENTITY).await;
    }
    let menus = match apply::menus(d).await {
        Ok((_, m)) => m.into_iter().map(|(_, m)| m).collect::<Vec<_>>(),
        Err(_) => return v.error(),
    };
    for (slot, bytes) in files {
        match apply::check(d, slot, &bytes, &menus) {
            Ok(_) => {
                f.drafts.insert(slot, bytes);
            }
            Err(p) => {
                f.errors.insert(slot, v.problem(&p));
            }
        }
    }
    if !f.errors.is_empty() {
        return page::render(&v, None, f, StatusCode::UNPROCESSABLE_ENTITY).await;
    }
    if op != "apply" {
        return page::render(&v, None, f, StatusCode::OK).await;
    }
    let change = Change {
        locked: f.drafts.get(&MenuSlot::Locked).cloned(),
        unlocked: f.drafts.get(&MenuSlot::Unlocked).cloned(),
        reset: false,
    };
    let done = apply::save(d, change, &actor, &ctx).await;
    outcome(&v, done, "kernel.branding.menu_applied", f).await
}

async fn outcome(
    v: &View<'_>,
    done: crate::Result<MenuImagesSaved>,
    ok: &str,
    mut f: MenuForm,
) -> Response {
    match done {
        Ok(MenuImagesSaved::Saved) => v.redirect(true, ok),
        Ok(MenuImagesSaved::Unchanged) => v.redirect(true, "kernel.branding.unchanged"),
        Ok(MenuImagesSaved::Stale) => v.redirect(false, "kernel.admin.conflict"),
        Ok(MenuImagesSaved::Refused(problems)) => {
            for (slot, p) in problems {
                f.errors.insert(slot, v.problem(&p));
            }
            page::render(v, None, f, StatusCode::UNPROCESSABLE_ENTITY).await
        }
        Err(e) => match e.root() {
            // The platform refused (or was unreachable): nothing saved.
            Error::Platform(_) | Error::RateLimited { .. } | Error::Unreachable => {
                f.alert = Some("kernel.branding.menu_stage_failed");
                page::render(v, None, f, StatusCode::BAD_GATEWAY).await
            }
            _ => v.error(),
        },
    }
}

//! `/admin/roles` (`kernel.staff.manage`): every Role with its Permissions
//! grouped by Module (the part of the id before the first `.`), the create
//! form, and `/admin/roles/{id}` to edit a Role's Permissions or delete an
//! unused Role. Roles from `[roles]` are in the same table and editable;
//! `[roles]` is seeded once (#110), so the page notes that an edit sticks
//! and a later `migrate` grants only Permissions new to the TOML.

use std::collections::BTreeMap;

use axum::Form;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::Response;
use maud::{Markup, html};

use super::view::View;
use crate::auth::role_admin;
use crate::auth::roles::{self, RoleRow};
use crate::auth::staff_admin::Outcome;
use crate::manifest::Permission;
use crate::{Actor, AppState, CurrentStaff, Error, RequestCtx};

/// Declared Permissions by Module, in declaration order.
fn by_module(perms: &[Permission]) -> BTreeMap<&str, Vec<&Permission>> {
    let mut m: BTreeMap<&str, Vec<&Permission>> = BTreeMap::new();
    for p in perms {
        let module = p.id.split('.').next().unwrap_or(p.id);
        m.entry(module).or_default().push(p);
    }
    m
}

/// A Module as Staff know it (#125): "ระบบหลัก" for the Kernel, else the
/// title of the Module's first back-office page ("ประกาศข่าว"), else its
/// own `<module>.title` or `<module>.menu` text, else "ส่วนเสริม".
pub(super) fn group_name(v: &View<'_>, module: &str) -> String {
    if module == crate::kernel::NAME {
        return v.t("kernel.admin.roles.group_kernel");
    }
    let prefix = format!("{module}.");
    if let Some(n) =
        v.s.deployment
            .nav_items()
            .iter()
            .find(|n| n.permission.starts_with(&prefix))
    {
        return v.t(n.title_key);
    }
    [format!("{module}.title"), format!("{module}.menu")]
        .iter()
        .find(|k| v.s.catalog.knows(k))
        .map_or_else(|| v.t("kernel.admin.texts.area_module"), |k| v.t(k))
}

/// Permission checkboxes named `permission`, grouped by Module, each
/// labelled with its Thai sentence (#125).
fn permission_boxes(v: &View<'_>, perms: &[Permission], held: &[String]) -> Markup {
    html! {
        @for (module, list) in by_module(perms) {
            fieldset class="checks" {
                legend { (group_name(v, module)) }
                @for p in list {
                    label class="check" {
                        input type="checkbox" name="permission" value=(p.id)
                            checked[held.iter().any(|h| h == p.id)];
                        " " span { (p.label()) }
                    }
                }
            }
        }
    }
}

/// Every Permission's id beside its English description, for the
/// technical details (#123).
fn permission_ids(perms: &[Permission]) -> Markup {
    html! {
        ul class="list permission-ids" {
            @for p in perms { li { span class="code" { (p.id) } span class="muted" { (p.description) } } }
        }
    }
}

/// Each Role's stored name beside the label Staff read, for the
/// technical details (#125).
fn stored_names(v: &View<'_>, roles: &[&RoleRow]) -> Markup {
    html! {
        @if !roles.is_empty() {
            p class="muted" { (v.t("kernel.admin.roles.stored")) }
            ul class="list" {
                @for r in roles { li { span class="code" { (r.name) } " " (v.role(&r.name)) } }
            }
        }
    }
}

fn from_toml(v: &View<'_>, name: &str) -> bool {
    v.s.config.roles.contains_key(name)
}

pub(super) async fn list(State(s): State<AppState>, staff: CurrentStaff) -> Response {
    let v = View::new(&s, &staff);
    render_list(&v, "", &[], None, StatusCode::OK).await
}

async fn render_list(
    v: &View<'_>,
    name: &str,
    picked: &[String],
    error: Option<&str>,
    status: StatusCode,
) -> Response {
    let all = match roles::list(&v.s.services).await {
        Ok(r) => r,
        Err(e) => return v.error(&e),
    };
    let declared = v.s.deployment.permissions();
    let title = v.t("kernel.admin.roles.title");
    let l = |k: &str| v.t(k);
    let body = html! {
        p { a href="/admin/staff" { "← " (l("kernel.admin.staff.title")) } }
        h1 { (title) }
        @for r in &all {
            section class="card" {
                h2 {
                    a href=(format!("/admin/roles/{}", r.id)) { (v.role(&r.name)) }
                }
                p class="muted" { (l("kernel.admin.roles.holders")) " " (crate::ui::number(r.holders)) }
                @for (module, list) in by_module(declared) {
                    @let held: Vec<&&Permission> = list.iter().filter(|p| r.permissions.iter().any(|h| h == p.id)).collect();
                    @if !held.is_empty() {
                        p { span class="perm-group" { (group_name(v, module)) } " "
                            @for p in held { span class="chip" { (p.label()) } " " } }
                    }
                }
            }
        }
        section class="card" id="new-role" {
            h2 { (l("kernel.admin.roles.create")) }
            form method="post" action="/admin/roles" {
                (v.staff.csrf_field())
                @if let Some(e) = error { p class="field-error" role="alert" { (l(e)) } }
                div class="field" {
                    label for="role-name" { (l("kernel.admin.roles.name")) }
                    input id="role-name" name="name" value=(name) required maxlength="64";
                }
                (permission_boxes(v, declared, picked))
                button type="submit" { (l("kernel.admin.roles.create")) }
            }
        }
        // Raw details (#123): Roles from the TOML, Permission ids.
        (crate::ui::technical(v.s, v.staff, html! {
            @for r in all.iter().filter(|r| from_toml(v, &r.name)) {
                p { (v.role(&r.name)) " " span class="chip" { (l("kernel.admin.roles.toml")) } }
            }
            (stored_names(v, &all.iter().collect::<Vec<_>>()))
            (permission_ids(declared))
        }))
    };
    v.page(status, &title, body)
}

/// `name` and the checked `permission`s.
fn posted(pairs: Vec<(String, String)>) -> (String, Vec<String>, String) {
    let (mut name, mut perms, mut version) = (String::new(), Vec::new(), String::new());
    for (k, v) in pairs {
        match k.as_str() {
            "name" => name = v,
            "permission" => perms.push(v),
            "version" => version = v,
            _ => {}
        }
    }
    (name, perms, version)
}

pub(super) async fn create(
    State(s): State<AppState>,
    staff: CurrentStaff,
    ctx: RequestCtx,
    Form(pairs): Form<Vec<(String, String)>>,
) -> Response {
    let v = View::new(&s, &staff);
    let (name, perms, _) = posted(pairs);
    let bad = StatusCode::UNPROCESSABLE_ENTITY;
    let Ok(clean) = role_admin::clean_name(&name) else {
        return render_list(&v, &name, &perms, Some("kernel.admin.roles.err_name"), bad).await;
    };
    // A name another Role is already shown as (#125): Staff could not
    // tell the two apart.
    match roles::list(&s.services).await {
        Ok(all)
            if all
                .iter()
                .any(|r| r.name != clean && v.role(&r.name) == clean) =>
        {
            let key = Some("kernel.admin.roles.err_label");
            return render_list(&v, &name, &perms, key, bad).await;
        }
        Ok(_) => {}
        Err(e) => return v.error(&e),
    }
    let Ok(perms) = role_admin::clean_permissions(s.deployment.permissions(), &perms) else {
        return render_list(
            &v,
            &name,
            &perms,
            Some("kernel.admin.roles.err_permission"),
            bad,
        )
        .await;
    };
    let actor = Actor::staff(&staff);
    match role_admin::create(&s.services, &actor, &clean, &perms, &ctx).await {
        Ok((Outcome::Done, Some(id))) => v.redirect(
            &format!("/admin/roles/{id}"),
            true,
            "kernel.admin.roles.created",
        ),
        Ok(_) => render_list(&v, &name, &perms, Some("kernel.admin.roles.err_taken"), bad).await,
        Err(e) => v.error(&e),
    }
}

async fn load(v: &View<'_>, id: &str) -> Result<RoleRow, Box<Response>> {
    match roles::get(&v.s.services, id).await {
        Ok(Some(r)) => Ok(r),
        Ok(None) => Err(Box::new(v.not_found("/admin/roles"))),
        Err(e) => Err(Box::new(v.error(&e))),
    }
}

pub(super) async fn edit(
    State(s): State<AppState>,
    staff: CurrentStaff,
    Path(id): Path<String>,
) -> Response {
    let v = View::new(&s, &staff);
    let role = match load(&v, &id).await {
        Ok(r) => r,
        Err(resp) => return *resp,
    };
    render_edit(&v, &role, &role.permissions, None, StatusCode::OK)
}

fn render_edit(
    v: &View<'_>,
    role: &RoleRow,
    picked: &[String],
    error: Option<&str>,
    status: StatusCode,
) -> Response {
    let l = |k: &str| v.t(k);
    let base = format!("/admin/roles/{}", role.id);
    let body = html! {
        p { a href="/admin/roles" { "← " (l("kernel.admin.roles.title")) } }
        h1 { (v.role(&role.name)) }
        p class="muted" { (l("kernel.admin.roles.holders")) " " (crate::ui::number(role.holders)) }
        p class="muted" { (l("kernel.admin.roles.undeclared")) }
        form method="post" action=(base) class="card" {
            (v.staff.csrf_field())
            input type="hidden" name="version" value=(role.version());
            @if let Some(e) = error { p class="field-error" role="alert" { (l(e)) } }
            (permission_boxes(v, v.s.deployment.permissions(), picked))
            button type="submit" { (l("kernel.admin.save")) }
        }
        @if role.holders == 0 {
            form method="post" action=(format!("{base}/delete")) class="card" {
                (v.staff.csrf_field())
                p class="muted" { (l("kernel.admin.roles.delete_help")) }
                button class="secondary" type="submit" { (l("kernel.admin.roles.delete")) }
            }
        } @else {
            p class="muted" { (l("kernel.admin.roles.in_use")) }
        }
        // Raw details (#123): where the Role came from, Permission ids.
        (crate::ui::technical(v.s, v.staff, html! {
            @if from_toml(v, &role.name) { p class="muted" { (l("kernel.admin.roles.toml_help")) } }
            (stored_names(v, &[role]))
            (permission_ids(v.s.deployment.permissions()))
        }))
    };
    v.page(status, &v.role(&role.name), body)
}

pub(super) async fn update(
    State(s): State<AppState>,
    staff: CurrentStaff,
    ctx: RequestCtx,
    Path(id): Path<String>,
    Form(pairs): Form<Vec<(String, String)>>,
) -> Response {
    let v = View::new(&s, &staff);
    let role = match load(&v, &id).await {
        Ok(r) => r,
        Err(resp) => return *resp,
    };
    let (_, perms, version) = posted(pairs);
    let back = format!("/admin/roles/{id}");
    let Ok(clean) = role_admin::clean_permissions(s.deployment.permissions(), &perms) else {
        let bad = StatusCode::UNPROCESSABLE_ENTITY;
        return render_edit(
            &v,
            &role,
            &perms,
            Some("kernel.admin.roles.err_permission"),
            bad,
        );
    };
    let actor = Actor::staff(&staff);
    match role_admin::set_permissions(&s.services, &actor, &role, &clean, &version, &ctx).await {
        Ok(Outcome::Done) => v.redirect(&back, true, "kernel.admin.saved"),
        Ok(Outcome::LastManager) => v.redirect(&back, false, "kernel.admin.staff.last_manager"),
        Ok(_) => v.redirect(&back, false, "kernel.admin.conflict"),
        Err(Error::Invalid(_)) => v.redirect(&back, false, "kernel.admin.roles.err_permission"),
        Err(e) => v.error(&e),
    }
}

pub(super) async fn delete(
    State(s): State<AppState>,
    staff: CurrentStaff,
    ctx: RequestCtx,
    Path(id): Path<String>,
) -> Response {
    let v = View::new(&s, &staff);
    let role = match load(&v, &id).await {
        Ok(r) => r,
        Err(resp) => return *resp,
    };
    let actor = Actor::staff(&staff);
    match role_admin::delete(&s.services, &actor, &role, &ctx).await {
        Ok(Outcome::Done) => v.redirect("/admin/roles", true, "kernel.admin.roles.deleted"),
        Ok(_) => v.redirect(
            &format!("/admin/roles/{id}"),
            false,
            "kernel.admin.roles.in_use",
        ),
        Err(e) => v.error(&e),
    }
}

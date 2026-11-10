//! `GET /admin/students/{id}`: one Student's fields, attributes, Links per
//! Channel, Tags, Consent and recent Security Events, with the forms the
//! Staff member's Permissions allow. Each view is logged as the sensitive
//! read `student.view`; the page is refused when that log cannot be
//! written.

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::Response;
use maud::{Markup, html};

use super::read::{self, LinkRow, StudentRow};
use super::view::{View, channel_label};
use super::{attribute_field, pick, service};
use crate::kernel::{LINK_MANAGE, LINK_REVOKE};
use crate::{Actor, AppState, CurrentStaff, RequestCtx, Result};

pub(super) async fn page(
    State(s): State<AppState>,
    staff: CurrentStaff,
    ctx: RequestCtx,
    Path(id): Path<String>,
) -> Response {
    let v = View::new(&s, &staff);
    let row = match read::student(&s.services, &id).await {
        Ok(Some(row)) => row,
        Ok(None) => return v.not_found(),
        Err(e) => return v.error(&e),
    };
    let actor = Actor::staff(&staff);
    if let Err(e) = service::record_view(&s.deployment, &id, &actor, &ctx).await {
        return v.error(&e);
    }
    match body(&v, &row).await {
        Ok(body) => v.page(StatusCode::OK, &row.fields.display_name, body),
        Err(e) => v.error(&e),
    }
}

async fn body(v: &View<'_>, row: &StudentRow) -> Result<Markup> {
    let s = &v.s.services;
    let links = read::links(s, &row.id).await?;
    let tags = read::tags(s, &row.id).await?;
    let consents = read::consents(s, &row.id).await?;
    let events = read::security_events(s, &row.id).await?;
    let picker: Pick = if v.staff.can(LINK_MANAGE) && row.status == "active" {
        let offered = v.s.deployment.staff_channel_kinds();
        Some(pick::recent_unlinked(s, &offered).await?)
    } else {
        None
    };
    let can_edit = v.staff.can("kernel.student.edit");
    let base = format!("/admin/students/{}", row.id);
    let no_value = v.t("kernel.students.no_value");
    let shown = |val: String| {
        if val.is_empty() {
            no_value.clone()
        } else {
            val
        }
    };
    let badge = |field: &str| {
        let on = row.overrides.iter().any(|o| o == field);
        html! { @if on { " " span class="chip override" { (v.t("kernel.students.override")) } } }
    };
    Ok(html! {
        p { a class="back" href="/admin/students" { "← " (v.t("kernel.students.back")) } }
        h1 { (row.fields.display_name) " " (v.status_chip(&row.status)) (badge("status")) }
        p class="code id" { (row.code) }
        div class="cols" {
            div {
                section class="card" {
                    h2 { (v.t("kernel.students.profile")) }
                    dl class="facts" {
                        dt { (v.t("kernel.students.name")) (badge("display_name")) }
                        dd { (row.fields.display_name) }
                        dt { (v.t("kernel.students.email")) (badge("email")) }
                        dd { (shown(row.fields.email.clone().unwrap_or_default())) }
                        dt { (v.t("kernel.students.locale")) (badge("locale")) }
                        dd { (v.s.catalog.locale_name(&v.staff.locale, &row.fields.locale.0)) }
                        @for (key, spec) in v.s.config.roster.attributes_in_order() {
                            dt { (v.label(key, spec)) (badge(&attribute_field(key))) }
                            dd { (shown(v.shown(spec, row.fields.attributes.get(key)))) }
                        }
                    }
                    @if can_edit {
                        a class="btn" href=(format!("{base}/edit")) { (v.t("kernel.students.edit")) }
                    }
                }
                (links_card(v, row, &links, &picker))
            }
            div {
                (tags_card(v, &base, &tags, can_edit))
                section class="card" {
                    h2 { (v.t("kernel.students.consent")) }
                    @if consents.is_empty() { p class="muted" { (v.t("kernel.students.consent_none")) } }
                    ul class="list" {
                        @for (version, at, channel) in &consents {
                            li {
                                span class="name" { (v.t("kernel.students.consent_version")) " " (version) }
                                span class="muted" { (v.time(*at)) " · " (channel_label(channel)) }
                            }
                        }
                    }
                }
                section class="card" {
                    h2 { (v.t("kernel.students.security")) }
                    @if events.is_empty() { p class="muted" { (v.t("kernel.students.security_none")) } }
                    ul class="list" {
                        @for (at, kind) in &events {
                            li { (crate::kernel::security::kind_label(&v.s.catalog, &v.staff.locale, kind)) " " span class="muted" { (v.time(*at)) } }
                        }
                    }
                }
                @if can_edit { (status_card(v, &base, &row.status)) }
            }
        }
    })
}

/// The Manual Link picker's accounts (#143), read only for Staff who may
/// Manual Link an active Student.
type Pick = Option<(Vec<pick::Recent>, bool)>;

fn links_card(v: &View<'_>, row: &StudentRow, links: &[LinkRow], picker: &Pick) -> Markup {
    // Staff unlink and Manual Link are separate Permissions (#80).
    let can_revoke = v.staff.can(LINK_REVOKE);
    // A dev-only or unregistered Channel (#131, #133) gets a row only when
    // it holds a Link: existing Links always show, with their revoke button.
    let offered = v.s.deployment.staff_channel_kinds();
    let live = links.iter().filter(|l| l.status != "revoked");
    let kinds = v.s.deployment.link_kinds(live.map(|l| l.channel.as_str()));
    let base = format!("/admin/students/{}", row.id);
    let history: Vec<&LinkRow> = links.iter().filter(|l| l.status == "revoked").collect();
    html! {
        section class="card" {
            h2 { (v.t("kernel.students.links")) }
            ul class="list links" {
                @for kind in &kinds {
                    @let live = links.iter().find(|l| l.channel == *kind && l.status != "revoked");
                    li {
                        span class="name" { (v.link_icon(kind.as_str(), live.map(|l| l.status.as_str()))) }
                        // The state is in the line's words already (#129).
                        @if let Some(l) = live {
                            span class="muted" {
                                (v.t(&format!("kernel.students.method_{}", l.method))) " · "
                                (v.t("kernel.students.linked_at")) " " (v.time(l.linked_at))
                                @if let Some(by) = &l.verified_by { " · " (v.t("kernel.students.verified_by")) " " (by) }
                            }
                            span class="muted code" { (v.t("kernel.students.account")) " " (l.external_id) }
                            @if can_revoke {
                                form method="post" action=(format!("{base}/links/{}/revoke", l.id)) {
                                    (v.staff.csrf_field())
                                    button class="secondary" type="submit" { (v.t("kernel.students.revoke")) }
                                }
                            }
                        }
                    }
                }
            }
            @if !history.is_empty() {
                details {
                    summary { (v.t("kernel.students.history")) " (" (history.len()) ")" }
                    ul class="list" {
                        @for l in &history {
                            li class="muted" {
                                (channel_label(&l.channel)) " · " (l.external_id) " · "
                                (v.t(&format!("kernel.students.method_{}", l.method))) " · "
                                (v.time(l.linked_at)) " → " (l.revoked_at.map(|t| v.time(t)).unwrap_or_default())
                            }
                        }
                    }
                }
            }
            @if let Some((recent, more)) = picker {
                form method="post" action=(format!("{base}/links")) class="manual-link" {
                    (v.staff.csrf_field())
                    h3 { (v.t("kernel.students.manual_link")) }
                    p class="muted" { (v.t("kernel.students.manual_link_help")) }
                    (pick::fields(v, recent, *more, &offered))
                    button type="submit" { (v.t("kernel.students.link_button")) }
                }
            }
        }
    }
}

fn tags_card(
    v: &View<'_>,
    base: &str,
    tags: &[(String, String, String)],
    can_edit: bool,
) -> Markup {
    let group = |src: &'static str| tags.iter().filter(move |(_, _, s)| s == src);
    html! {
        section class="card" {
            h2 { (v.t("kernel.students.tags")) }
            h3 { (v.t("kernel.students.tags_roster")) }
            p class="chips" {
                @for (_, name, _) in group("roster") { span class="chip" { (name) } " " }
                @if group("roster").next().is_none() { span class="muted" { (v.t("kernel.students.no_tags")) } }
            }
            h3 { (v.t("kernel.students.tags_manual")) }
            ul class="list" {
                @for (id, name, _) in group("manual") {
                    li {
                        span class="chip" { (name) }
                        @if can_edit {
                            form method="post" action=(format!("{base}/tags/remove")) {
                                (v.staff.csrf_field())
                                input type="hidden" name="tag_id" value=(id);
                                button class="link" type="submit" { (v.t("kernel.students.tag_remove")) }
                            }
                        }
                    }
                }
            }
            @if group("manual").next().is_none() { p class="muted" { (v.t("kernel.students.no_tags")) } }
            @if can_edit {
                form method="post" action=(format!("{base}/tags")) class="inline" {
                    (v.staff.csrf_field())
                    label for="tag-name" { (v.t("kernel.students.tag_name")) }
                    input id="tag-name" name="name" type="text" maxlength="64" required;
                    button type="submit" { (v.t("kernel.students.tag_add")) }
                }
            }
        }
    }
}

fn status_card(v: &View<'_>, base: &str, status: &str) -> Markup {
    let (to, key) = if status == "left" {
        ("active", "kernel.students.set_active")
    } else {
        ("left", "kernel.students.set_left")
    };
    html! {
        section class="card" {
            h2 { (v.t("kernel.students.status")) }
            form method="post" action=(format!("{base}/status")) {
                (v.staff.csrf_field())
                input type="hidden" name="status" value=(to);
                button class="secondary" type="submit" { (v.t(key)) }
            }
        }
    }
}

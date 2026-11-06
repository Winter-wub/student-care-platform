//! `GET/POST /admin/announcements/new`, `GET/POST …/{id}/edit` and the
//! live Audience count `POST …/audience` (an HTMX fragment: Students,
//! Links per Channel and the metered Push estimate).

use axum::Form;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{Html, IntoResponse, Response};
use maud::{Markup, html};

use super::form::{Fields, MAX_BODY, MAX_TITLE};
use super::store::{self, Row};
use super::view::View;
use super::{page_detail, service};
use crate::audience::{self, Counts};
use crate::config::AttributeType;
use crate::kernel::students::channel_label;
use crate::kernel::text::fill;
use crate::kernel::usage::{self, MonthSummary};
use crate::{Actor, AppState, CurrentStaff, Error, RequestCtx};

type Posted = Form<Vec<(String, String)>>;

const AUDIENCE_PATH: &str = "/admin/announcements/audience";

pub(super) async fn new_page(State(s): State<AppState>, staff: CurrentStaff) -> Response {
    let v = View::new(&s, &staff);
    let fields = Fields {
        when: "now".into(),
        ..Fields::default()
    };
    form_page(&v, StatusCode::OK, None, &fields, &[])
}

pub(super) async fn new_post(
    State(s): State<AppState>,
    staff: CurrentStaff,
    ctx: RequestCtx,
    Form(pairs): Posted,
) -> Response {
    let v = View::new(&s, &staff);
    let f = Fields::from_pairs(&pairs);
    let d = s.deployment.as_ref();
    let draft = match f.draft(&d.config().roster, v.offset(), d.services().now_ms()) {
        Ok(draft) => draft,
        Err(errors) => return form_page(&v, StatusCode::UNPROCESSABLE_ENTITY, None, &f, &errors),
    };
    let actor = Actor::staff(&staff);
    let id = match service::create(d, &draft, &actor, &ctx).await {
        Ok(id) => id,
        Err(e) => return v.error(&e),
    };
    let to = format!("/admin/announcements/{id}");
    if f.submit != "confirm" {
        return v.redirect(&to, true, "announcements.flash.saved");
    }
    match service::confirm(d, &id, 0, &actor, &ctx).await {
        Ok(true) => v.redirect(&to, true, "announcements.flash.scheduled"),
        Ok(false) => v.redirect(&to, true, "announcements.flash.sending"),
        Err(e) => v.error(&e),
    }
}

pub(super) async fn edit_page(
    State(s): State<AppState>,
    staff: CurrentStaff,
    Path(id): Path<String>,
) -> Response {
    let v = View::new(&s, &staff);
    let svc = &s.services;
    let (row, texts) = match store::load(svc, &id).await {
        Ok(Some(row)) => match store::texts(svc, &id).await {
            Ok(t) => (row, t),
            Err(e) => return v.error(&e),
        },
        Ok(None) => return v.not_found(),
        Err(e) => return v.error(&e),
    };
    if !editable(&row) {
        return v.redirect(&detail(&id), false, "announcements.flash.not_editable");
    }
    let fields = Fields::of(&row, &texts, v.offset());
    form_page(&v, StatusCode::OK, Some(&row), &fields, &[])
}

pub(super) async fn edit_post(
    State(s): State<AppState>,
    staff: CurrentStaff,
    ctx: RequestCtx,
    Path(id): Path<String>,
    Form(pairs): Posted,
) -> Response {
    let v = View::new(&s, &staff);
    let d = s.deployment.as_ref();
    let row = match store::load(d.services(), &id).await {
        Ok(Some(row)) => row,
        Ok(None) => return v.not_found(),
        Err(e) => return v.error(&e),
    };
    let mut f = Fields::from_pairs(&pairs);
    if row.status == "scheduled" {
        f.when = "later".into();
    }
    if row.status == "sent" {
        // Only texts and expiry change once sent.
        f.attrs = row.audience.attributes.clone();
        f.tags = row.audience.tags.join(", ");
        f.unreachable = row.audience.include_unreachable;
        f.when = "now".into();
    }
    let draft = match f.draft(&d.config().roster, v.offset(), d.services().now_ms()) {
        Ok(draft) => draft,
        Err(errors) => {
            return form_page(
                &v,
                StatusCode::UNPROCESSABLE_ENTITY,
                Some(&row),
                &f,
                &errors,
            );
        }
    };
    let version = f.version.unwrap_or(-1);
    let actor = Actor::staff(&staff);
    match service::update(d, &id, version, &draft, &actor, &ctx).await {
        Ok(()) => v.redirect(&detail(&id), true, "announcements.flash.updated"),
        Err(Error::Conflict(_)) => v.redirect(&detail(&id), false, "announcements.flash.conflict"),
        Err(Error::Invalid(_)) => {
            v.redirect(&detail(&id), false, "announcements.flash.not_editable")
        }
        Err(e) => v.error(&e),
    }
}

/// The live count fragment for the posted form.
pub(super) async fn audience_fragment(
    State(s): State<AppState>,
    staff: CurrentStaff,
    Form(pairs): Posted,
) -> Response {
    let v = View::new(&s, &staff);
    let a = Fields::from_pairs(&pairs).audience();
    let counts = audience::count(&s.deployment, &a).await.ok();
    let kinds: Vec<String> = counts
        .iter()
        .flat_map(|c| c.per_channel.keys().map(|k| k.to_string()))
        .collect();
    let mut used = Vec::new();
    for kind in kinds {
        if let Ok(m) = usage::month_summary(&s.deployment, &kind).await {
            used.push((kind, m));
        }
    }
    (
        StatusCode::OK,
        Html(count_box(&v, counts.as_ref(), &used).into_string()),
    )
        .into_response()
}

fn detail(id: &str) -> String {
    format!("/admin/announcements/{id}")
}

fn editable(row: &Row) -> bool {
    matches!(row.status.as_str(), "draft" | "scheduled" | "sent")
}

/// Count, per Channel, and "จะใช้ push ประมาณ x ครั้ง": one metered unit per
/// recipient on a metered Channel (one Card is one Push request).
/// `used`: this month's usage per Channel (#26), shown beside the estimate.
fn count_box(v: &View<'_>, counts: Option<&Counts>, used: &[(String, MonthSummary)]) -> Markup {
    let Some(c) = counts else {
        return html! {
            div id="audience-count" class="card count" hx-post=(AUDIENCE_PATH)
                hx-trigger="change from:closest form" hx-include="closest form"
                hx-target="this" hx-swap="outerHTML" {
                p class="muted" { (v.t("announcements.form.audience_invalid")) }
            }
        };
    };
    let d = v.s.deployment.as_ref();
    let metered = |kind: &str| {
        d.channel(kind).is_some_and(|ch| ch.multicast().is_some())
            || d.config().quotas.contains_key(kind)
    };
    let units: usize = c
        .per_channel
        .iter()
        .filter(|(k, _)| metered(k))
        .map(|(_, (active, _))| *active)
        .sum();
    html! {
        div id="audience-count" class="card count" hx-post=(AUDIENCE_PATH)
            hx-trigger="change from:closest form" hx-include="closest form"
            hx-target="this" hx-swap="outerHTML" aria-live="polite" {
            p { strong { (v.t("announcements.form.students")) " " (c.students) } }
            ul class="list" {
                @for (kind, (active, unreachable)) in &c.per_channel {
                    li {
                        (channel_label(kind)) ": " (active) " " (v.t("announcements.form.links"))
                        @if *unreachable > 0 {
                            " (" (unreachable) " " (v.t("announcements.form.unreachable_skipped")) ")"
                        }
                        @if metered(kind) {
                            " · " (v.t("announcements.form.estimate_channel")) " " (active)
                        }
                        @if let Some((_, m)) = used.iter().find(|(k, _)| k == kind) {
                            " · " span class="month-used" { (month_used(v, m)) }
                        }
                    }
                }
            }
            p class="estimate" { (v.t("announcements.form.estimate")) " " (units) " " (v.t("announcements.form.times")) }
        }
    }
}

/// "ใช้ไปแล้ว X/Y เดือนนี้" (or without a Quota, "ใช้ไปแล้ว X เดือนนี้").
fn month_used(v: &View<'_>, m: &MonthSummary) -> String {
    let used = m.used.to_string();
    match m.cap {
        Some(cap) => fill(
            &v.t("kernel.usage.send_used"),
            &[("used", &used), ("cap", &cap.to_string())],
        ),
        None => fill(&v.t("kernel.usage.send_used_no_cap"), &[("used", &used)]),
    }
}

fn form_page(
    v: &View<'_>,
    status: StatusCode,
    row: Option<&Row>,
    f: &Fields,
    errors: &[(&'static str, &'static str)],
) -> Response {
    let title = v.t(if row.is_some() {
        "announcements.page.edit"
    } else {
        "announcements.page.new"
    });
    let action = row.map_or_else(
        || "/admin/announcements/new".to_string(),
        |r| format!("{}/edit", detail(&r.id)),
    );
    let sent = row.is_some_and(|r| r.status == "sent");
    let scheduled = row.is_some_and(|r| r.status == "scheduled");
    let err = |field: &str| -> Markup {
        html! { @for (_, key) in errors.iter().filter(|(f, _)| *f == field) { p class="field-error" role="alert" { (v.t(key)) } } }
    };
    let roster = &v.s.config.roster;
    let body = html! {
        h1 { (title) }
        @if sent { p class="muted" { (v.t("announcements.form.sent_note")) } }
        form method="post" action=(action) class="card announcement-form" {
            (v.staff.csrf_field())
            @if let Some(version) = f.version { input type="hidden" name="version" value=(version); }
            fieldset {
                legend { (v.t("announcements.form.thai")) }
                (err("th"))
                div class="field" {
                    label for="title_th" { (v.t("announcements.form.title")) }
                    input id="title_th" name="title_th" required maxlength=(MAX_TITLE) value=(f.title_th);
                }
                div class="field" {
                    label for="body_th" { (v.t("announcements.form.body")) }
                    textarea id="body_th" name="body_th" required maxlength=(MAX_BODY) rows="5" { (f.body_th) }
                }
            }
            fieldset {
                legend { (v.t("announcements.form.english")) }
                (err("en"))
                div class="field" {
                    label for="title_en" { (v.t("announcements.form.title")) }
                    input id="title_en" name="title_en" maxlength=(MAX_TITLE) value=(f.title_en);
                }
                div class="field" {
                    label for="body_en" { (v.t("announcements.form.body")) }
                    textarea id="body_en" name="body_en" maxlength=(MAX_BODY) rows="5" { (f.body_en) }
                }
            }
            @if !sent {
                label class="check" {
                    input type="checkbox" name="require_ack" value="1" checked[f.require_ack];
                    " " (v.t("announcements.form.require_ack"))
                }
            } @else if f.require_ack {
                input type="hidden" name="require_ack" value="1";
            }
            div class="field" {
                label for="expires" { (v.t("announcements.form.expires")) }
                (err("expires"))
                input id="expires" type="date" name="expires" value=(f.expires);
            }
            @if !sent {
                fieldset {
                    legend { (v.t("announcements.form.when")) }
                    (err("send_at"))
                    @if !scheduled {
                        label class="check" {
                            input type="radio" name="when" value="now" checked[f.when != "later"];
                            " " (v.t("announcements.form.now"))
                        }
                    }
                    label class="check" {
                        input type="radio" name="when" value="later" checked[f.when == "later"];
                        " " (v.t("announcements.form.later"))
                    }
                    input type="datetime-local" name="send_at" value=(f.send_at)
                        aria-label=(v.t("announcements.form.later"));
                }
                fieldset {
                    legend { (v.t("announcements.form.audience")) }
                    (err("audience"))
                    p class="muted" { (v.t("announcements.form.audience_help")) }
                    @for (key, spec) in roster.attributes_in_order() {
                        @let chosen = f.attrs.get(key).cloned().unwrap_or_default();
                        div class="field" {
                            span class="label" { (spec.label_in(key, &v.staff.locale, &v.s.config.default_locale)) }
                            @if spec.kind == AttributeType::Enum {
                                @for value in &spec.values {
                                    label class="check" {
                                        input type="checkbox" name=(format!("attr.{key}")) value=(value)
                                            checked[chosen.contains(value)];
                                        " " (value)
                                    }
                                }
                            } @else {
                                input name=(format!("attr.{key}")) value=(chosen.join(", "))
                                    placeholder=(v.t("announcements.form.any_of"));
                            }
                        }
                    }
                    div class="field" {
                        label for="tags" { (v.t("announcements.form.tags")) }
                        input id="tags" name="tags" value=(f.tags) placeholder=(v.t("announcements.form.any_of"));
                    }
                    label class="check" {
                        input type="checkbox" name="unreachable" value="1" checked[f.unreachable];
                        " " (v.t("announcements.form.include_unreachable"))
                    }
                    (count_box_placeholder(v))
                }
            }
            (page_detail::preview(v, &f.title_th, &f.body_th, f.require_ack))
            div class="actions" {
                @if row.is_none() {
                    button type="submit" name="do" value="draft" class="secondary" { (v.t("announcements.form.save_draft")) }
                    button type="submit" name="do" value="confirm" { (v.t("announcements.form.confirm")) }
                } @else {
                    button type="submit" { (v.t("announcements.form.save")) }
                }
                a class="btn secondary" href=(row.map_or_else(|| "/admin/announcements".to_string(), |r| detail(&r.id))) {
                    (v.t("announcements.page.cancel"))
                }
            }
        }
    };
    v.page(status, &title, body)
}

/// The count box before its first HTMX load.
fn count_box_placeholder(v: &View<'_>) -> Markup {
    html! {
        div id="audience-count" class="card count" hx-post=(AUDIENCE_PATH)
            hx-trigger="load, change from:closest form" hx-include="closest form"
            hx-target="this" hx-swap="outerHTML" aria-live="polite" {
            p class="muted" { (v.t("announcements.form.counting")) }
        }
    }
}

//! `GET/POST /admin/announcements/new` and `GET/POST …/{id}/edit`. The
//! live Audience count is [`super::page_count`], the Student picker
//! [`super::page_pick`].

use axum::Form;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::Response;
use maud::{Markup, html};

use super::draft::changed_locales;
use super::form::{Fields, MAX_BODY, MAX_TITLE};
use super::notice;
use super::store::{self, Row};
use super::view::View;
use super::{page_count, page_detail, page_notice, page_pick, service};
use crate::config::AttributeType;
use crate::{Actor, AppState, CurrentStaff, Error, RequestCtx};

type Posted = Form<Vec<(String, String)>>;

pub(super) async fn new_page(State(s): State<AppState>, staff: CurrentStaff) -> Response {
    let v = View::new(&s, &staff);
    let fields = Fields {
        when: "now".into(),
        ..Fields::default()
    };
    form_page(&v, StatusCode::OK, None, &fields, &[], None)
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
        Err(errors) => {
            return form_page(
                &v,
                StatusCode::UNPROCESSABLE_ENTITY,
                None,
                &f,
                &errors,
                None,
            );
        }
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
    let cost = match page_notice::cost(&s.deployment, &row).await {
        Ok(c) => c,
        Err(e) => return v.error(&e),
    };
    form_page(&v, StatusCode::OK, Some(&row), &fields, &[], cost.as_ref())
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
        f.students = row.audience.students.join("\n");
        f.unreachable = row.audience.include_unreachable;
        f.when = "now".into();
    }
    let now = d.services().now_ms();
    let mut draft = f.draft(&d.config().roster, v.offset(), now);
    // #77: a notice needs an announcement open now and after this save
    // (Students could not open it otherwise) and a changed text (else it
    // would only cost Pushes); a box ticked in vain is an error, not a
    // silent drop.
    if let Ok(dr) = &draft
        && row.status == "sent"
        && f.notify
    {
        let closes = !notice::open(&row, now) || dr.expires_at.is_some_and(|e| e <= now);
        match store::texts(d.services(), &id).await {
            Ok(_) if closes => {
                draft = Err(vec![("notify", "announcements.form.notify_closed")]);
            }
            Ok(old) if changed_locales(&old, dr).is_empty() => {
                draft = Err(vec![("notify", "announcements.form.notify_unchanged")]);
            }
            Ok(_) => {}
            Err(e) => return v.error(&e),
        }
    }
    let draft = match draft {
        Ok(draft) => draft,
        Err(errors) => {
            let cost = page_notice::cost(d, &row).await.ok().flatten();
            return form_page(
                &v,
                StatusCode::UNPROCESSABLE_ENTITY,
                Some(&row),
                &f,
                &errors,
                cost.as_ref(),
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

fn detail(id: &str) -> String {
    format!("/admin/announcements/{id}")
}

fn editable(row: &Row) -> bool {
    matches!(row.status.as_str(), "draft" | "scheduled" | "sent")
}

fn form_page(
    v: &View<'_>,
    status: StatusCode,
    row: Option<&Row>,
    f: &Fields,
    errors: &[(&'static str, &'static str)],
    cost: Option<&page_notice::Cost>,
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
            // #77: LINE cannot edit a sent Card; one Push per recipient.
            // Not offered once expired: Students could not open it.
            @if sent && row.is_some_and(|r| notice::open(r, v.s.services.now_ms())) {
                (page_notice::field(v, f, cost, err("notify")))
            } @else {
                (err("notify"))
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
                    (page_pick::field(v, f))
                    label class="check" {
                        input type="checkbox" name="unreachable" value="1" checked[f.unreachable];
                        " " (v.t("announcements.form.include_unreachable"))
                    }
                    (page_count::placeholder(v))
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

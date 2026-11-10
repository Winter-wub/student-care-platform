//! `GET/POST /admin/announcements/new` and `GET/POST …/{id}/edit`. The
//! live Audience count is [`super::page_count`], the Student picker
//! [`super::page_pick`].

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::Response;
use maud::{Markup, html};

use super::draft::changed_locales;
use super::files::{self, Stored};
use super::form::{Fields, MAX_BODY, MAX_TITLE};
use super::form_images::{self, Chosen, Posted};
use super::form_pdfs;
use super::notice;
use super::store::{self, Row};
use super::view::View;
use super::{
    page_audience, page_detail, page_editor, page_more, page_notice, page_review, service,
};
use crate::{Actor, AppState, CurrentStaff, Error, RequestCtx};

/// Errors as (field, catalog key).
type Errors = Vec<(&'static str, &'static str)>;

/// The draft of `f` with the posted files (images, PDFs), or every
/// problem of both.
fn with_images(
    draft: Result<service::Draft, Errors>,
    posted: &mut Posted,
) -> Result<service::Draft, Errors> {
    match (draft, posted.file_edits()) {
        (Ok(mut d), Ok(files)) => {
            d.files = files;
            Ok(d)
        }
        (Err(mut a), Err(b)) => {
            a.extend(b);
            Err(super::form::unique(a))
        }
        (Err(e), Ok(_)) | (Ok(_), Err(e)) => Err(e),
    }
}

/// A new draft; `?scope=students&students=…` prefills the Audience
/// ([`super::form_scope::prefilled`], #127), and so does a Students list
/// filter (`from=students`, [`super::form_scope::from_students`], #145).
pub(super) async fn new_page(
    State(s): State<AppState>,
    staff: CurrentStaff,
    Query(q): Query<Vec<(String, String)>>,
) -> Response {
    let v = View::new(&s, &staff);
    let mut fields = super::form_scope::prefilled(&q);
    if let Some(codes) = super::form_scope::from_students(&s, &staff, &q).await {
        fields.scope = "students".into();
        fields.students = codes.join("\n");
    }
    let form = Form::new(None, &fields);
    form_page(&v, StatusCode::OK, &form, &[], None)
}

pub(super) async fn new_post(
    State(s): State<AppState>,
    staff: CurrentStaff,
    ctx: RequestCtx,
    mut posted: Posted,
) -> Response {
    let v = View::new(&s, &staff);
    let f = Fields::from_pairs(&posted.pairs);
    let d = s.deployment.as_ref();
    let draft = f.draft(&d.config().roster, v.offset(), d.services().now_ms());
    let lost = posted.chosen();
    let draft = match with_images(draft, &mut posted) {
        Ok(draft) => draft,
        Err(errors) => {
            let form = Form {
                reselect: lost,
                ..Form::new(None, &f)
            };
            return form_page(&v, StatusCode::UNPROCESSABLE_ENTITY, &form, &errors, None);
        }
    };
    let actor = Actor::staff(&staff);
    let id = match service::create(d, draft, &actor, &ctx).await {
        Ok(id) => id,
        Err(e) => return v.error(&e),
    };
    if f.submit == "confirm" {
        // #120: saved as a draft, then the review step before any send.
        return v.redirect(&page_review::path(&id), true, "announcements.flash.saved");
    }
    v.redirect(&detail(&id), true, "announcements.flash.saved")
}

pub(super) async fn edit_page(
    State(s): State<AppState>,
    staff: CurrentStaff,
    Path(id): Path<String>,
) -> Response {
    let v = View::new(&s, &staff);
    let svc = &s.services;
    let (row, texts, images) = match store::load(svc, &id).await {
        Ok(Some(row)) => match (store::texts(svc, &id).await, files::list(svc, &id).await) {
            (Ok(t), Ok(i)) => (row, t, i),
            (Err(e), _) | (_, Err(e)) => return v.error(&e),
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
    let form = Form {
        images: &images,
        ..Form::new(Some(&row), &fields)
    };
    form_page(&v, StatusCode::OK, &form, &[], cost.as_ref())
}

pub(super) async fn edit_post(
    State(s): State<AppState>,
    staff: CurrentStaff,
    ctx: RequestCtx,
    Path(id): Path<String>,
    mut posted: Posted,
) -> Response {
    let v = View::new(&s, &staff);
    let d = s.deployment.as_ref();
    let row = match store::load(d.services(), &id).await {
        Ok(Some(row)) => row,
        Ok(None) => return v.not_found(),
        Err(e) => return v.error(&e),
    };
    let mut f = Fields::from_pairs(&posted.pairs);
    if row.status == "scheduled" {
        f.when = "later".into();
    }
    if row.status == "sent" {
        // Only texts and expiry change once sent.
        f.attrs = row.audience.attributes.clone();
        f.tags = row.audience.tags.join(", ");
        f.students = row.audience.students.join("\n");
        f.unreachable = row.audience.include_unreachable;
        f.scope = super::form::scope_of(&row.audience).into();
        f.when = "now".into();
    }
    let now = d.services().now_ms();
    let lost = posted.chosen();
    let mut draft = with_images(f.draft(&d.config().roster, v.offset(), now), &mut posted);
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
            let images = files::list(d.services(), &id).await.unwrap_or_default();
            let form = Form {
                images: &images,
                reselect: lost,
                ..Form::new(Some(&row), &f)
            };
            return form_page(
                &v,
                StatusCode::UNPROCESSABLE_ENTITY,
                &form,
                &errors,
                cost.as_ref(),
            );
        }
    };
    let version = f.version.unwrap_or(-1);
    let actor = Actor::staff(&staff);
    // #120: a schedule whose Audience changes is held as a draft for review.
    if row.status == "scheduled" && draft.audience != row.audience {
        return page_review::hold(&v, d, &id, version, draft, &actor, &ctx).await;
    }
    match service::update(d, &id, version, draft, &actor, &ctx).await {
        Ok(()) if f.submit == "confirm" && row.status == "draft" => {
            v.redirect(&page_review::path(&id), true, "announcements.flash.updated")
        }
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

/// What the form shows: the announcement edited (if any), the fields,
/// its saved images, and whether files chosen were lost (#109).
struct Form<'a> {
    row: Option<&'a Row>,
    f: &'a Fields,
    images: &'a [Stored],
    reselect: Chosen,
}

impl<'a> Form<'a> {
    fn new(row: Option<&'a Row>, f: &'a Fields) -> Self {
        Self {
            row,
            f,
            images: &[],
            reselect: Chosen::default(),
        }
    }
}

fn form_page(
    v: &View<'_>,
    status: StatusCode,
    form: &Form<'_>,
    errors: &[(&'static str, &'static str)],
    cost: Option<&page_notice::Cost>,
) -> Response {
    let (row, f) = (form.row, form.f);
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
    let err = |field: &str| -> Markup { super::form_scope::field_errors(v, errors, field) };
    let d = |field: &str| super::form_scope::described(errors, field);
    let open = page_more::Open::of(f, form.images, errors, form.reselect);
    let body = html! {
        h1 { (title) }
        @if sent { p class="muted" { (v.t("announcements.form.sent_note")) } }
        form method="post" action=(action) enctype="multipart/form-data" class="card announcement-form" {
            (v.staff.csrf_field())
            @if !errors.is_empty() || form.reselect.any() { p class="flash err" role="alert" { (v.t("announcements.form.fix_errors")) } }
            @if let Some(version) = f.version { input type="hidden" name="version" value=(version); }
            @if let Some(r) = row { input type="hidden" name="announcement_id" value=(r.id); }
            fieldset aria-describedby=[d("th")] {
                legend { (v.t("announcements.form.thai")) }
                (err("th"))
                div class="field" {
                    label for="title_th" { (v.t("announcements.form.title")) }
                    input id="title_th" name="title_th" required maxlength=(MAX_TITLE) value=(f.title_th);
                }
                div class="field" {
                    label for="body_th" { (v.t("announcements.form.body")) }
                    (page_editor::toolbar(v, "body_th"))
                    textarea id="body_th" name="body_th" required maxlength=(MAX_BODY) rows="5" { (f.body_th) }
                    (page_editor::help(v))
                }
            }
            // #77: LINE cannot edit a sent Card; one Push per recipient.
            // Not offered once expired: Students could not open it.
            @if sent && row.is_some_and(|r| notice::open(r, v.s.services.now_ms())) {
                (page_notice::field(v, f, cost, err("notify")))
            } @else {
                (err("notify"))
            }
            // #140: who and when stay in view, under the Thai text.
            @if !sent {
                (page_audience::fieldset(v, f, errors))
                fieldset aria-describedby=[d("send_at")] {
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
            }
            // #140: the optional parts, each open when it holds a value
            // or an error ([`page_more::Open`]).
            (page_more::part("more-english", &v.t("announcements.form.english"), open.english, html! {
                fieldset aria-describedby=[d("en")] {
                    legend { (v.t("announcements.form.english")) }
                    (err("en"))
                    div class="field" {
                        label for="title_en" { (v.t("announcements.form.title")) }
                        input id="title_en" name="title_en" maxlength=(MAX_TITLE) value=(f.title_en);
                    }
                    div class="field" {
                        label for="body_en" { (v.t("announcements.form.body")) }
                        (page_editor::toolbar(v, "body_en"))
                        textarea id="body_en" name="body_en" maxlength=(MAX_BODY) rows="5" { (f.body_en) }
                        (page_editor::help(v))
                    }
                }
            }))
            (page_more::part("more-images", &v.t("announcements.editor.images_summary"), open.images,
                form_images::field(v, form.images, &[&f.body_th, &f.body_en], sent, form.reselect.images, &err)))
            (page_more::part("more-pdfs", &v.t("announcements.editor.pdfs_summary"), open.pdfs,
                form_pdfs::field(v, form.images, sent, form.reselect.pdfs, &err)))
            (page_more::part("more-options", &v.t("announcements.editor.more_summary"), open.more, html! {
                div class="field" {
                    label for="expires" { (v.t("announcements.form.expires")) }
                    (err("expires"))
                    input id="expires" type="date" name="expires" value=(f.expires) aria-describedby=[d("expires")];
                }
                @if !sent {
                    label class="check" {
                        input type="checkbox" name="require_ack" value="1" checked[f.require_ack];
                        " " (v.t("announcements.form.require_ack"))
                    }
                }
            }))
            @if sent && f.require_ack {
                input type="hidden" name="require_ack" value="1";
            }
            (page_editor::preview(v, f, form.images))
            (page_detail::preview(v, &f.title_th, &f.body_th, f.require_ack))
            div class="actions" {
                @if row.is_none() {
                    button type="submit" name="do" value="draft" class="secondary" { (v.t("announcements.form.save_draft")) }
                    button type="submit" name="do" value="confirm" { (v.t("announcements.form.confirm")) }
                } @else if row.is_some_and(|r| r.status == "draft") {
                    button type="submit" class="secondary" { (v.t("announcements.form.save")) }
                    button type="submit" name="do" value="confirm" { (v.t("announcements.form.confirm")) }
                } @else {
                    button type="submit" { (v.t("announcements.form.save")) }
                }
                a class="btn secondary" href=(row.map_or_else(|| "/admin/announcements".to_string(), |r| detail(&r.id))) {
                    (v.t("announcements.page.cancel"))
                }
            }
        }
        (page_editor::script())
    };
    v.page(status, &title, body)
}

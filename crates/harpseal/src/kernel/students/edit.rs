//! `GET|POST /admin/students/{id}/edit` and `POST .../override`
//! ("ใช้ค่าจากทะเบียน"). Invalid input re-renders the form (422) with a
//! message per field and writes nothing.

use axum::Form;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::Response;
use maud::{Markup, html};

use super::form::{FormErrors, FormMap, attr_input, parse_edit};
use super::read::{self, StudentRow};
use super::view::View;
use super::{StudentFields, attribute_field, service};
use crate::config::{AttributeSpec, AttributeType};
use crate::{Actor, AppState, CurrentStaff, Error, RequestCtx};

pub(super) async fn form(
    State(s): State<AppState>,
    staff: CurrentStaff,
    Path(id): Path<String>,
) -> Response {
    let v = View::new(&s, &staff);
    match read::student(&s.services, &id).await {
        Ok(Some(row)) => {
            let values = values_of(&v, &row.fields);
            render(&v, &row, &values, &FormErrors::new(), StatusCode::OK)
        }
        Ok(None) => v.not_found(),
        Err(e) => v.error(&e),
    }
}

pub(super) async fn submit(
    State(s): State<AppState>,
    staff: CurrentStaff,
    ctx: RequestCtx,
    Path(id): Path<String>,
    Form(pairs): Form<Vec<(String, String)>>,
) -> Response {
    let v = View::new(&s, &staff);
    let row = match read::student(&s.services, &id).await {
        Ok(Some(row)) => row,
        Ok(None) => return v.not_found(),
        Err(e) => return v.error(&e),
    };
    let values: FormMap = pairs.into_iter().collect();
    let fields = match parse_edit(&s.config.roster, &row.fields, &values) {
        Ok(f) => f,
        Err(errors) => {
            return render(&v, &row, &values, &errors, StatusCode::UNPROCESSABLE_ENTITY);
        }
    };
    let detail = format!("/admin/students/{id}");
    let actor = Actor::staff(&staff);
    match service::update(&s.deployment, &id, &fields, &actor, &ctx).await {
        Ok(changed) if changed.is_empty() => v.redirect(&detail, true, "kernel.students.no_change"),
        Ok(_) => v.redirect(&detail, true, "kernel.students.saved"),
        Err(Error::Conflict(_)) => {
            v.redirect(&format!("{detail}/edit"), false, "kernel.students.conflict")
        }
        Err(Error::Invalid(_)) => v.not_found(),
        Err(e) => v.error(&e),
    }
}

/// "ใช้ค่าจากทะเบียน": the button sits inside the edit form and names the
/// field in `field`; the other fields are ignored.
pub(super) async fn clear(
    State(s): State<AppState>,
    staff: CurrentStaff,
    ctx: RequestCtx,
    Path(id): Path<String>,
    Form(pairs): Form<Vec<(String, String)>>,
) -> Response {
    let v = View::new(&s, &staff);
    let field = pairs
        .into_iter()
        .rev()
        .find(|(k, _)| k == "field")
        .map(|(_, f)| f)
        .unwrap_or_default();
    let edit = format!("/admin/students/{id}/edit");
    let actor = Actor::staff(&staff);
    match service::clear_override(&s.deployment, &id, &field, &actor, &ctx).await {
        Ok(()) => v.redirect(&edit, true, "kernel.students.override_cleared"),
        Err(Error::Conflict(_)) => v.redirect(&edit, false, "kernel.students.conflict"),
        Err(Error::Invalid(_)) => v.not_found(),
        Err(e) => v.error(&e),
    }
}

/// The stored fields as form values.
fn values_of(v: &View<'_>, f: &StudentFields) -> FormMap {
    let mut m = FormMap::new();
    m.insert("display_name".into(), f.display_name.clone());
    m.insert("email".into(), f.email.clone().unwrap_or_default());
    m.insert("locale".into(), f.locale.0.clone());
    for key in v.s.config.roster.attributes.keys() {
        let value = AttributeSpec::display(f.attributes.get(key));
        m.insert(attr_input(key), value);
    }
    m
}

/// One labelled field: override badge, the input, its error and, when
/// overridden, the "ใช้ค่าจากทะเบียน" button.
struct Field<'a> {
    input: String,
    override_key: String,
    label: &'a str,
    control: Markup,
}

fn render(
    v: &View<'_>,
    row: &StudentRow,
    values: &FormMap,
    errors: &FormErrors,
    status: StatusCode,
) -> Response {
    let value = |k: &str| values.get(k).cloned().unwrap_or_default();
    let text_input = |name: &str, kind: &str| {
        let err = errors.contains_key(name);
        html! {
            input id=(name) name=(name) type=(kind) value=(value(name))
                aria-invalid=[err.then_some("true")];
        }
    };
    let (t_name, t_email, t_locale) = (
        v.t("kernel.students.name"),
        v.t("kernel.students.email"),
        v.t("kernel.students.locale"),
    );
    let mut locales: Vec<String> = vec!["th".into(), "en".into()];
    for l in [v.s.config.default_locale.0.clone(), value("locale")] {
        if !l.is_empty() && !locales.contains(&l) {
            locales.push(l);
        }
    }
    let current_locale = value("locale");
    let mut fields = vec![
        Field {
            input: "display_name".into(),
            override_key: "display_name".into(),
            label: &t_name,
            control: text_input("display_name", "text"),
        },
        Field {
            input: "email".into(),
            override_key: "email".into(),
            label: &t_email,
            control: text_input("email", "email"),
        },
        Field {
            input: "locale".into(),
            override_key: "locale".into(),
            label: &t_locale,
            control: html! {
                select id="locale" name="locale" {
                    @for l in &locales {
                        option value=(l) selected[*l == current_locale] { (l) }
                    }
                }
            },
        },
    ];
    for (key, spec) in v.s.config.roster.attributes_in_order() {
        let name = attr_input(key);
        let control = match spec.kind {
            AttributeType::Enum => {
                let cur = value(&name);
                html! {
                    select id=(name) name=(name) {
                        @if spec.optional || cur.is_empty() { option value="" { "-" } }
                        @for opt in &spec.values {
                            option value=(opt) selected[*opt == cur] { (opt) }
                        }
                    }
                }
            }
            AttributeType::Date => text_input(&name, "date"),
            AttributeType::Integer | AttributeType::Number => text_input(&name, "text"),
            _ => text_input(&name, "text"),
        };
        fields.push(Field {
            input: name,
            override_key: attribute_field(key),
            label: v.label(key, spec),
            control,
        });
    }
    let title = format!(
        "{} · {}",
        v.t("kernel.students.edit"),
        row.fields.display_name
    );
    let clear_action = format!("/admin/students/{}/override", row.id);
    let body = html! {
        h1 { (title) }
        @if !errors.is_empty() {
            div class="flash err" role="alert" { (v.t("kernel.students.fix_errors")) }
        }
        form method="post" action=(format!("/admin/students/{}/edit", row.id)) class="card" {
            (v.staff.csrf_field())
            // Enter in a field submits the first submit button: make it Save.
            button class="sr-default" type="submit" tabindex="-1" aria-hidden="true" { (v.t("kernel.students.save")) }
            p class="muted" { (v.t("kernel.students.override_help")) }
            div class="field" {
                label { (v.t("kernel.students.code")) }
                p class="code" { (row.code) " " span class="muted" { "(" (v.t("kernel.students.code_readonly")) ")" } }
            }
            @for f in &fields {
                @let overridden = row.overrides.contains(&f.override_key);
                div class="field" {
                    label for=(f.input) {
                        (f.label)
                        @if overridden { " " span class="chip override" { (v.t("kernel.students.override")) } }
                    }
                    (f.control)
                    @if let Some(key) = errors.get(&f.input) {
                        p class="field-error" role="alert" { (v.t(key)) }
                    }
                    @if overridden {
                        button class="link" type="submit" formaction=(clear_action)
                            name="field" value=(f.override_key) formnovalidate {
                            (v.t("kernel.students.use_roster"))
                        }
                    }
                }
            }
            div class="actions" {
                button type="submit" { (v.t("kernel.students.save")) }
                a class="btn secondary" href=(format!("/admin/students/{}", row.id)) { (v.t("kernel.students.cancel")) }
            }
        }
    };
    v.page(status, &title, body)
}

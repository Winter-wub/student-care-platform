//! `GET /admin/roster`: the last 20 imports, the upload form (Staff with
//! `kernel.roster.import`) with the expected headers, and the template
//! link. `POST /admin/roster/upload` stores and previews a file.
//! `GET /admin/roster/template.csv` is the header row.

use axum::extract::{Multipart, State};
use axum::http::StatusCode;
use axum::response::Response;
use maud::{Markup, html};

use super::file::expected_headers;
use super::preview::{self, MAX_FILE_BYTES, Uploaded};
use super::store::{self, Import};
use super::view::{BASE, IMPORT, View, csv_body, download};
use crate::config::{AttributeType, RosterConfig};
use crate::{AppState, CurrentStaff, RequestCtx};

pub(super) async fn page(State(s): State<AppState>, staff: CurrentStaff) -> Response {
    let v = View::new(&s, &staff);
    let imports = match store::recent(&s.services, 20).await {
        Ok(i) => i,
        Err(e) => return v.error(&e),
    };
    let title = v.t("kernel.roster.title");
    let mapped = super::has_mapping(&s.config.roster);
    let body = html! {
        h1 { (title) }
        @if !mapped {
            div class="warn" role="status" { (v.t("kernel.roster.no_roster")) }
        } @else if staff.can(IMPORT) {
            (upload_form(&v))
        }
        @if mapped { (headers(&v, &s.config.roster)) }
        h2 { (v.t("kernel.roster.recent")) }
        (list(&v, &imports))
    };
    v.page(StatusCode::OK, &title, body)
}

/// The modes the form offers: the allowed ones, `full` first, the
/// Deployment's default preselected.
fn upload_form(v: &View<'_>) -> Markup {
    let roster = &v.s.config.roster;
    let offered = ["full", "upsert"]
        .into_iter()
        .filter(|m| roster.allows_mode(m));
    html! {
        form class="card" method="post" action=(format!("{BASE}/upload"))
            enctype="multipart/form-data" {
            (v.staff.csrf_field())
            h2 { (v.t("kernel.roster.upload_title")) }
            div class="field" {
                label for="roster-file" { (v.t("kernel.roster.file")) }
                input id="roster-file" type="file" name="file" required
                    accept=".csv,.xlsx,text/csv,application/vnd.openxmlformats-officedocument.spreadsheetml.sheet";
            }
            fieldset class="field" {
                legend { (v.t("kernel.roster.mode")) }
                @for mode in offered {
                    label class="choice" {
                        input type="radio" name="mode" value=(mode)
                            checked[mode == roster.default_mode];
                        span { (v.t(&format!("kernel.roster.mode_{mode}_help"))) }
                    }
                }
            }
            button type="submit" { (v.t("kernel.roster.check")) }
        }
    }
}

/// The headers table: what each column holds and whether it may be empty.
fn headers(v: &View<'_>, roster: &RosterConfig) -> Markup {
    let mut rows: Vec<(String, String, String)> = vec![
        (
            roster.student_code.clone(),
            v.t("kernel.students.code"),
            v.t("kernel.roster.required"),
        ),
        (
            roster.display_name.clone(),
            v.t("kernel.students.name"),
            v.t("kernel.roster.required"),
        ),
        (
            roster.email.clone(),
            v.t("kernel.students.email"),
            v.t("kernel.roster.optional"),
        ),
    ];
    if let Some(l) = &roster.locale {
        rows.push((
            l.column.clone(),
            v.t("kernel.students.locale"),
            v.t("kernel.roster.optional"),
        ));
    }
    if let Some(t) = &roster.tags {
        let holds = format!(
            "{} ({} \"{}\")",
            v.t("kernel.students.tags"),
            v.t("kernel.roster.separator"),
            t.separator
        );
        rows.push((t.column.clone(), holds, v.t("kernel.roster.optional")));
    }
    for (key, a) in roster.attributes_in_order() {
        let mut holds = a
            .label_in(key, &v.staff.locale, &v.s.config.default_locale)
            .to_string();
        if a.kind == AttributeType::Enum {
            let (locale, default) = (&v.staff.locale, &v.s.config.default_locale);
            let all: Vec<String> = a
                .values
                .iter()
                .map(|x| a.labelled(x, locale, default))
                .collect();
            holds = format!(
                "{holds} ({} {})",
                v.t("kernel.roster.one_of"),
                all.join(", ")
            );
        }
        if let Some(range) = a.range_text() {
            holds = format!("{holds} ({range})");
        }
        let need = if a.optional {
            "kernel.roster.optional"
        } else {
            "kernel.roster.required"
        };
        rows.push((a.column.clone(), holds, v.t(need)));
    }
    html! {
        details class="card" {
            summary { h2 { (v.t("kernel.roster.expected")) } }
            table class="rows" {
                thead { tr {
                    th { (v.t("kernel.roster.header")) }
                    th { (v.t("kernel.roster.holds")) }
                    th { "" }
                } }
                tbody {
                    @for (h, holds, need) in &rows {
                        tr {
                            td class="code" data-label=(v.t("kernel.roster.header")) { (h) }
                            td data-label=(v.t("kernel.roster.holds")) { (holds) }
                            td data-label="" { (need) }
                        }
                    }
                }
            }
            p { a class="btn secondary" href=(format!("{BASE}/template.csv")) download {
                (v.t("kernel.roster.template"))
            } }
        }
    }
}

fn list(v: &View<'_>, imports: &[Import]) -> Markup {
    if imports.is_empty() {
        return html! { div class="card" { p { (v.t("kernel.roster.none")) } } };
    }
    html! {
        table class="rows" {
            thead { tr {
                th { (v.t("kernel.roster.when")) }
                th { (v.t("kernel.roster.state")) }
                th { (v.t("kernel.roster.mode")) }
                th { (v.t("kernel.roster.rows")) }
                th { (v.t("kernel.roster.who")) }
            } }
            tbody {
                @for i in imports {
                    tr {
                        td data-label=(v.t("kernel.roster.when")) {
                            a class="name" href=(format!("{BASE}/{}", i.id)) { (v.time(i.created_at)) }
                        }
                        td data-label=(v.t("kernel.roster.state")) { (v.state(&i.state)) }
                        td data-label=(v.t("kernel.roster.mode")) { (v.mode(&i.mode)) }
                        td data-label=(v.t("kernel.roster.rows")) {
                            (i.total_rows) " · " span class="muted" { (i.file_name) }
                        }
                        td data-label=(v.t("kernel.roster.who")) { (i.staff) }
                    }
                }
            }
        }
    }
}

/// The header row of a file this Deployment accepts.
pub(super) async fn template(State(s): State<AppState>, staff: CurrentStaff) -> Response {
    let v = View::new(&s, &staff);
    if !super::has_mapping(&s.config.roster) {
        return v.not_found();
    }
    let headers = expected_headers(&s.config.roster);
    let refs: Vec<&str> = headers.iter().map(String::as_str).collect();
    let body = csv_body([super::csv::line(&refs)]);
    download(
        "text/csv; charset=utf-8",
        "roster-template.csv",
        "roster-template.csv",
        body,
    )
}

/// The fields of the upload form.
#[derive(Default)]
struct Upload {
    mode: String,
    name: String,
    bytes: Option<Vec<u8>>,
}

async fn read_upload(mut m: Multipart) -> Option<Upload> {
    let mut up = Upload::default();
    while let Some(field) = m.next_field().await.ok()? {
        match field.name() {
            Some("mode") => up.mode = field.text().await.ok()?,
            Some("file") => {
                up.name = field.file_name().unwrap_or_default().to_string();
                let bytes = field.bytes().await.ok()?;
                if bytes.len() > MAX_FILE_BYTES {
                    return None;
                }
                up.bytes = Some(bytes.to_vec());
            }
            _ => {}
        }
    }
    Some(up)
}

/// 400 in the layout: a mode this Deployment does not allow (`key` says
/// why), with a way back. Nothing was imported or queued.
pub(super) fn refused(v: &View<'_>, key: &str) -> Response {
    let text = v.t(key);
    let body = html! {
        div class="card" {
            p { (text) }
            a class="btn" href=(BASE) { (v.t("kernel.roster.back")) }
        }
    };
    v.page(StatusCode::BAD_REQUEST, &text, body)
}

pub(super) async fn upload(
    State(s): State<AppState>,
    staff: CurrentStaff,
    ctx: RequestCtx,
    m: Multipart,
) -> Response {
    let v = View::new(&s, &staff);
    if !super::has_mapping(&s.config.roster) {
        return v.redirect(BASE, false, "kernel.roster.no_roster");
    }
    let Some(up) = read_upload(m).await else {
        return v.redirect(BASE, false, "kernel.roster.bad_upload");
    };
    let Some(bytes) = up.bytes.filter(|b| !b.is_empty()) else {
        return v.redirect(BASE, false, "kernel.roster.no_file");
    };
    // No mode: the Deployment's default, never `full` implicitly (#79).
    let roster = &s.config.roster;
    let mode = if up.mode.is_empty() {
        roster.default_mode.as_str()
    } else {
        up.mode.as_str()
    };
    if !roster.allows_mode(mode) {
        return refused(&v, "kernel.roster.mode_not_allowed");
    }
    match preview::upload(&s.deployment, &up.name, bytes, mode, &staff.id, &ctx).await {
        Ok(Uploaded::Previewed(id)) => {
            v.redirect(&format!("{BASE}/{id}"), true, "kernel.roster.previewed")
        }
        Ok(Uploaded::Refused(id)) => v.redirect(
            &format!("{BASE}/{id}"),
            false,
            "kernel.roster.flash_refused",
        ),
        Ok(Uploaded::Busy(b)) => {
            v.redirect(&format!("{BASE}/{}", b.id), false, "kernel.roster.busy")
        }
        Err(e) => v.error(&e),
    }
}

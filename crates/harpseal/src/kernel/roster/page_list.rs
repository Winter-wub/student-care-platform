//! `GET /admin/roster`: the last 20 imports, the upload form (Staff with
//! `kernel.roster.import`) with its help beside it (#127: the columns, the
//! file rules and the template link, `page_help`). `POST /admin/roster/upload` stores and previews a file.
//! `GET /admin/roster/template.csv` is the header row.

use axum::extract::{Multipart, State};
use axum::http::StatusCode;
use axum::response::Response;
use maud::{Markup, html};

use super::file::expected_headers;
use super::preview::{self, MAX_FILE_BYTES, Uploaded};
use super::store::{self, Import};
use super::view::{BASE, IMPORT, View, csv_body, download};
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
        } @else {
            // The help beside the input (#127); alone for Staff who may
            // only look.
            div class="cols upload-pair" {
                @if staff.can(IMPORT) { (upload_form(&v)) }
                aside class="card" id="upload-help" aria-label=(v.t("kernel.upload.help_title")) {
                    (super::page_help::panel(&v, true))
                }
            }
        }
        h2 { (v.t("kernel.roster.recent")) }
        (list(&v, &imports))
    };
    v.page(StatusCode::OK, &title, body)
}

/// The modes the form offers: the allowed ones, `upsert` first (#120: a
/// full replace is a deliberate choice), the Deployment's default
/// preselected.
fn upload_form(v: &View<'_>) -> Markup {
    let roster = &v.s.config.roster;
    let offered = ["upsert", "full"]
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

fn list(v: &View<'_>, imports: &[Import]) -> Markup {
    if imports.is_empty() {
        return crate::ui::empty_state(&v.t("kernel.empty.imports"), None);
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
        // A preview with the leave warning says so in its flash, the one
        // message the page announces (#129).
        Ok(Uploaded::Previewed(id, warned)) => {
            let (ok, key) = if warned {
                (false, "kernel.roster.previewed_warning")
            } else {
                (true, "kernel.roster.previewed")
            };
            v.redirect(&format!("{BASE}/{id}"), ok, key)
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

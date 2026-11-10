//! A customer Module's Mini App page (#72): "My info" shows the Student
//! who opened it their own record (student id, name and the
//! `[roster.attributes]` values), linked from the Mini App home.
//!
//! It lists the `[roster.attributes]` marked `student_visible` (programme
//! and year in `deployment.toml`; the GPA stays staff-only).
//!
//! The handler takes [`AppStudent`] and nothing else from the request:
//! the record comes from the App Session, so no URL, query or form value
//! can show another Student's. Without a session the extractor answers
//! the Kernel's entry page, which signs in and comes back here.

use axum::extract::State;
use axum::response::Response;
use axum::routing::get;
use harpseal::web::axum;
use harpseal::{AppState, AppStudent, AttributeSpec, CatalogEntry, Module, ModuleRegistrar};
use maud::html;

/// The page, under `/app/` so it gets the App Session cookie and the
/// Mini App's CSP.
pub const PATH: &str = "/app/my-info";

/// The Module.
pub struct MyInfo;

const TEXTS: &[CatalogEntry] = &[
    CatalogEntry::new("my_info.title", "th", "ข้อมูลของฉัน"),
    CatalogEntry::new("my_info.title", "en", "My info"),
    CatalogEntry::new("my_info.code", "th", "รหัสนักศึกษา"),
    CatalogEntry::new("my_info.code", "en", "Student ID"),
    CatalogEntry::new("my_info.name", "th", "ชื่อ-สกุล"),
    CatalogEntry::new("my_info.name", "en", "Name"),
    CatalogEntry::new("my_info.back", "th", "กลับหน้าของฉัน"),
    CatalogEntry::new("my_info.back", "en", "Back to my page"),
];

impl Module for MyInfo {
    fn name(&self) -> &'static str {
        "my_info"
    }

    fn register(&self, r: &mut ModuleRegistrar) {
        r.messages(TEXTS)
            .public(PATH, get(page))
            .mini_app_link("my_info.title", PATH, 50);
    }
}

async fn page(State(s): State<AppState>, me: AppStudent) -> Response {
    let default = &s.config.default_locale;
    let rows: Vec<(String, String)> = s
        .config
        .roster
        .attributes_in_order()
        .into_iter()
        // `AppStudent::attributes` holds only these.
        .filter(|(_, spec)| spec.student_visible)
        .map(|(key, spec)| {
            let label = spec.label_in(key, &me.locale, default).to_string();
            (label, AttributeSpec::display(me.attributes.get(key)))
        })
        .collect();
    let title = me.t("my_info.title");
    let body = html! {
        section class="card" {
            h1 { (title) }
            dl {
                dt { (me.t("my_info.code")) } dd { (me.student_code) }
                dt { (me.t("my_info.name")) } dd { (me.display_name) }
                @for (label, value) in &rows {
                    dt { (label) } dd { @if value.is_empty() { "-" } @else { (value) } }
                }
            }
        }
        p { a class="btn secondary" href="/app" { (me.t("my_info.back")) } }
    };
    me.page(&s, &title, body)
}

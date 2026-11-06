//! `GET /admin`: the back-office home, listing the pages the Staff member
//! may open.

use axum::extract::State;
use axum::response::Html;
use maud::html;

use super::CurrentStaff;
use crate::AppState;

pub(crate) async fn home(State(s): State<AppState>, staff: CurrentStaff) -> Html<String> {
    let t = |k: &str| s.catalog.t(&staff.locale, k);
    let title = t("kernel.backoffice.home");
    let body = html! {
        h1 { (title) }
        p class="muted" { (t("kernel.backoffice.welcome")) " " (staff.display_name) }
        @if staff.nav().is_empty() {
            div class="card" { p { (t("kernel.backoffice.no_pages")) } }
        } @else {
            ul class="tiles" {
                @for (label, href) in staff.nav() {
                    li { a class="tile" href=(href) { (label) } }
                }
            }
        }
    };
    Html(crate::ui::back_office(&s, &staff, &title, body).into_string())
}

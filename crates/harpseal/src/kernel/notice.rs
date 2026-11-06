//! `GET /notice`: the public privacy notice, rendered from the catalog
//! (`kernel.notice.title`, `kernel.notice.body`) with `setting` rows on
//! top, so each Deployment words it in its TOML or from the back office
//! and an edit shows on the next request.

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{Html, IntoResponse, Response};
use maud::html;

use super::text::Texts;
use crate::AppState;
use crate::ui::{Shell, page as layout};

pub(crate) const PATH: &str = "/notice";

pub(crate) async fn page(State(s): State<AppState>) -> Response {
    let locale = s.config.default_locale.clone();
    let texts = match Texts::load(&s.services, s.deployment.catalog_arc(), locale).await {
        Ok(t) => t,
        Err(_) => return StatusCode::SERVICE_UNAVAILABLE.into_response(),
    };
    let title = texts.t("kernel.notice.title");
    let body = texts.t("kernel.notice.body");
    let content = html! {
        article class="card notice" {
            h1 { (title) }
            @for para in body.split("\n\n").map(str::trim).filter(|p| !p.is_empty()) {
                p { (para) }
            }
        }
    };
    Html(layout(&s.config.name, &title, Shell::MiniApp, content).into_string()).into_response()
}

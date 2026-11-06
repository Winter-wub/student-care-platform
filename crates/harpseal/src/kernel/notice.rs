//! `GET /notice`: the public privacy notice, rendered from the catalog
//! (`kernel.notice.title`, `kernel.notice.body`)
//! so each Deployment words it in its TOML or from the back office.

use axum::extract::State;
use axum::response::Html;
use maud::html;

use crate::AppState;
use crate::ui::{Shell, page as layout};

pub(crate) const PATH: &str = "/notice";

pub(crate) async fn page(State(s): State<AppState>) -> Html<String> {
    let title = s.t("kernel.notice.title");
    let body = s.t("kernel.notice.body");
    let content = html! {
        article class="card notice" {
            h1 { (title) }
            @for para in body.split("\n\n").map(str::trim).filter(|p| !p.is_empty()) {
                p { (para) }
            }
        }
    };
    Html(layout(&s.config.name, &title, Shell::MiniApp, content).into_string())
}

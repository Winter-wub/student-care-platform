//! `GET /notice`: the public privacy notice, rendered from the catalog
//! (`kernel.notice.title`, `kernel.notice.body`) with `setting` rows on
//! top, so each Deployment words it in its TOML or from the back office
//! and an edit shows on the next request. It shows the current notice
//! version (`Texts::consent_version`). No session is needed.
//!
//! `?app=1` (the Mini App's links) renders it in the App Session shell,
//! with a way back to `/app`; otherwise it is the plain Mini App layout
//! (the chat's "อ่านฉบับเต็ม" button and a browser). The App Session
//! cookie is scoped to `/app`, so the query is the only signal.

use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::{Html, IntoResponse, Response};
use maud::html;
use serde::Deserialize;

use super::text::Texts;
use crate::AppState;

pub(crate) const PATH: &str = "/notice";

/// `?app=1`.
#[derive(Debug, Default, Deserialize)]
pub(crate) struct NoticeQuery {
    #[serde(default)]
    app: Option<String>,
}

pub(crate) async fn page(State(s): State<AppState>, Query(q): Query<NoticeQuery>) -> Response {
    let in_app = q.app.as_deref() == Some("1");
    let locale = s.config.default_locale.clone();
    let texts = match Texts::load(&s.services, s.deployment.catalog_arc(), locale).await {
        Ok(t) => t,
        Err(_) => return StatusCode::SERVICE_UNAVAILABLE.into_response(),
    };
    let title = texts.t("kernel.notice.title");
    let body = texts.t("kernel.notice.body");
    let version = texts.consent_version(&s.config.consent.version);
    let content = html! {
        article class="card notice" {
            h1 { (title) }
            p class="muted" { (texts.tf("kernel.app.notice_version", &[("version", &version)])) }
            @for para in body.split("\n\n").map(str::trim).filter(|p| !p.is_empty()) {
                p { (para) }
            }
        }
        @if in_app {
            p { a class="btn secondary" href="/app" { (texts.t("kernel.app.back")) } }
        }
    };
    let page = if in_app {
        let label = texts.t("kernel.app.notice_link");
        crate::ui::mini_app_session(&s, &title, None, &label, content)
    } else {
        crate::ui::mini_app(&s, &title, content)
    };
    Html(page.into_string()).into_response()
}

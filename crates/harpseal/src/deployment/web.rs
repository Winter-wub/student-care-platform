//! The Deployment's axum Router: webhooks, the shared stylesheet, public
//! and Mini App pages at the root, and back-office pages under `/admin`,
//! each behind a Permission guard.

use std::sync::Arc;

use axum::body::Bytes;
use axum::extract::{Path, Request, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::middleware::Next;
use axum::response::{Html, IntoResponse, Response};
use axum::routing::{get, post};

use super::Deployment;
use crate::delivery::WebhookRequest;
use crate::{AppState, Error, PageState};

impl Deployment {
    /// The whole HTTP surface:
    /// - `POST /webhook/{kind}`: Channel webhooks (401 on a bad signature);
    /// - `GET /static/app.css`: the shared stylesheet;
    /// - Module public routes at the root (the Kernel's `/notice`);
    /// - Module back-office routes under `/admin`, each guarded by its
    ///   Permission.
    pub fn router(self: Arc<Self>) -> axum::Router {
        let state: AppState = Arc::new(PageState {
            services: self.services.clone(),
            catalog: self.catalog.clone(),
            config: self.config.clone(),
        });
        let mut admin = axum::Router::new();
        for (_module, permission, r) in &self.registry.back_office {
            let guarded = r.clone().layer(axum::middleware::from_fn_with_state(
                (state.clone(), *permission),
                guard,
            ));
            admin = admin.merge(guarded);
        }
        let mut root = axum::Router::new();
        for r in &self.registry.public {
            root = root.merge(r.clone());
        }
        let css = get(|| async {
            (
                [
                    (header::CONTENT_TYPE, "text/css; charset=utf-8"),
                    (header::CACHE_CONTROL, "public, max-age=86400"),
                ],
                crate::ui::CSS,
            )
        });
        root.route("/webhook/{kind}", post(webhook))
            .route(crate::ui::CSS_PATH, css)
            .nest("/admin", admin)
            .with_state(state)
            .layer(axum::Extension(self))
    }
}

async fn webhook(
    axum::Extension(d): axum::Extension<Arc<Deployment>>,
    Path(kind): Path<String>,
    headers: HeaderMap,
    body: Bytes,
) -> StatusCode {
    if d.channel(&kind).is_none() {
        return StatusCode::NOT_FOUND;
    }
    let req = WebhookRequest {
        headers,
        body: body.to_vec(),
    };
    match d.handle_webhook(&kind, req).await {
        Ok(()) => StatusCode::OK,
        Err(Error::BadSignature) => StatusCode::UNAUTHORIZED,
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR,
    }
}

/// Lets a back-office request through only for Staff holding `permission`.
/// Staff sign-in and sessions arrive with #21; until then no request holds
/// a session, so every guarded route answers 401 with a sign-in notice.
async fn guard(
    State((state, permission)): State<(AppState, &'static str)>,
    req: Request,
    next: Next,
) -> Response {
    if staff_holds(&state, &req, permission) {
        return next.run(req).await;
    }
    let text = state.t("kernel.backoffice.sign_in");
    let body = crate::ui::page(
        &state.config.name,
        &text,
        crate::ui::Shell::BackOffice { staff_email: None },
        maud::html! { div class="card" { p { (text) } } },
    );
    (StatusCode::UNAUTHORIZED, Html(body.into_string())).into_response()
}

/// Whether the request carries a Staff session granting `permission`.
fn staff_holds(_state: &PageState, _req: &Request, _permission: &str) -> bool {
    false
}

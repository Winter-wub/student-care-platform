//! The Deployment's axum Router: webhooks, the shared stylesheet, public
//! and Mini App pages at the root, and back-office pages under `/admin`,
//! each behind the Staff guard.

use std::sync::Arc;

use axum::body::Bytes;
use axum::extract::Path;
use axum::http::{HeaderMap, StatusCode, header};
use axum::routing::{MethodRouter, get, post};

use super::Deployment;
use super::routes::WEBHOOK_PATH;
use crate::auth::{self, AdminCx, Gate};
use crate::delivery::WebhookRequest;
use crate::{AppState, Error, PageState};

impl Deployment {
    /// The whole HTTP surface:
    /// - `POST /webhook/{kind}`: Channel webhooks: 200 once the call is
    ///   accepted (a failing event is logged, never turned into an error
    ///   status, or the platform would resend the whole call), 401 on a bad
    ///   signature, 400 when the body cannot be parsed, 404 for an unknown
    ///   Channel;
    /// - `GET /static/app.css`: the shared stylesheet;
    /// - Module public routes at the root (the Kernel's `/notice`);
    /// - `/admin/login`, `/admin/logout`, `/admin/password` and the `/admin`
    ///   home (Staff sign-in, see [`crate::CurrentStaff`]);
    /// - Module back-office routes under `/admin`, each guarded by its
    ///   Permission: no session → 303 to `/admin/login`; a pending password
    ///   change → 303 to `/admin/password`; Permission missing → a 403
    ///   page; a POST without the CSRF token → 403.
    ///
    /// Every response carries `x-request-id` (the [`crate::RequestCtx`] id).
    /// Never panics: `build()` already refused overlapping or invalid paths.
    pub fn router(self: Arc<Self>) -> axum::Router {
        let state: AppState = Arc::new(PageState {
            services: self.services.clone(),
            catalog: self.catalog.clone(),
            config: self.config.clone(),
        });
        let cx = Arc::new(AdminCx {
            state: state.clone(),
            nav: self.registry.nav.clone(),
        });
        let guarded = |route: MethodRouter<AppState>, gate: Gate| {
            route.layer(axum::middleware::from_fn_with_state(
                (cx.clone(), gate),
                auth::guard,
            ))
        };
        let signed_in = Gate {
            permission: None,
            during_forced_change: false,
        };
        let any_time = Gate {
            permission: None,
            during_forced_change: true,
        };
        let mut root = axum::Router::new()
            .route(
                auth::LOGIN_PATH,
                get(auth::login_form).post(auth::login_submit),
            )
            .route(auth::HOME_PATH, guarded(get(auth::home), signed_in))
            .route(auth::LOGOUT_PATH, guarded(post(auth::logout), any_time))
            .route(
                auth::PASSWORD_PATH,
                guarded(
                    get(auth::password_form).post(auth::password_submit),
                    any_time,
                ),
            );
        for (_module, path, route) in &self.registry.public {
            root = root.route(path, route.clone());
        }
        for (_module, permission, path, route) in &self.registry.back_office {
            let gate = Gate {
                permission: Some(*permission),
                during_forced_change: false,
            };
            root = root.route(
                &format!("{}{path}", super::routes::ADMIN),
                guarded(route.clone(), gate),
            );
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
        root.route(WEBHOOK_PATH, post(webhook))
            .route(crate::ui::CSS_PATH, css)
            .layer(axum::middleware::from_fn_with_state(
                state.clone(),
                auth::request_ctx,
            ))
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
        Ok(_) => StatusCode::OK,
        Err(Error::BadSignature) => StatusCode::UNAUTHORIZED,
        Err(_) => StatusCode::BAD_REQUEST,
    }
}

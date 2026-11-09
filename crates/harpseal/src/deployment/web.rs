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
    ///   Channel or one that takes no traffic (a [`crate::Channel::dev_only`]
    ///   Channel outside dev, #133), before anything is read or recorded.
    ///   When the request carries a [`crate::Background`] extension (both
    ///   runtimes add one), the 200 comes right after the events are
    ///   claimed and the handling runs on it, independent of the
    ///   connection ([`Deployment::accept_webhook`]); without one the
    ///   events are handled before the answer;
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
            jobs: self.queue.clone(),
            deployment: self.clone(),
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
            body_limit: auth::MULTIPART_LIMIT,
            too_large: None,
            module: None,
        };
        let any_time = Gate {
            permission: None,
            during_forced_change: true,
            body_limit: auth::MULTIPART_LIMIT,
            too_large: None,
            module: None,
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
        if !self.registry.settings.is_empty() {
            let gate = Gate {
                module: Some(crate::kernel::NAME),
                ..signed_in
            };
            root = root.route(
                super::routes::SETTINGS_PATH,
                guarded(crate::kernel::admin::settings_route(), gate),
            );
        }
        for (_module, path, route) in &self.registry.public {
            root = root.route(path, route.clone());
        }
        for (module, permission, path, route) in &self.registry.back_office {
            let upload = self.registry.uploads.iter().find(|u| u.path == *path);
            let gate = Gate {
                permission: Some(*permission),
                during_forced_change: false,
                body_limit: upload.map_or(auth::MULTIPART_LIMIT, |u| u.max_body),
                too_large: upload.and_then(|u| u.too_large),
                module: Some(*module),
            };
            root = root.route(
                &format!("{}{path}", super::routes::ADMIN),
                guarded(route.clone(), gate),
            );
            // Its template (#127), for the same Permission.
            if let Some(columns) = upload.and_then(|u| u.columns) {
                let template = crate::module::template_route(path, columns);
                let gate = Gate {
                    body_limit: auth::MULTIPART_LIMIT,
                    too_large: None,
                    ..gate
                };
                let at = crate::module::template_path(path);
                root = root.route(&at, guarded(template, gate));
            }
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
                crate::kernel::branding::refresh,
            ))
            .layer(axum::middleware::from_fn_with_state(
                state.clone(),
                auth::request_ctx,
            ))
            .with_state(state)
            .layer(axum::Extension(self))
    }
}

impl Deployment {
    /// The Channel of `kind` as registered, whether or not it takes
    /// traffic: only to tell a refused dev-only Channel from an unknown one.
    pub(crate) fn registered_channel(&self, kind: &str) -> Option<&dyn crate::Channel> {
        self.channels
            .iter()
            .find(|c| c.kind().as_str() == kind)
            .map(|c| c.as_ref())
    }

    /// Whether Channel `kind` is registered but refused: dev-only, outside
    /// dev (#133).
    pub(crate) fn refuses_channel(&self, kind: &str) -> bool {
        self.registered_channel(kind).is_some() && self.channel(kind).is_none()
    }

    /// Whether `c` takes traffic: dev mode, or not
    /// [`crate::Channel::dev_only`] (#133).
    pub(super) fn takes(&self, c: &dyn crate::Channel) -> bool {
        self.dev || !c.dev_only()
    }

    /// The kinds of the Deployment's Channels, in registration order.
    pub(crate) fn channel_kinds(&self) -> Vec<crate::ChannelKind> {
        self.channels.iter().map(|c| c.kind()).collect()
    }

    /// The kinds the back office offers Staff by default (#131): every
    /// Channel that takes traffic ([`Deployment::channel`]). A Link that
    /// exists on a refused Channel is still shown (and can be revoked)
    /// wherever Links are listed.
    pub(crate) fn staff_channel_kinds(&self) -> Vec<crate::ChannelKind> {
        self.channels
            .iter()
            .filter(|c| self.takes(c.as_ref()))
            .map(|c| c.kind())
            .collect()
    }

    /// The Channel kinds a Student's Links card or list row shows (#131,
    /// #133): every kind offered to Staff, plus the Channel of each of the
    /// Student's live Links (`live`) even when it is not offered (dev-only
    /// outside dev) or not registered here at all (the example Worker has
    /// no Fake Channel), so an existing Link always shows and can be
    /// revoked. Registration order, then unregistered kinds as they come.
    pub(crate) fn link_kinds<'a>(&self, live: impl IntoIterator<Item = &'a str>) -> Vec<String> {
        let live: Vec<&str> = live.into_iter().collect();
        let offered = self.staff_channel_kinds();
        let mut out: Vec<String> = self
            .channels
            .iter()
            .map(|c| c.kind())
            .filter(|k| offered.contains(k) || live.contains(&k.as_str()))
            .map(|k| k.as_str().to_string())
            .collect();
        for k in live {
            if !out.iter().any(|o| o == k) {
                out.push(k.to_string());
            }
        }
        out
    }

    /// Whether Channel `kind` is one of [`Self::staff_channel_kinds`].
    pub(crate) fn staff_sees_channel(&self, kind: &str) -> bool {
        self.staff_channel_kinds()
            .iter()
            .any(|k| k.as_str() == kind)
    }
}

async fn webhook(
    axum::Extension(d): axum::Extension<Arc<Deployment>>,
    bg: Option<axum::Extension<crate::Background>>,
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
    let accepted = match bg {
        Some(axum::Extension(bg)) => d.accept_webhook(&kind, req, &bg).await,
        None => d.handle_webhook(&kind, req).await,
    };
    match accepted {
        Ok(_) => StatusCode::OK,
        Err(Error::BadSignature) => StatusCode::UNAUTHORIZED,
        Err(_) => StatusCode::BAD_REQUEST,
    }
}

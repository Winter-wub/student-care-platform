//! [`AppStudent`] and [`AppForm`]: the Student behind the App Session, for
//! a Module's own Mini App page (#72), with CSRF checked by construction.

use axum::Form;
use axum::body::Body;
use axum::extract::{FromRequest, FromRequestParts, Request};
use axum::http::request::Parts;
use axum::http::{HeaderMap, Method, StatusCode};
use axum::response::{IntoResponse, Response};
use maud::Markup;
use serde::de::DeserializeOwned;
use serde_json::{Map, Value as Json};

use super::view;
use crate::auth::{CSRF_FIELD, CSRF_HEADER, ct_eq, form_field, over_limit};
use crate::kernel::students::is_htmx;
use crate::kernel::text::Texts;
use crate::{AppState, ErrorReport, Locale, PageState, StudentId};

/// Largest form body [`AppForm`] reads.
const FORM_LIMIT: usize = 64 * 1024;

/// The linked Student who opened a Mini App page: an axum extractor for
/// a Module's handler registered with [`crate::ModuleRegistrar::public`]
/// under `/app/`.
///
/// It reads the App Session cookie exactly as the Kernel's pages do (a
/// LIFF or Telegram Web App session, the partitioned cookie of Telegram
/// Web included) and the Channel Account's active Link on every request.
/// A request that does not come from one of the Deployment's own pages
/// (a new launch from the chat, a deep link, a bookmark) never reaches
/// the handler with the old session: it is dropped and the entry page
/// asks the platform again who opened the Mini App (#86).
/// Nothing in the path, query or body picks the Student: a handler that
/// only reads data by [`AppStudent::id`] can never show another Student's.
///
/// The rejection is the Kernel's own answer, never a 500 page:
/// - no session, an unknown, forged or expired one, or a new launch: the
///   entry page,
///   which signs in through the platform and comes back to this path
///   (the cookie is cleared when one was sent);
/// - a session whose account has no active Link (revoked, `left`): the
///   "go back to the chat and link" page;
/// - the database unreachable: a bare 503;
/// - an HTMX request in any of these cases: `HX-Redirect`, so the whole
///   page reloads into the answer instead of a fragment: to this path for
///   a `GET`, else to the page the request came from (`HX-Current-URL`,
///   kept on `/app`), since HTMX follows the redirect with a `GET`.
///
/// **Requests that may change data** (any method but `GET`, `HEAD`,
/// `OPTIONS`) are refused with 403 when `Sec-Fetch-Site` says
/// `cross-site`, and unless they carry this session's CSRF token in the
/// `X-CSRF-Token` header (HTMX sends it from [`AppStudent::page`]). A
/// plain HTML form cannot set a header: take [`AppForm`] instead, which
/// reads the `_csrf` field. A handler cannot skip the check. `OPTIONS`
/// counts as safe: never change data on it (e.g. through `any(..)`).
/// A `multipart/form-data` form has no `AppForm`: send it with HTMX
/// (`hx-encoding="multipart/form-data"`) or `fetch`, which carry the
/// header; a plain multipart `<form>` always gets 403.
///
/// A route outside `/app` never sees the cookie (`Path=/app`), so its
/// handler always answers the entry page, which then signs in to `/app`.
#[derive(Clone)]
#[non_exhaustive]
pub struct AppStudent {
    /// `student.id`.
    pub id: StudentId,
    /// The student id (`student.student_code`).
    pub student_code: String,
    /// `student.display_name`.
    pub display_name: String,
    /// The Locale the page is in: the Student's when the catalog has it,
    /// else the account's platform Locale, else the default.
    pub locale: Locale,
    /// The attributes the Deployment lets Students see: only keys declared
    /// in `[roster.attributes]` with `student_visible = true` (default
    /// `false`). A read-only copy: changing it changes nothing stored.
    pub attributes: Map<String, Json>,
    /// The Channel kind of the account that opened the page.
    pub channel: String,
    csrf: String,
    texts: Texts,
}

impl core::fmt::Debug for AppStudent {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("AppStudent")
            .field("id", &self.id)
            .field("locale", &self.locale)
            .field("channel", &self.channel)
            .finish_non_exhaustive()
    }
}

impl AppStudent {
    /// A text in [`AppStudent::locale`], with the back office's Texts
    /// overrides, as the Kernel's Mini App pages read them.
    pub fn t(&self, key: &str) -> String {
        self.texts.t(key)
    }

    /// [`AppStudent::t`] with `{name}` placeholders replaced.
    pub fn tf(&self, key: &str, args: &[(&str, &str)]) -> String {
        self.texts.tf(key, args)
    }

    /// The App Session's CSRF token, for the `_csrf` field of a plain form
    /// that [`AppForm`] reads. The page from [`AppStudent::page`] already
    /// sends it on HTMX requests (`X-CSRF-Token`).
    pub fn csrf_token(&self) -> &str {
        &self.csrf
    }

    /// A whole Mini App page (`200`) in the Deployment's branding, as the
    /// Kernel's pages: CSRF header for HTMX, privacy-notice footer and
    /// `Referrer-Policy: same-origin` (#86). Every `/app` response also gets
    /// `Cache-Control: no-store` and the `frame-ancestors` CSP.
    pub fn page(&self, s: &PageState, title: &str, body: Markup) -> Response {
        let notice = self.texts.t("kernel.app.notice_link");
        let doc = crate::ui::mini_app_session(s, title, Some(&self.csrf), &notice, body);
        view::respond(StatusCode::OK, doc)
    }

    fn csrf_ok(&self, sent: &str) -> bool {
        ct_eq(sent.as_bytes(), self.csrf.as_bytes())
    }

    fn forbidden(&self, s: &PageState) -> Response {
        view::answer(
            s,
            &self.texts,
            StatusCode::FORBIDDEN,
            "kernel.app.forbidden",
            None,
        )
    }

    /// The Student, after the cross-site check; no CSRF check yet.
    async fn load(parts: &Parts, s: &AppState) -> Result<Self, Box<Response>> {
        let path = super::safe_next(parts.uri.path()).to_string();
        let headers = &parts.headers;
        if !is_safe(&parts.method) && cross_site(headers) {
            return Err(Box::new(
                match super::texts(s, &s.config.default_locale).await {
                    Ok(t) => {
                        view::answer(s, &t, StatusCode::FORBIDDEN, "kernel.app.forbidden", None)
                    }
                    Err(e) => ErrorReport::attach(
                        StatusCode::SERVICE_UNAVAILABLE.into_response(),
                        "app.page",
                        &e,
                    ),
                },
            ));
        }
        let htmx = is_htmx(headers);
        // Where HTMX reloads the whole page: this path for a GET, the page
        // the request came from otherwise (a POST-only path would be 405).
        let reload = if matches!(parts.method, Method::GET | Method::HEAD) {
            path.clone()
        } else {
            super::safe_next(current_path(headers)).to_string()
        };
        let (v, texts) = match super::pages::open(s, headers, &path).await {
            Ok(x) => x,
            Err(r) if htmx => return Err(Box::new(super::pages::hx_redirect_from(&reload, &r))),
            Err(r) => return Err(r),
        };
        let Some(st) = v.student else {
            if htmx {
                return Err(Box::new(super::pages::hx_redirect(&reload)));
            }
            return Err(Box::new(view::not_linked(s, &texts, Some(&v.csrf))));
        };
        let visible = &s.config.roster.attributes;
        let attributes = st
            .attributes
            .into_iter()
            .filter(|(k, _)| visible.get(k).is_some_and(|a| a.student_visible))
            .collect();
        Ok(AppStudent {
            id: StudentId(st.id),
            student_code: st.code,
            display_name: st.name,
            locale: v.locale,
            attributes,
            channel: v.channel,
            csrf: v.csrf,
            texts,
        })
    }
}

/// Methods that change nothing.
fn is_safe(m: &Method) -> bool {
    matches!(*m, Method::GET | Method::HEAD | Method::OPTIONS)
}

/// The browser says the request comes from another site. In Telegram
/// Web's iframe another bot's Mini App shares our partitioned cookie.
fn cross_site(headers: &HeaderMap) -> bool {
    headers
        .get("sec-fetch-site")
        .is_some_and(|v| v == "cross-site")
}

/// The path of HTMX's `HX-Current-URL` (the page the request came from),
/// `""` without one; [`super::safe_next`] then keeps it on `/app`.
fn current_path(headers: &HeaderMap) -> &str {
    let url = headers
        .get("hx-current-url")
        .and_then(|h| h.to_str().ok())
        .unwrap_or_default();
    let rest = url.split_once("://").map_or(url, |(_, r)| r);
    let path = rest.find('/').map_or("", |i| &rest[i..]);
    path.split(['?', '#']).next().unwrap_or_default()
}

fn header_token(headers: &HeaderMap) -> &str {
    headers
        .get(CSRF_HEADER)
        .and_then(|h| h.to_str().ok())
        .unwrap_or_default()
}

impl FromRequestParts<AppState> for AppStudent {
    type Rejection = Response;

    async fn from_request_parts(parts: &mut Parts, s: &AppState) -> Result<Self, Response> {
        let me = Self::load(parts, s).await.map_err(|r| *r)?;
        if !is_safe(&parts.method) && !me.csrf_ok(header_token(&parts.headers)) {
            return Err(me.forbidden(s));
        }
        Ok(me)
    }
}

/// A form posted from a Module's Mini App page, and the [`AppStudent`]
/// who posted it: the extractor for a plain `<form method="post">`
/// (`application/x-www-form-urlencoded` only, at most 64 KiB: a larger
/// body is 413, a broken one 400) that carries
/// `<input type="hidden" name="_csrf" value=(me.csrf_token())>`.
///
/// It answers like [`AppStudent`] (cross-site 403, entry page, …), then
/// 403 unless the `_csrf` field or the `X-CSRF-Token` header is this
/// session's token, then decodes the body as `T` (`_csrf` is one more
/// field: ignore it, or give `T` a `_csrf` field); a body that does not
/// decode gets axum's `Form` rejection (4xx).
#[derive(Debug)]
#[non_exhaustive]
pub struct AppForm<T> {
    /// Who posted it.
    pub student: AppStudent,
    /// The decoded form.
    pub form: T,
}

impl<T> FromRequest<AppState> for AppForm<T>
where
    T: DeserializeOwned + Send,
{
    type Rejection = Response;

    async fn from_request(req: Request, s: &AppState) -> Result<Self, Response> {
        let (parts, body) = req.into_parts();
        let student = AppStudent::load(&parts, s).await.map_err(|r| *r)?;
        let bytes = match axum::body::to_bytes(body, FORM_LIMIT).await {
            Ok(b) => b,
            Err(e) if over_limit(&e) => {
                return Err(StatusCode::PAYLOAD_TOO_LARGE.into_response());
            }
            // A client that went away, a broken body: not "too large".
            Err(_) => return Err(StatusCode::BAD_REQUEST.into_response()),
        };
        let text = std::str::from_utf8(&bytes).unwrap_or_default();
        let sent = match header_token(&parts.headers) {
            "" => form_field(text, CSRF_FIELD).unwrap_or_default(),
            h => h,
        };
        if !student.csrf_ok(sent) {
            return Err(student.forbidden(s));
        }
        let req = Request::from_parts(parts, Body::from(bytes));
        let Form(form) = Form::<T>::from_request(req, s)
            .await
            .map_err(IntoResponse::into_response)?;
        Ok(AppForm { student, form })
    }
}

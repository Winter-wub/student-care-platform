//! [`AppStudent`]: the Student behind the App Session, for a Module's own
//! Mini App page (#72).

use axum::extract::FromRequestParts;
use axum::http::StatusCode;
use axum::http::request::Parts;
use axum::response::Response;
use maud::Markup;
use serde_json::{Map, Value as Json};

use super::view;
use crate::auth::ct_eq;
use crate::kernel::text::Texts;
use crate::{AppState, Locale, PageState, StudentId};

/// The linked Student who opened a Mini App page: an axum extractor for
/// a Module's handler registered with [`crate::ModuleRegistrar::public`]
/// under `/app/`.
///
/// It reads the App Session cookie exactly as the Kernel's pages do (a
/// LIFF or Telegram Web App session, the partitioned cookie of Telegram
/// Web included) and the Channel Account's active Link on every request.
/// Nothing in the path, query or body picks the Student: a handler that
/// only reads data by [`AppStudent::id`] can never show another Student's.
///
/// The rejection is the Kernel's own answer, never a 500 page:
/// - no session, or an unknown, forged or expired one: the entry page,
///   which signs in through the platform and comes back to this path
///   (the cookie is cleared when one was sent);
/// - a session whose account has no active Link (revoked, `left`): the
///   "go back to the chat and link" page;
/// - the database unreachable: a bare 503.
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
    /// `student.attributes` (per `[roster.attributes]`), a read-only
    /// copy: changing it changes nothing stored.
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

    /// The App Session's CSRF token. The page from [`AppStudent::page`]
    /// already sends it on HTMX requests (`X-CSRF-Token`); a plain form
    /// posts it as `_csrf`.
    pub fn csrf_token(&self) -> &str {
        &self.csrf
    }

    /// Whether `sent` is this session's CSRF token (constant time). Check
    /// it on every `POST` before changing anything.
    pub fn csrf_ok(&self, sent: &str) -> bool {
        ct_eq(sent.as_bytes(), self.csrf.as_bytes())
    }

    /// A whole Mini App page (`200`) in the Deployment's branding, as the
    /// Kernel's pages: CSRF header for HTMX, privacy-notice footer,
    /// `Cache-Control: no-store` and `Referrer-Policy: no-referrer`. The
    /// `frame-ancestors` CSP is added to every `/app` response.
    pub fn page(&self, s: &PageState, title: &str, body: Markup) -> Response {
        let notice = self.texts.t("kernel.app.notice_link");
        let doc = crate::ui::mini_app_session(s, title, Some(&self.csrf), &notice, body);
        view::respond(StatusCode::OK, doc)
    }
}

impl FromRequestParts<AppState> for AppStudent {
    type Rejection = Response;

    async fn from_request_parts(parts: &mut Parts, s: &AppState) -> Result<Self, Response> {
        let path = parts.uri.path().to_string();
        let (v, texts) = super::pages::open(s, &parts.headers, &path)
            .await
            .map_err(|r| *r)?;
        let Some(st) = v.student else {
            return Err(view::not_linked(s, &texts, Some(&v.csrf)));
        };
        Ok(AppStudent {
            id: StudentId(st.id),
            student_code: st.code,
            display_name: st.name,
            locale: v.locale,
            attributes: st.attributes,
            channel: v.channel,
            csrf: v.csrf,
            texts,
        })
    }
}

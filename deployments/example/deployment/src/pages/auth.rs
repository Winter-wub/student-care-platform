//! Staff login, logout and the session guard (`Staff` extractor).
//! Session cookie: random 32-byte token; the table stores only its SHA-256.

use axum::extract::{FromRequestParts, State};
use axum::http::request::Parts;
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::{Html, IntoResponse, Redirect, Response};
use axum::{Extension, Form};
use harpseal::AppState;
use harpseal::port_types::Statement;
use harpseal::ui::{Flash, Shell, flash, page};
use maud::{Markup, html};
use serde::Deserialize;

use super::{Page, cookie, is_htmx, random_hex, sha256_hex, text};

pub const SESSION_COOKIE: &str = "hs_session";
const SESSION_MS: i64 = 12 * 60 * 60 * 1000;

/// A signed-in Staff member. Handlers that take it are guarded: without a
/// valid session the request is sent to `/admin/login`.
pub struct Staff {
    pub id: String,
    pub email: String,
}

impl FromRequestParts<AppState> for Staff {
    type Rejection = Response;

    async fn from_request_parts(parts: &mut Parts, s: &AppState) -> Result<Self, Response> {
        let to_login = || {
            if is_htmx(&parts.headers) {
                ([("hx-redirect", "/admin/login")], StatusCode::UNAUTHORIZED).into_response()
            } else {
                Redirect::to("/admin/login").into_response()
            }
        };
        let Some(token) = cookie(&parts.headers, SESSION_COOKIE) else {
            return Err(to_login());
        };
        let q = Statement::new(
            "SELECT st.id, st.email FROM staff_session ss JOIN staff st ON st.id = ss.staff_id \
             WHERE ss.id = ? AND ss.expires_at > ? AND st.status = 'active'",
        )
        .bind(sha256_hex(token))
        .bind(s.services.clock.now().0);
        match s.services.db.query(q).await {
            Ok(rows) => match rows.rows.first() {
                Some(r) => Ok(Staff {
                    id: text(r, 0),
                    email: text(r, 1),
                }),
                None => Err(to_login()),
            },
            Err(e) => Err((StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response()),
        }
    }
}

fn login_page(org: &str, email: &str, error: Option<&str>) -> Markup {
    let body = html! {
        div class="card" style="max-width:420px;margin:24px auto" {
            h1 { "เข้าสู่ระบบเจ้าหน้าที่" }
            @if let Some(e) = error { (flash(&Flash { ok: false, text: e })) }
            form method="post" action="/admin/login" {
                div class="field" {
                    label for="email" { "อีเมล" }
                    input id="email" name="email" type="email" autocomplete="username"
                        required value=(email);
                }
                div class="field" {
                    label for="password" { "รหัสผ่าน" }
                    input id="password" name="password" type="password"
                        autocomplete="current-password" required minlength="12";
                }
                button class="wide" type="submit" { "เข้าสู่ระบบ" }
            }
        }
    };
    page(
        org,
        "เข้าสู่ระบบ",
        Shell::BackOffice { staff_email: None },
        body,
    )
}

pub async fn login_form(State(s): State<AppState>) -> Response {
    Html(login_page(&s.org, "", None).into_string()).into_response()
}

#[derive(Deserialize)]
pub struct LoginForm {
    email: String,
    password: String,
}

fn session_cookie(value: &str, max_age: i64, secure: bool) -> HeaderValue {
    let secure = if secure { "; Secure" } else { "" };
    let c = format!(
        "{SESSION_COOKIE}={value}; Path=/admin; HttpOnly; SameSite=Lax; Max-Age={max_age}{secure}"
    );
    HeaderValue::from_str(&c).unwrap_or(HeaderValue::from_static(""))
}

/// Whether cookies get `Secure` (off only for plain-HTTP localhost).
#[derive(Clone, Copy)]
pub struct SecureCookies(pub bool);

/// `POST /admin/login`. Prototype gaps: no lockout after 5 failures, no
/// dummy hash for unknown emails (timing tells whether an email exists),
/// no CSRF token (SameSite=Lax only).
pub async fn login(
    State(s): State<AppState>,
    Extension(SecureCookies(secure)): Extension<SecureCookies>,
    Form(f): Form<LoginForm>,
) -> Page {
    let email = f.email.trim().to_lowercase();
    let q =
        Statement::new("SELECT id, password_hash FROM staff WHERE email = ? AND status = 'active'")
            .bind(email.clone());
    let rows = s.services.db.query(q).await?;
    let ok_staff = match rows.rows.first() {
        Some(r) if harpseal::password::verify(&text(r, 1), &f.password)? => Some(text(r, 0)),
        _ => None,
    };
    let Some(staff_id) = ok_staff else {
        let html = login_page(&s.org, &email, Some("อีเมลหรือรหัสผ่านไม่ถูกต้อง"));
        return Ok((StatusCode::UNAUTHORIZED, Html(html.into_string())).into_response());
    };
    let token = random_hex(&s.services, 32)?;
    let now = s.services.clock.now().0;
    let insert = Statement::new(
        "INSERT INTO staff_session (id, staff_id, created_at, expires_at, last_seen_at) \
         VALUES (?, ?, ?, ?, ?)",
    )
    .bind(sha256_hex(&token))
    .bind(staff_id)
    .bind(now)
    .bind(now + SESSION_MS)
    .bind(now);
    s.services.db.execute(insert).await?;
    let mut resp = Redirect::to("/admin/students").into_response();
    let c = session_cookie(&token, SESSION_MS / 1000, secure);
    resp.headers_mut().insert(header::SET_COOKIE, c);
    Ok(resp)
}

/// `POST /admin/logout`: deletes the session row and the cookie.
pub async fn logout(
    State(s): State<AppState>,
    Extension(SecureCookies(secure)): Extension<SecureCookies>,
    headers: HeaderMap,
) -> Page {
    if let Some(token) = cookie(&headers, SESSION_COOKIE) {
        let del = Statement::new("DELETE FROM staff_session WHERE id = ?").bind(sha256_hex(token));
        s.services.db.execute(del).await?;
    }
    let mut resp = Redirect::to("/admin/login").into_response();
    resp.headers_mut()
        .insert(header::SET_COOKIE, session_cookie("", 0, secure));
    Ok(resp)
}

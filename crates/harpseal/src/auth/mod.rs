//! Staff sign-in, sessions, Roles and Permissions, and the back-office
//! guard (#21, spec "Staff auth").
//!
//! - Passwords: at least 12 characters, Argon2id m=19456 t=2 p=1 as a PHC
//!   string ([`crate::password`]).
//! - Sessions: a 32-byte random token in the `hs_session` cookie
//!   (`HttpOnly; Secure; SameSite=Lax; Path=/admin`); `staff_session.id` is
//!   its SHA-256, never the token. 12 hours sliding, 7 days absolute, both
//!   checked against the Clock on every request.
//! - Lockout: 5 wrong passwords in a row → `staff.locked_until` = now + 15
//!   minutes, the counter (`staff.failed_logins`) resets, and a Security
//!   Event `staff.lockout` is written. Wrong current passwords on the change
//!   form count too. A password change rotates the session.
//! - CSRF: every back-office POST carries a per-session token (SHA-256 of
//!   `"csrf:" + session token`) in a hidden `_csrf` field or an
//!   `X-CSRF-Token` header (HTMX sends it from `hx-headers`); the login form,
//!   which has no session yet, uses a double-submit cookie `hs_csrf`.

mod attempt;
mod cookies;
mod csrf_body;
mod guard;
mod login;
mod pages;
pub(crate) mod role_admin;
mod role_seed;
pub(crate) mod roles;
mod session;
mod staff;
pub(crate) mod staff_admin;
mod texts;

pub(crate) use cookies::flash as flash_cookie;
pub(crate) use cookies::{ct_eq, form_field, get as cookie};
pub(crate) use csrf_body::{MULTIPART_LIMIT, over_limit};
pub use guard::CurrentStaff;
pub(crate) use guard::{AdminCx, Gate, NavItem, forbidden, guard, request_ctx};
pub(crate) use login::{login_form, login_submit, logout, password_form, password_submit};
pub(crate) use pages::home;
pub(crate) use role_seed::seed as seed_roles;
pub(crate) use session::prune as prune_sessions;
pub use staff::{
    ADMINISTRATOR_ROLE, MIN_PASSWORD_CHARS, NewStaff, check_password_rules, create_staff,
    ensure_role, role_permissions, set_roles, set_temporary_password, staff_count,
};
pub(crate) use texts::TEXTS;

/// Session cookie.
pub(crate) const SESSION_COOKIE: &str = "hs_session";
/// Double-submit cookie of the login form.
pub(crate) const LOGIN_CSRF_COOKIE: &str = "hs_csrf";
/// One-shot flash message: a catalog key prefixed `ok:` or `err:`.
pub(crate) const FLASH_COOKIE: &str = "hs_flash";
/// Hidden form field carrying the CSRF token.
pub(crate) const CSRF_FIELD: &str = "_csrf";
/// Header carrying the CSRF token (HTMX requests).
pub(crate) const CSRF_HEADER: &str = "x-csrf-token";

const MINUTE_MS: i64 = 60_000;
/// Idle time after which a session ends.
pub(crate) const SLIDING_MS: i64 = 12 * 60 * MINUTE_MS;
/// Age after which a session ends regardless of activity.
pub(crate) const ABSOLUTE_MS: i64 = 7 * 24 * 60 * MINUTE_MS;
/// `last_seen_at` is written at most this often.
pub(crate) const TOUCH_EVERY_MS: i64 = MINUTE_MS;
/// Wrong passwords in a row that lock an account.
pub(crate) const LOCK_AFTER: i64 = 5;
/// How long the lock lasts.
pub(crate) const LOCK_MS: i64 = 15 * MINUTE_MS;

/// Back-office paths the Kernel mounts itself (reserved in `Routes`).
pub(crate) const HOME_PATH: &str = "/admin";
pub(crate) const LOGIN_PATH: &str = "/admin/login";
pub(crate) const LOGOUT_PATH: &str = "/admin/logout";
pub(crate) const PASSWORD_PATH: &str = "/admin/password";

/// SHA-256 as lowercase hex.
pub(crate) fn sha256_hex(data: &[u8]) -> String {
    use sha2::Digest;
    crate::id::hex(&sha2::Sha256::digest(data))
}

/// The CSRF token bound to a session token.
pub(crate) fn csrf_for(session_token: &str) -> String {
    sha256_hex(format!("csrf:{session_token}").as_bytes())
}

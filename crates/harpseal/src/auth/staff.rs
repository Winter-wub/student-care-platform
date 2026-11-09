//! Staff accounts: create, Roles, temporary and changed passwords. Every
//! write lands in one batch with its audit row.

use serde_json::json;

use super::{roles, session};
use crate::audit::{self, Actor, RequestCtx, Target};
use crate::message::Locale;
use crate::password::{self, Scheme};
use crate::port_types::Statement;
use crate::{Error, Result, Services};

/// Shortest password accepted, in characters (no composition rules).
pub const MIN_PASSWORD_CHARS: usize = 12;

/// Refuses a password shorter than [`MIN_PASSWORD_CHARS`].
pub fn check_password_rules(password: &str) -> Result<()> {
    if password.chars().count() < MIN_PASSWORD_CHARS {
        return Err(Error::Invalid(format!(
            "password must be at least {MIN_PASSWORD_CHARS} characters"
        )));
    }
    Ok(())
}

/// A Staff account to create with [`create_staff`].
#[derive(Clone)]
#[non_exhaustive]
pub struct NewStaff {
    /// Sign-in email; stored trimmed and lowercased.
    pub email: String,
    /// Name shown in the back office.
    pub display_name: String,
    /// At least [`MIN_PASSWORD_CHARS`] characters; never stored.
    pub password: String,
    /// Locale of the back office for this Staff member.
    pub locale: Locale,
    /// Role names (must exist).
    pub roles: Vec<String>,
    /// Ask for a new password at the first sign-in (temporary password).
    pub must_change_password: bool,
}

impl NewStaff {
    /// A Staff member without Roles who keeps `password`.
    pub fn new(email: &str, display_name: &str, password: &str, locale: Locale) -> Self {
        Self {
            email: email.into(),
            display_name: display_name.into(),
            password: password.into(),
            locale,
            roles: Vec::new(),
            must_change_password: false,
        }
    }
}

/// Never prints the password.
impl core::fmt::Debug for NewStaff {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("NewStaff")
            .field("email", &self.email)
            .field("display_name", &self.display_name)
            .field("password", &crate::audit::REDACTED)
            .field("locale", &self.locale)
            .field("roles", &self.roles)
            .field("must_change_password", &self.must_change_password)
            .finish()
    }
}

/// Normalised email: trimmed, lowercased.
pub(crate) fn normalise_email(email: &str) -> String {
    email.trim().to_lowercase()
}

/// A PHC string for `password` with the spec's Argon2id parameters.
pub(crate) fn hash_password(services: &Services, password: &str) -> Result<String> {
    let mut salt = [0u8; 16];
    services.random.fill(&mut salt)?;
    password::hash(Scheme::ARGON2ID_OWASP, password, &salt)
}

/// Creates a Staff account with its Roles; returns `staff.id`. Audited as
/// `staff.create` (no password or hash in the row).
pub async fn create_staff(
    services: &Services,
    actor: &Actor,
    s: NewStaff,
    ctx: &RequestCtx,
) -> Result<String> {
    let email = normalise_email(&s.email);
    if !email.contains('@') || email.len() > 254 {
        return Err(Error::Invalid("email is not valid".into()));
    }
    check_password_rules(&s.password)?;
    roles::check_exist(services, &s.roles).await?;
    let id = services.new_id()?;
    let now = services.now_ms();
    let mut stmts = vec![
        Statement::new(
            "INSERT INTO staff (id, email, display_name, password_hash, locale, \
             must_change_password, created_at, updated_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(id.clone())
        .bind(email.clone())
        .bind(s.display_name.trim())
        .bind(hash_password(services, &s.password)?)
        .bind(s.locale.0.clone())
        .bind(s.must_change_password)
        .bind(now)
        .bind(now),
    ];
    stmts.extend(s.roles.iter().map(|r| roles::grant(&id, r)));
    let after = json!({
        "email": email,
        "display_name": s.display_name.trim(),
        "roles": s.roles,
        "must_change_password": s.must_change_password,
    });
    let target = Target::new("staff", id.clone());
    stmts.push(audit::entry(
        services,
        actor,
        "staff.create",
        Some(&target),
        None,
        Some(after),
        ctx,
    )?);
    services.db.batch(stmts).await.map_err(|e| match e {
        Error::Db(m) if m.contains("UNIQUE") => Error::Invalid("email is already in use".into()),
        e => e,
    })?;
    Ok(id)
}

/// Replaces `staff_id`'s Roles with `role_names`. Audited as `staff.roles`
/// with the Role names before and after.
pub async fn set_roles(
    services: &Services,
    actor: &Actor,
    staff_id: &str,
    role_names: &[String],
    ctx: &RequestCtx,
) -> Result<()> {
    roles::check_exist(services, role_names).await?;
    let before = roles::roles_of(services, staff_id).await?;
    let mut after: Vec<String> = role_names.to_vec();
    after.sort();
    after.dedup();
    let mut stmts =
        vec![Statement::new("DELETE FROM staff_role WHERE staff_id = ?").bind(staff_id)];
    stmts.extend(after.iter().map(|r| roles::grant(staff_id, r)));
    let target = Target::new("staff", staff_id);
    stmts.push(audit::entry(
        services,
        actor,
        "staff.roles",
        Some(&target),
        Some(json!({ "roles": before })),
        Some(json!({ "roles": after })),
        ctx,
    )?);
    services.db.batch(stmts).await.map(|_| ())
}

/// An Administrator's password reset: sets a temporary password that must
/// be changed at the next sign-in, unlocks the account and ends every
/// session. Audited as `staff.password_reset`.
pub async fn set_temporary_password(
    services: &Services,
    actor: &Actor,
    staff_id: &str,
    password: &str,
    ctx: &RequestCtx,
) -> Result<()> {
    check_password_rules(password)?;
    let stmts = vec![
        Statement::new(
            "UPDATE staff SET password_hash = ?, must_change_password = 1, failed_logins = 0, \
             locked_until = NULL, updated_at = ? WHERE id = ?",
        )
        .bind(hash_password(services, password)?)
        .bind(services.now_ms())
        .bind(staff_id),
        session::delete_others(staff_id, None),
        audit::entry(
            services,
            actor,
            "staff.password_reset",
            Some(&Target::new("staff", staff_id)),
            None,
            Some(json!({ "must_change_password": true })),
            ctx,
        )?,
    ];
    services.db.batch(stmts).await.map(|_| ())
}

/// What the change form achieved.
pub(crate) enum Change {
    /// Changed; every session of the Staff member was ended and this is
    /// the cookie token of the new one.
    Done(String),
    /// `current` was wrong (counted toward the lock). `locked`: this
    /// failure locked the account, and every session was ended.
    WrongCurrent { locked: bool },
}

/// The Staff member's own change: `current` must verify, `new` must pass
/// the rules (checked by the caller). The session rotates: every session
/// of the Staff member ends, a new one starts. Audited as
/// `staff.password_change`, or `staff.password_change_failed` when
/// `current` is wrong.
pub(crate) async fn change_password(
    services: &Services,
    staff_id: &str,
    current: &str,
    new: &str,
    ctx: &RequestCtx,
) -> Result<Change> {
    let rows = services
        .db
        .query(
            Statement::new("SELECT password_hash, must_change_password FROM staff WHERE id = ?")
                .bind(staff_id),
        )
        .await?;
    let phc = session::text(&rows, "password_hash");
    let actor = Actor::Staff(staff_id.into());
    let target = Target::new("staff", staff_id);
    if phc.is_empty() || !password::verify(&phc, current).unwrap_or(false) {
        let (failures, locked) = super::attempt::count_failure(services, staff_id, ctx).await?;
        let after = json!({ "failures": failures, "locked": locked });
        let mut stmts = vec![audit::entry(
            services,
            &actor,
            "staff.password_change_failed",
            Some(&target),
            None,
            Some(after),
            ctx,
        )?];
        if locked {
            stmts.push(session::delete_others(staff_id, None));
        }
        services.db.batch(stmts).await?;
        return Ok(Change::WrongCurrent { locked });
    }
    let was_forced = rows.get(0, "must_change_password").and_then(|v| v.as_i64()) == Some(1);
    let (token, insert) = session::insert(services, staff_id, ctx)?;
    let stmts = vec![
        Statement::new(
            "UPDATE staff SET password_hash = ?, must_change_password = 0, failed_logins = 0, \
             updated_at = ? WHERE id = ?",
        )
        .bind(hash_password(services, new)?)
        .bind(services.now_ms())
        .bind(staff_id),
        session::delete_others(staff_id, None),
        insert,
        audit::entry(
            services,
            &actor,
            "staff.password_change",
            Some(&target),
            Some(json!({ "must_change_password": was_forced })),
            Some(json!({ "must_change_password": false })),
            ctx,
        )?,
    ];
    services.db.batch(stmts).await?;
    Ok(Change::Done(token))
}

/// Name of the Role the CLI creates for the first Administrator.
pub const ADMINISTRATOR_ROLE: &str = roles::ADMINISTRATOR;

/// Creates Role `name` with `permissions` when no Role has that name;
/// `Ok(true)` when it was created. An existing Role is left as it is.
/// Audited as `role.create`.
pub async fn ensure_role(
    services: &Services,
    actor: &Actor,
    name: &str,
    permissions: &[&str],
    ctx: &RequestCtx,
) -> Result<bool> {
    let exists = services
        .db
        .query(Statement::new("SELECT id FROM role WHERE name = ?").bind(name))
        .await?;
    if !exists.is_empty() {
        return Ok(false);
    }
    // The audit target is the new Role's id, as the Roles page writes it,
    // so the entry links to `/admin/roles/{id}`.
    let (id, mut stmts) = roles::ensure_role(services, name, permissions)?;
    let after = json!({ "name": name, "permissions": permissions });
    stmts.push(audit::entry(
        services,
        actor,
        "role.create",
        Some(&Target::new("role", id)),
        None,
        Some(after),
        ctx,
    )?);
    services.db.batch(stmts).await?;
    Ok(true)
}

/// The Permission ids Role `name` grants, sorted; empty for no such Role.
pub async fn role_permissions(services: &Services, name: &str) -> Result<Vec<String>> {
    let rows = services
        .db
        .query(
            Statement::new(
                "SELECT rp.permission_id FROM role_permission rp JOIN role r ON r.id = rp.role_id \
                 WHERE r.name = ? ORDER BY rp.permission_id",
            )
            .bind(name),
        )
        .await?;
    Ok(rows
        .rows
        .iter()
        .filter_map(|r| r.first().and_then(|v| v.as_text()).map(str::to_string))
        .collect())
}

/// How many Staff accounts exist.
pub async fn staff_count(services: &Services) -> Result<i64> {
    let rows = services
        .db
        .query(Statement::new("SELECT count(*) FROM staff"))
        .await?;
    Ok(rows
        .rows
        .first()
        .and_then(|r| r.first())
        .and_then(|v| v.as_i64())
        .unwrap_or(0))
}

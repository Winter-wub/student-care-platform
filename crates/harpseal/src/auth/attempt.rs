//! One sign-in attempt: lockout, constant work for unknown emails, audit
//! rows for success and failure, and the Security Event on lock.

use std::sync::OnceLock;

use serde_json::json;

use super::session::text;
use super::staff::normalise_email;
use super::{LOCK_AFTER, LOCK_MS};
use crate::audit::{self, Actor, RequestCtx, Target};
use crate::password::{self, Scheme};
use crate::port_types::Statement;
use crate::{Result, Services};

/// A signed-in Staff member.
pub(crate) struct Accepted {
    pub(crate) staff_id: String,
    pub(crate) must_change_password: bool,
}

/// A hash with the production parameters, verified when there is no real
/// hash to check, so unknown and locked accounts take as long as real ones.
fn dummy_hash() -> &'static str {
    static DUMMY: OnceLock<String> = OnceLock::new();
    DUMMY.get_or_init(|| {
        password::hash(
            Scheme::ARGON2ID_OWASP,
            "harpseal-dummy",
            b"harpseal-dummy-s",
        )
        .unwrap_or_default()
    })
}

fn burn(password: &str) {
    let _ = password::verify(dummy_hash(), password);
}

/// Checks `email` + `password`. `None` is every kind of refusal (unknown
/// email, wrong password, locked, disabled): the caller shows one text.
pub(crate) async fn attempt(
    services: &Services,
    email: &str,
    password: &str,
    ctx: &RequestCtx,
) -> Result<Option<Accepted>> {
    let email = normalise_email(email);
    let rows = services
        .db
        .query(
            Statement::new(
                "SELECT id, password_hash, status, failed_logins, locked_until, \
                 must_change_password FROM staff WHERE email = ?",
            )
            .bind(email.clone()),
        )
        .await?;
    if rows.is_empty() {
        burn(password);
        let after = json!({ "reason": "unknown_email" });
        audit::record(
            services,
            &Actor::System,
            "staff.login_failed",
            None,
            None,
            Some(after),
            ctx,
        )
        .await?;
        return Ok(None);
    }
    let int = |c: &str| rows.get(0, c).and_then(|v| v.as_i64());
    let id = text(&rows, "id");
    let target = Target::new("staff", id.clone());
    let actor = Actor::Staff(id.clone());
    let now = services.now_ms();
    let fail = |reason: &str| json!({ "email": email, "reason": reason });
    if int("locked_until").is_some_and(|until| until > now) {
        burn(password);
        audit::record(
            services,
            &actor,
            "staff.login_failed",
            Some(&target),
            None,
            Some(fail("locked")),
            ctx,
        )
        .await?;
        return Ok(None);
    }
    let ok = password::verify(&text(&rows, "password_hash"), password).unwrap_or(false);
    if text(&rows, "status") != "active" {
        audit::record(
            services,
            &actor,
            "staff.login_failed",
            Some(&target),
            None,
            Some(fail("disabled")),
            ctx,
        )
        .await?;
        return Ok(None);
    }
    if !ok {
        wrong_password(
            services,
            &id,
            int("failed_logins").unwrap_or(0) + 1,
            &email,
            ctx,
        )
        .await?;
        return Ok(None);
    }
    let stmts = vec![
        Statement::new("UPDATE staff SET failed_logins = 0, locked_until = NULL WHERE id = ?")
            .bind(id.clone()),
        audit::entry(
            services,
            &actor,
            "staff.login",
            Some(&target),
            None,
            None,
            ctx,
        )?,
    ];
    services.db.batch(stmts).await?;
    Ok(Some(Accepted {
        staff_id: id,
        must_change_password: int("must_change_password") == Some(1),
    }))
}

/// Counts a wrong password; the [`LOCK_AFTER`]th in a row locks the
/// account for [`LOCK_MS`] and writes Security Event `staff.login_locked`.
async fn wrong_password(
    services: &Services,
    id: &str,
    failures: i64,
    email: &str,
    ctx: &RequestCtx,
) -> Result<()> {
    let now = services.now_ms();
    let actor = Actor::Staff(id.into());
    let target = Target::new("staff", id);
    let locks = failures >= LOCK_AFTER;
    let after = json!({ "email": email, "reason": "wrong_password", "failures": failures, "locked": locks });
    let mut stmts = vec![audit::entry(
        services,
        &actor,
        "staff.login_failed",
        Some(&target),
        None,
        Some(after),
        ctx,
    )?];
    if locks {
        let until = now + LOCK_MS;
        stmts.push(
            Statement::new("UPDATE staff SET failed_logins = 0, locked_until = ? WHERE id = ?")
                .bind(until)
                .bind(id),
        );
        stmts.push(
            Statement::new(
                "INSERT INTO security_event (id, at, kind, staff_id, details) VALUES \
                 (?, ?, 'staff.login_locked', ?, json_object('locked_until', ?, 'ip', ?, \
                 'failures', ?))",
            )
            .bind(services.new_id()?)
            .bind(now)
            .bind(id)
            .bind(until)
            .bind(ctx.ip.clone())
            .bind(failures),
        );
    } else {
        stmts.push(
            Statement::new("UPDATE staff SET failed_logins = ? WHERE id = ?")
                .bind(failures)
                .bind(id),
        );
    }
    services.db.batch(stmts).await.map(|_| ())
}

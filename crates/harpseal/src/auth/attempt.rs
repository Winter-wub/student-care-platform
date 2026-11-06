//! One sign-in attempt: lockout, constant work for unknown emails, audit
//! rows for success and failure, and the Security Event on lock.

use serde_json::json;

use super::session::text;
use super::staff::normalise_email;
use super::{LOCK_AFTER, LOCK_MS};
use crate::audit::{self, Actor, RequestCtx, Target};
use crate::password;
use crate::port_types::Statement;
use crate::{Result, Services};

/// A signed-in Staff member.
pub(crate) struct Accepted {
    pub(crate) staff_id: String,
    pub(crate) must_change_password: bool,
}

/// `password::hash(ARGON2ID_OWASP, "harpseal-dummy", b"harpseal-dummy-s")`,
/// precomputed (a test checks it): verified when there is no real hash, so
/// unknown and locked accounts take as long as real ones, without a cold
/// isolate paying for a second Argon2 run to build it.
pub(crate) const DUMMY_PHC: &str = "$argon2id$v=19$m=19456,t=2,p=1$aGFycHNlYWwtZHVtbXktcw$rraRkJkQnMUEdnl0QQod/9J0HMhwd/L6fplM7BC2k+g";

fn burn(password: &str) {
    let _ = password::verify(DUMMY_PHC, password);
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
                "SELECT id, password_hash, status, locked_until, must_change_password \
                 FROM staff WHERE email = ?",
            )
            .bind(email.clone()),
        )
        .await?;
    if rows.is_empty() {
        burn(password);
        let after = json!({ "reason": "unknown_email" });
        let action = "staff.login_failed";
        audit::record(
            services,
            &Actor::System,
            action,
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
    let fail = |reason: &str| Some(json!({ "email": email, "reason": reason }));
    let refuse = |reason: &'static str| {
        audit::record(
            services,
            &actor,
            "staff.login_failed",
            Some(&target),
            None,
            fail(reason),
            ctx,
        )
    };
    if int("locked_until").is_some_and(|until| until > services.now_ms()) {
        burn(password);
        refuse("locked").await?;
        return Ok(None);
    }
    let ok = password::verify(&text(&rows, "password_hash"), password).unwrap_or(false);
    if text(&rows, "status") != "active" {
        refuse("disabled").await?;
        return Ok(None);
    }
    if !ok {
        let (failures, locked) = count_failure(services, &id, ctx).await?;
        let after = json!({ "email": email, "reason": "wrong_password",
                            "failures": failures, "locked": locked });
        let action = "staff.login_failed";
        audit::record(
            services,
            &actor,
            action,
            Some(&target),
            None,
            Some(after),
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

/// Counts one wrong password in the database (safe against parallel
/// attempts: the increment and the lock are single statements). The
/// [`LOCK_AFTER`]th in a row locks the account for [`LOCK_MS`], resets the
/// counter and writes Security Event `staff.lockout`; only the request
/// whose update set the lock writes it. Returns (failures, locked now).
pub(crate) async fn count_failure(
    services: &Services,
    id: &str,
    ctx: &RequestCtx,
) -> Result<(i64, bool)> {
    let rows = services
        .db
        .query(
            Statement::new(
                "UPDATE staff SET failed_logins = failed_logins + 1 WHERE id = ? \
                 RETURNING failed_logins",
            )
            .bind(id),
        )
        .await?;
    let failures = rows
        .rows
        .first()
        .and_then(|r| r.first())
        .and_then(|v| v.as_i64())
        .unwrap_or(0);
    if failures < LOCK_AFTER {
        return Ok((failures, false));
    }
    let now = services.now_ms();
    let until = now + LOCK_MS;
    let lock = Statement::new(
        "UPDATE staff SET locked_until = ?, failed_logins = 0 WHERE id = ? AND failed_logins >= ?",
    )
    .bind(until)
    .bind(id)
    .bind(LOCK_AFTER);
    if services.db.execute(lock).await?.rows_affected == 0 {
        return Ok((failures, false));
    }
    let event = Statement::new(
        "INSERT INTO security_event (id, at, kind, staff_id, details) VALUES \
         (?, ?, 'staff.lockout', ?, json_object('locked_until', ?, 'ip', ?, 'failures', ?))",
    )
    .bind(services.new_id()?)
    .bind(now)
    .bind(id)
    .bind(until)
    .bind(ctx.ip.clone())
    .bind(failures);
    services.db.execute(event).await?;
    Ok((failures, true))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::password::Scheme;

    #[test]
    fn dummy_phc_is_the_production_hash_it_claims_to_be() {
        let h = password::hash(
            Scheme::ARGON2ID_OWASP,
            "harpseal-dummy",
            b"harpseal-dummy-s",
        );
        assert_eq!(h.unwrap(), DUMMY_PHC);
    }
}

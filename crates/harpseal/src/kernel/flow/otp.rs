//! Steps 3–5: start a challenge (resend limit), judge answers (attempts,
//! lock, expiry), resend, cancel, and the Link itself.

use super::claim::{self, StudentRow};
use super::{Turn, account, actions, minutes_left};
use crate::kernel::security;
use crate::message::{Card, Choices, Message};
use crate::port_types::{Statement, Value};
use crate::verification::{StudentClaim, Verdict, VerifyContext};
use crate::{Error, Result, StudentId};

const HOUR_MS: i64 = 3_600_000;

fn cx(t: &Turn<'_>, student: &StudentRow) -> VerifyContext {
    VerifyContext::new(t.s().clone(), t.texts.in_locale(&student.locale))
}

/// `text` with the resend / cancel buttons.
fn with_buttons(t: &mut Turn<'_>, text: String) -> Result<()> {
    let options = vec![
        t.action("kernel.otp.resend", actions::OTP_RESEND)?,
        t.action("kernel.otp.cancel", actions::OTP_CANCEL)?,
    ];
    t.out.push(Message::Choices(Choices { text, options }));
    Ok(())
}

/// Opens a challenge unless the Student asked for too many codes this hour.
pub(super) async fn start(t: &mut Turn<'_>, student: &StudentRow) -> Result<()> {
    let now = t.s().now_ms();
    let window = Statement::new(
        "SELECT count(*) AS n, min(created_at) AS first FROM verification_challenge \
         WHERE student_id = ? AND created_at > ?",
    )
    .bind(student.id.clone())
    .bind(now - HOUR_MS);
    let rows = t.s().db.query(window).await?;
    let n = rows.get(0, "n").and_then(Value::as_i64).unwrap_or(0);
    let limit = i64::from(t.d.config().otp.resends_per_hour);
    if n >= limit {
        let first = rows.get(0, "first").and_then(Value::as_i64).unwrap_or(now);
        let minutes = minutes_left(now, first + HOUR_MS);
        t.out.push(Message::text(
            t.tf("kernel.otp.resend_limit", &[("minutes", &minutes)]),
        ));
        return Ok(());
    }
    t.s()
        .db
        .execute(account::cancel_challenges(&t.acct.id))
        .await?;
    let email = student
        .email
        .clone()
        .ok_or_else(|| Error::Invalid("student has no email".into()))?;
    let claim = StudentClaim {
        student: StudentId(student.id.clone()),
        student_code: student.code.clone(),
        display_name: student.name.clone(),
        email,
        locale: student.locale.clone(),
        channel_account: t.acct.id.clone(),
    };
    let challenge = t.d.verification().start(&cx(t, student), &claim).await?;
    t.acct.flow.student_id = Some(student.id.clone());
    t.acct.flow.challenge_id = Some(challenge.id);
    t.s().db.execute(t.save("awaiting_otp")).await?;
    let minutes = t.d.config().otp.ttl_minutes.to_string();
    let text = t.tf(
        "kernel.otp.sent",
        &[("email", &challenge.sent_to), ("minutes", &minutes)],
    );
    with_buttons(t, text)
}

/// Not a code: say what is expected, count nothing.
pub(super) fn reprompt(t: &mut Turn<'_>) {
    let digits = t.d.config().otp.digits.to_string();
    let text = t.tf("kernel.otp.prompt", &[("digits", &digits)]);
    // Only fails on an invalid Kernel Action id, which is a constant.
    let _ = with_buttons(t, text);
}

/// The claimed Student, or back to the student-id step when the claim is
/// gone (or the Student is locked meanwhile).
async fn claimed(t: &mut Turn<'_>) -> Result<Option<StudentRow>> {
    let student = match t.acct.flow.student_id.clone() {
        Some(id) => claim::by_id(t, &id).await?,
        None => None,
    };
    let now = t.s().now_ms();
    match student {
        Some(s) if s.active => match s.locked_until.filter(|u| *u > now) {
            Some(until) => {
                let minutes = minutes_left(now, until);
                t.out.push(Message::text(
                    t.tf("kernel.otp.locked", &[("minutes", &minutes)]),
                ));
                back_to_student_id(t).await?;
                Ok(None)
            }
            None => Ok(Some(s)),
        },
        _ => {
            back_to_student_id(t).await?;
            t.say("kernel.link.ask_student_id");
            Ok(None)
        }
    }
}

async fn back_to_student_id(t: &mut Turn<'_>) -> Result<()> {
    t.reset_flow();
    let stmts = vec![
        account::cancel_challenges(&t.acct.id),
        t.save("awaiting_student_id"),
    ];
    t.s().db.batch(stmts).await.map(|_| ())
}

/// Step 4: digits only after removing spaces and dashes.
pub(super) async fn answer(t: &mut Turn<'_>, typed: &str) -> Result<()> {
    let code: String = typed
        .chars()
        .filter(|c| !matches!(c, ' ' | '-') && !c.is_whitespace())
        .collect();
    if code.is_empty() || !code.chars().all(|c| c.is_ascii_digit()) {
        reprompt(t);
        return Ok(());
    }
    let Some(student) = claimed(t).await? else {
        return Ok(());
    };
    let Some(challenge) = t.acct.flow.challenge_id.clone() else {
        back_to_student_id(t).await?;
        t.say("kernel.link.ask_student_id");
        return Ok(());
    };
    match t
        .d
        .verification()
        .check(&cx(t, &student), &challenge, &code)
        .await?
    {
        Verdict::Passed => link(t, &student, &challenge).await,
        Verdict::Expired => {
            let expire = Statement::new(
                "UPDATE verification_challenge SET state = 'expired' WHERE id = ? AND state = 'pending'",
            )
            .bind(challenge);
            t.s().db.execute(expire).await?;
            let text = t.t("kernel.otp.expired");
            with_buttons(t, text)
        }
        _ => wrong(t, &student, &challenge).await,
    }
}

/// Counts the attempt; the last one locks the Student.
async fn wrong(t: &mut Turn<'_>, student: &StudentRow, challenge: &str) -> Result<()> {
    let bump =
        Statement::new("UPDATE verification_challenge SET attempts = attempts + 1 WHERE id = ?")
            .bind(challenge);
    t.s().db.execute(bump).await?;
    let read =
        Statement::new("SELECT attempts FROM verification_challenge WHERE id = ?").bind(challenge);
    let rows = t.s().db.query(read).await?;
    let attempts = rows.get(0, "attempts").and_then(Value::as_i64).unwrap_or(0);
    let otp = &t.d.config().otp;
    let left = i64::from(otp.max_attempts) - attempts;
    if left > 0 {
        let text = t.tf("kernel.otp.wrong", &[("left", &left.to_string())]);
        return with_buttons(t, text);
    }
    let now = t.s().now_ms();
    let lock_ms = i64::from(otp.lock_minutes) * 60_000;
    let details = [
        ("channel", t.from.channel.as_str().to_string()),
        ("attempts", attempts.to_string()),
    ];
    let event = security::statement(
        t.s(),
        "otp.lockout",
        Some(&t.acct.id),
        Some(&student.id),
        &details,
    )?;
    t.reset_flow();
    let stmts = vec![
        Statement::new("UPDATE verification_challenge SET state = 'failed' WHERE id = ?")
            .bind(challenge),
        Statement::new(
            "UPDATE student SET verification_locked_until = ?, updated_at = ? WHERE id = ?",
        )
        .bind(now + lock_ms)
        .bind(now)
        .bind(student.id.clone()),
        event,
        t.save("awaiting_student_id"),
    ];
    t.s().db.batch(stmts).await?;
    let minutes = otp.lock_minutes.to_string();
    t.out.push(Message::text(
        t.tf("kernel.otp.locked", &[("minutes", &minutes)]),
    ));
    let others = account::other_links(t.s(), &student.id, &t.acct.id, None).await?;
    t.warn(others, "kernel.otp.lockout_notice", &student.locale);
    Ok(())
}

/// `kernel.otp.resend`.
pub(super) async fn resend(t: &mut Turn<'_>) -> Result<()> {
    match claimed(t).await? {
        Some(student) => start(t, &student).await,
        None => Ok(()),
    }
}

/// `kernel.otp.cancel`: back to the student-id prompt.
pub(super) async fn cancel(t: &mut Turn<'_>) -> Result<()> {
    back_to_student_id(t).await?;
    t.say("kernel.link.ask_student_id");
    Ok(())
}

/// Step 5, one batch: challenge passed, active Link, Consent (the version
/// shown on the Card), state `linked`, audit `link.create`. Then the Menu
/// unlocks and the success Card and the Menu Choices go out.
async fn link(t: &mut Turn<'_>, student: &StudentRow, challenge: &str) -> Result<()> {
    let s = t.s().clone();
    let now = s.now_ms();
    let link_id = s.new_id()?;
    let version = t
        .acct
        .flow
        .consent_version
        .clone()
        .unwrap_or_else(|| t.version.clone());
    let method = t.d.verification().id();
    let audit = account::audit(
        &s,
        "link.create",
        &student.id,
        &link_id,
        &[
            ("channel", t.from.channel.as_str().to_string()),
            ("channel_account_id", t.acct.id.clone()),
            ("method", method.to_string()),
        ],
    )?;
    t.acct.flow = Default::default();
    let stmts = vec![
        Statement::new("UPDATE verification_challenge SET state = 'passed' WHERE id = ?").bind(challenge),
        Statement::new(
            "INSERT INTO link (id, student_id, channel_account_id, channel, status, method, \
             linked_at, updated_at) VALUES (?, ?, ?, ?, 'active', ?, ?, ?)",
        )
        .bind(link_id)
        .bind(student.id.clone())
        .bind(t.acct.id.clone())
        .bind(t.from.channel.as_str())
        .bind(method)
        .bind(now)
        .bind(now),
        Statement::new(
            "INSERT INTO consent (id, student_id, channel_account_id, notice_version, locale, \
             accepted_at) VALUES (?, ?, ?, ?, ?, ?) ON CONFLICT (student_id, notice_version) DO NOTHING",
        )
        .bind(s.new_id()?)
        .bind(student.id.clone())
        .bind(t.acct.id.clone())
        .bind(version)
        .bind(student.locale.as_str())
        .bind(now),
        t.save("linked"),
        audit,
    ];
    s.db.batch(stmts).await?;
    t.texts = t.texts.in_locale(&student.locale);
    t.set_menu(Some(&student.locale)).await?;
    let body = t.tf("kernel.link.success", &[("name", &student.name)]);
    t.out.push(Message::Card(Card {
        title: t.t("kernel.link.success_title"),
        body,
        buttons: Vec::new(),
    }));
    t.menu_choices();
    Ok(())
}

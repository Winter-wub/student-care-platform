//! Steps 3–5: start a challenge (resend limit), judge answers (attempts,
//! lock, expiry), resend, cancel, and the Link itself.

use super::claim::{self, StudentRow};
use super::{Turn, account, actions, consent, minutes_left};
use crate::audit::{self, Actor, RequestCtx, Target};
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
    let email = student
        .email
        .clone()
        .ok_or_else(|| Error::Invalid("student has no email".into()))?;
    // Claim the step first: of two Inbounds of this account racing here,
    // only the one that moves the state opens a challenge.
    t.acct.flow.student_id = Some(student.id.clone());
    t.acct.flow.challenge_id = None;
    if !t.save("awaiting_otp").await? {
        return Ok(());
    }
    t.s()
        .db
        .execute(account::cancel_challenges(&t.acct.id))
        .await?;
    let claim = StudentClaim {
        student: StudentId(student.id.clone()),
        student_code: student.code.clone(),
        display_name: student.name.clone(),
        email,
        locale: student.locale.clone(),
        channel_account: t.acct.id.clone(),
    };
    let challenge = match t.d.verification().start(&cx(t, student), &claim).await {
        Ok(c) => c,
        Err(e) => return send_failed(t, student, e).await,
    };
    t.acct.flow.challenge_id = Some(challenge.id);
    let minutes = t.d.config().otp.ttl_minutes.to_string();
    let text = t.tf(
        "kernel.otp.sent",
        &[("email", &challenge.sent_to), ("minutes", &minutes)],
    );
    with_buttons(t, text)?;
    t.save("awaiting_otp").await?;
    Ok(())
}

/// The method could not open the challenge (e.g. the email bounced at the
/// provider): no code counts against the hour, the Student is told to try
/// later, and a Security Event `otp.send_failed` keeps the reason.
async fn send_failed(t: &mut Turn<'_>, student: &StudentRow, e: Error) -> Result<()> {
    let details = [("error", e.to_string().chars().take(200).collect())];
    security::record(
        t.s(),
        "otp.send_failed",
        Some(&t.acct.id),
        Some(&student.id),
        &details,
    )
    .await?;
    t.say("kernel.otp.send_failed");
    t.reset_flow();
    t.save("awaiting_student_id").await?;
    Ok(())
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
    let cancel = vec![account::cancel_challenges(&t.acct.id)];
    t.commit(cancel, "awaiting_student_id").await.map(|_| ())
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
    // Claim one attempt before comparing, atomically: the cap holds
    // however many answers arrive at once. Passing refunds it, so only
    // wrong answers count.
    let now = t.s().now_ms();
    let max = i64::from(t.d.config().otp.max_attempts);
    let claim_attempt = Statement::new(
        "UPDATE verification_challenge SET attempts = attempts + 1 \
         WHERE id = ? AND state = 'pending' AND attempts < ? AND expires_at > ? \
         RETURNING attempts",
    )
    .bind(challenge.clone())
    .bind(max)
    .bind(now);
    let claimed_rows = t.s().db.query(claim_attempt).await?;
    let Some(attempts) = claimed_rows.get(0, "attempts").and_then(Value::as_i64) else {
        return closed(t, &challenge).await;
    };
    match t
        .d
        .verification()
        .check(&cx(t, &student), &challenge, &code)
        .await?
    {
        Verdict::Passed => link(t, &student, &challenge).await,
        Verdict::Expired => expired(t, &challenge).await,
        _ => wrong(t, &student, &challenge, attempts).await,
    }
}

/// The challenge took no attempt (or lost its pass): past its expiry
/// (offer a resend), or used up / failed by a concurrent answer (locked).
///
/// The used-up case changes nothing: the answer that holds the last
/// attempt finishes the lockout or the Link, and cancelling the challenge
/// or moving the flow here would make that answer lose its own outcome.
async fn closed(t: &mut Turn<'_>, challenge: &str) -> Result<()> {
    let read = Statement::new("SELECT state, expires_at FROM verification_challenge WHERE id = ?")
        .bind(challenge);
    let rows = t.s().db.query(read).await?;
    let state = rows.get(0, "state").and_then(Value::as_text).unwrap_or("");
    let expires_at = rows
        .get(0, "expires_at")
        .and_then(Value::as_i64)
        .unwrap_or(0);
    let used_up = state == "failed" || (state == "pending" && expires_at > t.s().now_ms());
    if !used_up {
        return expired(t, challenge).await;
    }
    let minutes = t.d.config().otp.lock_minutes.to_string();
    t.out.push(Message::text(
        t.tf("kernel.otp.locked", &[("minutes", &minutes)]),
    ));
    Ok(())
}

async fn expired(t: &mut Turn<'_>, challenge: &str) -> Result<()> {
    let expire = Statement::new(
        "UPDATE verification_challenge SET state = 'expired' WHERE id = ? AND state = 'pending'",
    )
    .bind(challenge);
    t.s().db.execute(expire).await?;
    let text = t.t("kernel.otp.expired");
    with_buttons(t, text)
}

/// A wrong answer, already counted; the last allowed one locks the
/// Student.
async fn wrong(
    t: &mut Turn<'_>,
    student: &StudentRow,
    challenge: &str,
    attempts: i64,
) -> Result<()> {
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
    let minutes = otp.lock_minutes.to_string();
    t.out.push(Message::text(
        t.tf("kernel.otp.locked", &[("minutes", &minutes)]),
    ));
    let others = account::other_links(t.s(), &student.id, &t.acct.id, None).await?;
    t.warn(others, "kernel.otp.lockout_notice", &student.locale);
    t.reset_flow();
    let stmts = vec![
        Statement::new(
            "UPDATE verification_challenge SET state = 'failed' WHERE id = ? AND state = 'pending'",
        )
        .bind(challenge),
        Statement::new(
            "UPDATE student SET verification_locked_until = ?, updated_at = ? WHERE id = ?",
        )
        .bind(now + lock_ms)
        .bind(now)
        .bind(student.id.clone()),
        event,
    ];
    // The lock and the Security Event land even if a concurrent Inbound
    // moved the account first, and the locked text is still answered.
    t.commit_outcome(stmts, "awaiting_student_id").await?;
    Ok(())
}

/// `kernel.otp.resend`.
pub(super) async fn resend(t: &mut Turn<'_>) -> Result<()> {
    match claimed(t).await? {
        // The address may have changed since the claim (an import, a Staff
        // edit): a code never goes to one that fails the claim's checks.
        Some(student) if !claim::email_ok(t, &student).await? => back_to_student_id(t).await,
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
    // The Consent written with the Link must be one this account accepted:
    // with none remembered, the Card again (the claim starts over).
    let Some(version) = t.acct.flow.consent_version.clone() else {
        return consent::offer(t, false).await;
    };
    // Another account may have linked this Student on this Channel while
    // this one waited for its code.
    if claim::duplicate(t, student).await? {
        return back_to_student_id(t).await;
    }
    let pass = Statement::new(
        "UPDATE verification_challenge SET state = 'passed', attempts = max(attempts - 1, 0) \
         WHERE id = ? AND state = 'pending'",
    )
    .bind(challenge);
    if t.s().db.execute(pass).await?.rows_affected != 1 {
        // A concurrent wrong answer locked it first.
        return closed(t, challenge).await;
    }
    let s = t.s().clone();
    let now = s.now_ms();
    let link_id = s.new_id()?;
    let method = t.d.verification().id();
    let after = serde_json::json!({
        "channel": t.from.channel.as_str(),
        "channel_account_id": t.acct.id,
        "student_id": student.id,
        "method": method,
    });
    let audit = audit::entry(
        &s,
        &Actor::Student(student.id.clone()),
        "link.create",
        Some(&Target::new("link", link_id.clone())),
        None,
        Some(after),
        &RequestCtx::default(),
    )?;
    let flow = std::mem::take(&mut t.acct.flow);
    let stmts = vec![
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
        audit,
    ];
    match t.commit(stmts, "linked").await {
        Ok(true) => {}
        Ok(false) => return Ok(()),
        // The unique index lost the race to another account's Link.
        Err(Error::Db(e)) if e.contains("UNIQUE") => {
            t.acct.flow = flow;
            if claim::duplicate(t, student).await? {
                return back_to_student_id(t).await;
            }
            return Err(Error::Db(e));
        }
        Err(e) => return Err(e),
    }
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

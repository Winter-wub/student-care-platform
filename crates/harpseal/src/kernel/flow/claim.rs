//! Step 3: the typed student id, checked against the TOML pattern and the
//! roster, then handed to the Verification Method.

use super::{Turn, minutes_left, otp};
use crate::Result;
use crate::kernel::security;
use crate::kernel::students::email;
use crate::message::Locale;
use crate::port_types::{Statement, Value};

/// A roster row, as the claim needs it.
#[derive(Debug, Clone)]
pub(super) struct StudentRow {
    pub(super) id: String,
    pub(super) code: String,
    pub(super) name: String,
    pub(super) email: Option<String>,
    pub(super) locale: Locale,
    pub(super) active: bool,
    pub(super) locked_until: Option<i64>,
}

const BY: &str = "SELECT id, student_code, display_name, email, locale, status, \
    verification_locked_until FROM student WHERE ";

pub(super) async fn by_code(t: &Turn<'_>, code: &str) -> Result<Option<StudentRow>> {
    read(
        t,
        Statement::new(format!("{BY}student_code = ?")).bind(code),
    )
    .await
}

pub(super) async fn by_id(t: &Turn<'_>, id: &str) -> Result<Option<StudentRow>> {
    read(t, Statement::new(format!("{BY}id = ?")).bind(id)).await
}

async fn read(t: &Turn<'_>, stmt: Statement) -> Result<Option<StudentRow>> {
    let rows = t.s().db.query(stmt).await?;
    let text = |c: &str| rows.get(0, c).and_then(Value::as_text).map(str::to_string);
    let Some(id) = text("id") else {
        return Ok(None);
    };
    Ok(Some(StudentRow {
        id,
        code: text("student_code").unwrap_or_default(),
        name: text("display_name").unwrap_or_default(),
        email: text("email").filter(|e| !e.trim().is_empty()),
        locale: Locale(text("locale").unwrap_or_else(|| "th".into())),
        active: text("status").as_deref() == Some("active"),
        locked_until: rows
            .get(0, "verification_locked_until")
            .and_then(Value::as_i64),
    }))
}

/// Whether `id` (spaces already removed) fits `pattern` (see
/// `StudentConfig::id_pattern`): `#` digit, `@` letter, `?` either, other
/// characters literal, `|` between alternatives. `None`: 4 to 20 ASCII
/// letters, digits or `-`.
pub(crate) fn fits(id: &str, pattern: Option<&str>) -> bool {
    let Some(pattern) = pattern else {
        let ok = |c: char| c.is_ascii_alphanumeric() || c == '-';
        return (4..=20).contains(&id.len()) && id.chars().all(ok);
    };
    pattern.split('|').any(|alt| {
        alt.chars().count() == id.chars().count()
            && alt.chars().zip(id.chars()).all(|(p, c)| match p {
                '#' => c.is_ascii_digit(),
                '@' => c.is_ascii_alphabetic(),
                '?' => c.is_ascii_alphanumeric(),
                lit => lit == c,
            })
    })
}

/// Checks in order: format, roster (missing or left share one text), email,
/// a live Link of the same Student on this Channel from another account
/// (refused, the old account warned, Security Event), lock; then starts
/// the Verification Method.
pub(super) async fn student_id(t: &mut Turn<'_>, typed: &str) -> Result<()> {
    let code: String = typed.chars().filter(|c| !c.is_whitespace()).collect();
    if !fits(&code, t.d.config().student.id_pattern.as_deref()) {
        t.say("kernel.link.bad_format");
        return Ok(());
    }
    let Some(student) = by_code(t, &code).await?.filter(|s| s.active) else {
        t.say("kernel.link.not_found");
        return Ok(());
    };
    let Some(address) = student.email.as_deref() else {
        t.say("kernel.link.no_email");
        return Ok(());
    };
    // The roster only warns about a bad address (#71), so one can be on
    // file: a code sent there either bounces for ever or goes to a stranger.
    if !email::is_email(address.trim()) {
        t.say("kernel.link.bad_email");
        return Ok(());
    }
    if shared_email(t, &student, address).await? {
        return Ok(());
    }
    if duplicate(t, &student).await? {
        return Ok(());
    }
    let now = t.s().now_ms();
    if let Some(until) = student.locked_until.filter(|u| *u > now) {
        let minutes = minutes_left(now, until);
        t.out.push(crate::Message::text(
            t.tf("kernel.otp.locked", &[("minutes", &minutes)]),
        ));
        return Ok(());
    }
    otp::start(t, &student).await
}

/// Whether another Student who is not left has the same address (trimmed,
/// any case): the code would reach whoever reads that mailbox, so either
/// could link as the other. Refused to Staff (a Manual Link), with a
/// Security Event `link.shared_email` every time.
async fn shared_email(t: &mut Turn<'_>, student: &StudentRow, address: &str) -> Result<bool> {
    let stmt = Statement::new(
        "SELECT count(*) AS n FROM student \
         WHERE id <> ? AND status <> 'left' AND lower(trim(email)) = ?",
    )
    .bind(student.id.clone())
    .bind(email::key(address));
    let rows = t.s().db.query(stmt).await?;
    if rows.get(0, "n").and_then(Value::as_i64).unwrap_or(0) == 0 {
        return Ok(false);
    }
    t.say("kernel.link.shared_email");
    let details = [
        ("channel", t.from.channel.as_str().to_string()),
        ("student_code", student.code.clone()),
    ];
    security::record(
        t.s(),
        "link.shared_email",
        Some(&t.acct.id),
        Some(&student.id),
        &details,
    )
    .await?;
    Ok(true)
}

/// A live Link of this Student on this Channel held by another account:
/// refuse (`kernel.link.already_linked`), record a Security Event every
/// time, and Push a warning to that account at most once per Student per
/// hour.
pub(super) async fn duplicate(t: &mut Turn<'_>, student: &StudentRow) -> Result<bool> {
    let stmt = Statement::new(
        "SELECT ca.id, ca.external_id FROM link l \
         JOIN channel_account ca ON ca.id = l.channel_account_id \
         WHERE l.student_id = ? AND l.channel = ? AND l.status <> 'revoked' AND ca.id <> ?",
    )
    .bind(student.id.clone())
    .bind(t.from.channel.as_str())
    .bind(t.acct.id.clone());
    let rows = t.s().db.query(stmt).await?;
    let Some(old) = rows.get(0, "external_id").and_then(Value::as_text) else {
        return Ok(false);
    };
    let old = old.to_string();
    t.say("kernel.link.already_linked");
    let recent = Statement::new(
        "SELECT count(*) AS n FROM security_event \
         WHERE kind = 'link.duplicate_attempt' AND student_id = ? AND at > ?",
    )
    .bind(student.id.clone())
    .bind(t.s().now_ms() - 3_600_000);
    let warned = t.s().db.query(recent).await?;
    let warned = warned.get(0, "n").and_then(Value::as_i64).unwrap_or(0) > 0;
    let details = [
        ("channel", t.from.channel.as_str().to_string()),
        ("student_code", student.code.clone()),
    ];
    security::record(
        t.s(),
        "link.duplicate_attempt",
        Some(&t.acct.id),
        Some(&student.id),
        &details,
    )
    .await?;
    if !warned {
        let kind = t.from.channel.as_str().to_string();
        t.warn(
            vec![(kind, old)],
            "kernel.link.attempt_warning",
            &student.locale,
        );
    }
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::fits;

    #[test]
    fn the_default_accepts_plain_codes() {
        assert!(fits("68010001", None));
        assert!(fits("B6801-01", None));
        assert!(!fits("123", None));
        assert!(!fits("68010001!", None));
        assert!(!fits(&"1".repeat(21), None));
    }

    #[test]
    fn patterns_use_hash_at_and_question_mark() {
        let p = Some("########|@#######");
        assert!(fits("68010001", p));
        assert!(fits("B6801000", p));
        assert!(!fits("6801000", p));
        assert!(!fits("BB801000", p));
        assert!(fits("68-AB", Some("##-??")));
        assert!(!fits("68_AB", Some("##-??")));
    }
}

//! The Verification Method seam and the email-OTP method.
//!
//! The Kernel owns the chat flow, attempt counting, resend limits and
//! locks; a method only opens a challenge (`start`) and judges an answer
//! (`check`). Both get a [`VerifyContext`]: the ports plus the catalog in
//! the Student's Locale (with `setting` overrides).

use async_trait::async_trait;
use sha2::{Digest, Sha256};

use crate::config::OtpConfig;
use crate::delivery::{StudentId, Timestamp};
use crate::kernel::text::Texts;
use crate::message::Locale;
use crate::port_types::{Email, Statement};
use crate::{Error, Result, Services};

/// The Student a Channel Account claims to be, after the Kernel checked
/// the roster (active, has an email, not locked, not linked elsewhere).
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct StudentClaim {
    /// `student.id`.
    pub student: StudentId,
    /// The student code the Student typed.
    pub student_code: String,
    /// Name from the roster.
    pub display_name: String,
    /// Email from the roster.
    pub email: String,
    /// The Student's Locale.
    pub locale: Locale,
    /// `channel_account.id` of the account claiming.
    pub channel_account: String,
}

/// An open Verification attempt, stored in `verification_challenge`.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Challenge {
    /// `verification_challenge.id`.
    pub id: String,
    /// Where the secret went, safe to show (e.g. a masked email).
    pub sent_to: String,
    /// After this instant `check` answers [`Verdict::Expired`].
    pub expires_at: Timestamp,
}

/// What `check` decided about one answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Verdict {
    /// The answer is right and the challenge was still open.
    Passed,
    /// The answer is wrong; the Kernel counts the attempt.
    Wrong,
    /// The challenge is past its expiry or no longer open.
    Expired,
}

/// What a Verification Method gets from the Kernel.
#[non_exhaustive]
pub struct VerifyContext {
    services: Services,
    texts: Texts,
}

impl VerifyContext {
    pub(crate) fn new(services: Services, texts: Texts) -> Self {
        Self { services, texts }
    }
    /// The runtime ports.
    pub fn services(&self) -> &Services {
        &self.services
    }
    /// A text in the Student's Locale (catalog, `[texts]`, `setting`).
    pub fn t(&self, key: &str) -> String {
        self.texts.t(key)
    }
    /// The Student's Locale.
    pub fn locale(&self) -> &Locale {
        self.texts.locale()
    }
}

impl core::fmt::Debug for VerifyContext {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("VerifyContext { .. }")
    }
}

/// A swappable way of proving which Student owns a Channel Account.
#[async_trait]
pub trait VerificationMethod: Send + Sync {
    /// Stable id stored in `verification_challenge.method` and
    /// `link.method`, e.g. `email_otp`.
    fn id(&self) -> &'static str;
    /// Opens a challenge for `claim` (writes its `verification_challenge`
    /// row, e.g. emails a code). The Kernel has already cancelled any
    /// earlier open challenge of the account.
    async fn start(&self, cx: &VerifyContext, claim: &StudentClaim) -> Result<Challenge>;
    /// Judges `answer` for challenge `challenge_id`. Writes nothing: the
    /// Kernel records attempts and the outcome.
    async fn check(&self, cx: &VerifyContext, challenge_id: &str, answer: &str) -> Result<Verdict>;
}

/// Email OTP: a code sent to the roster email, compared when it comes back.
/// Parameters default to the spec (6 digits, 5 minutes, 3 wrong attempts,
/// 3 codes per hour, 30-minute lock) and come from `[otp]` in the TOML; the
/// method uses `digits` and `ttl_minutes`, the Kernel the rest.
///
/// The code is never stored: `secret_hash` is
/// `sha256$<salt>$<hex SHA-256 of salt ":" challenge id ":" code>` with a
/// fresh 16-byte salt per challenge. Texts: `kernel.otp.email_subject`
/// and `kernel.otp.email_body` (`{code}`, `{minutes}`, `{name}`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EmailOtp {
    settings: OtpConfig,
}

impl EmailOtp {
    /// Email OTP with the given parameters (usually `config.otp`).
    pub fn new(settings: OtpConfig) -> Self {
        Self { settings }
    }
    /// The parameters in use.
    pub fn settings(&self) -> &OtpConfig {
        &self.settings
    }
}

impl Default for EmailOtp {
    fn default() -> Self {
        Self::new(OtpConfig::default())
    }
}

/// A uniform code of `digits` digits (4..=9) by rejection sampling.
fn code(services: &Services, digits: u8) -> Result<String> {
    let digits = digits.clamp(4, 9);
    let modulus = 10u32.pow(u32::from(digits));
    let limit = u32::MAX - u32::MAX % modulus;
    loop {
        let mut b = [0u8; 4];
        services.random.fill(&mut b)?;
        let n = u32::from_le_bytes(b);
        if n < limit {
            return Ok(format!(
                "{:0width$}",
                n % modulus,
                width = usize::from(digits)
            ));
        }
    }
}

fn digest(salt: &str, challenge_id: &str, code: &str) -> String {
    let mut h = Sha256::new();
    h.update(salt.as_bytes());
    h.update(b":");
    h.update(challenge_id.as_bytes());
    h.update(b":");
    h.update(code.as_bytes());
    crate::id::hex(&h.finalize())
}

/// Equal in time independent of where the first difference is.
fn same(a: &str, b: &str) -> bool {
    a.len() == b.len()
        && a.bytes()
            .zip(b.bytes())
            .fold(0u8, |acc, (x, y)| acc | (x ^ y))
            == 0
}

/// `s68010001@x.example` -> `s*******1@x.example`; a one- or two-char
/// local part keeps only its first character.
pub(crate) fn mask_email(email: &str) -> String {
    let Some((local, domain)) = email.split_once('@') else {
        return "***".into();
    };
    let chars: Vec<char> = local.chars().collect();
    match chars.as_slice() {
        [] => format!("***@{domain}"),
        [a] | [a, _] => format!("{a}***@{domain}"),
        [a, mid @ .., z] => format!("{a}{}{z}@{domain}", "*".repeat(mid.len().min(6))),
    }
}

#[async_trait]
impl VerificationMethod for EmailOtp {
    fn id(&self) -> &'static str {
        "email_otp"
    }

    async fn start(&self, cx: &VerifyContext, claim: &StudentClaim) -> Result<Challenge> {
        let s = cx.services();
        let id = s.new_id()?;
        let code = code(s, self.settings.digits)?;
        let salt = s.random_hex(16)?;
        let hash = format!("sha256${salt}${}", digest(&salt, &id, &code));
        let now = s.now_ms();
        let ttl = i64::from(self.settings.ttl_minutes) * 60_000;
        let expires_at = Timestamp(now).plus_ms(ttl);
        let insert = Statement::new(
            "INSERT INTO verification_challenge (id, channel_account_id, student_id, method, \
             secret_hash, expires_at, created_at) VALUES (?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(id.clone())
        .bind(claim.channel_account.clone())
        .bind(claim.student.0.clone())
        .bind(self.id())
        .bind(hash)
        .bind(expires_at.ms())
        .bind(now);
        s.db.execute(insert).await?;
        let minutes = self.settings.ttl_minutes.to_string();
        let args = [
            ("code", code.as_str()),
            ("minutes", minutes.as_str()),
            ("name", claim.display_name.as_str()),
        ];
        let email = Email {
            to: claim.email.clone(),
            subject: crate::kernel::text::fill(&cx.t("kernel.otp.email_subject"), &args),
            text: crate::kernel::text::fill(&cx.t("kernel.otp.email_body"), &args),
            html: None,
        };
        s.email.send(email).await?;
        Ok(Challenge {
            id,
            sent_to: mask_email(&claim.email),
            expires_at,
        })
    }

    async fn check(&self, cx: &VerifyContext, challenge_id: &str, answer: &str) -> Result<Verdict> {
        let s = cx.services();
        let rows =
            s.db.query(
                Statement::new(
                    "SELECT secret_hash, expires_at, state FROM verification_challenge \
                     WHERE id = ? AND method = ?",
                )
                .bind(challenge_id)
                .bind(self.id()),
            )
            .await?;
        let (Some(hash), Some(expires_at), Some(state)) = (
            rows.get(0, "secret_hash").and_then(|v| v.as_text()),
            rows.get(0, "expires_at").and_then(|v| v.as_i64()),
            rows.get(0, "state").and_then(|v| v.as_text()),
        ) else {
            return Ok(Verdict::Expired);
        };
        if state != "pending" || s.now_ms() >= expires_at {
            return Ok(Verdict::Expired);
        }
        let mut parts = hash.splitn(3, '$');
        let (Some("sha256"), Some(salt), Some(expected)) =
            (parts.next(), parts.next(), parts.next())
        else {
            return Err(Error::Invalid("unknown verification secret format".into()));
        };
        Ok(if same(&digest(salt, challenge_id, answer), expected) {
            Verdict::Passed
        } else {
            Verdict::Wrong
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn emails_are_masked() {
        assert_eq!(
            mask_email("s68010001@student.sealbay.example"),
            "s******1@student.sealbay.example"
        );
        assert_eq!(mask_email("ab@x.example"), "a***@x.example");
        assert_eq!(mask_email("a@x.example"), "a***@x.example");
        assert_eq!(mask_email("broken"), "***");
    }

    #[test]
    fn digests_bind_salt_challenge_and_code() {
        let a = digest("s", "c", "123456");
        assert_eq!(a.len(), 64);
        assert_ne!(a, digest("t", "c", "123456"));
        assert_ne!(a, digest("s", "d", "123456"));
        assert_ne!(a, digest("s", "c", "123457"));
        assert!(same(&a, &digest("s", "c", "123456")));
    }
}

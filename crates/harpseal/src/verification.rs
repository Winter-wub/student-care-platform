//! The Verification Method seam and the email-OTP method's parameters.

use async_trait::async_trait;

use crate::config::OtpConfig;
use crate::delivery::StudentId;
use crate::{Error, Result, Services};

/// What the Student typed to claim an identity (their student code).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StudentClaim(pub String);

/// An open Verification attempt, stored by the Kernel in
/// `verification_challenge`.
#[derive(Debug, Clone)]
pub struct Challenge {
    /// `verification_challenge.id`.
    pub id: String,
    /// Catalog key of the prompt shown to the Student, e.g.
    /// `kernel.otp.sent`.
    pub prompt_key: &'static str,
}

/// A swappable way of proving which Student owns a Channel Account. The
/// Kernel owns the chat flow, attempt counting and locks; the method only
/// starts a challenge and checks an answer.
#[async_trait]
pub trait VerificationMethod: Send + Sync {
    /// Stable id stored in `verification_challenge.method`, e.g. `email_otp`.
    fn id(&self) -> &'static str;
    /// Opens a challenge for `claim` (e.g. emails a code).
    async fn start(&self, services: &Services, claim: &StudentClaim) -> Result<Challenge>;
    /// `Ok(None)`: wrong answer (the Kernel counts attempts).
    async fn check(
        &self,
        services: &Services,
        challenge: &Challenge,
        answer: &str,
    ) -> Result<Option<StudentId>>;
}

/// Email OTP: a code sent to the roster email, compared when it comes back.
/// Parameters default to the spec (6 digits, 5 minutes, 3 wrong attempts,
/// 3 resends per hour, 30-minute lock) and come from `[otp]` in the TOML.
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

/// The challenge/answer flow ships with the chat flow ticket (#19); until
/// then both calls fail cleanly instead of panicking.
fn not_yet() -> Error {
    Error::Platform("email OTP verification ships with the chat flow (#19)".into())
}

#[async_trait]
impl VerificationMethod for EmailOtp {
    fn id(&self) -> &'static str {
        "email_otp"
    }
    async fn start(&self, _: &Services, _: &StudentClaim) -> Result<Challenge> {
        Err(not_yet())
    }
    async fn check(&self, _: &Services, _: &Challenge, _: &str) -> Result<Option<StudentId>> {
        Err(not_yet())
    }
}

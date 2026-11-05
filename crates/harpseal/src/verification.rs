//! Data for Verification Methods. Each design defines its own trait and
//! implements it for `EmailOtp`.

/// What the Student typed to claim an identity (student ID, email, ...).
#[derive(Debug, Clone)]
pub struct StudentClaim(pub String);

/// An open Verification attempt, stored by the Kernel.
#[derive(Debug, Clone)]
pub struct Challenge {
    pub id: String,
    /// Catalog key of the prompt shown to the Student, e.g. "otp.sent".
    pub prompt_key: &'static str,
}

/// Email OTP: send a code to the roster email, compare what comes back.
#[derive(Debug, Clone)]
pub struct EmailOtp {
    pub code_digits: u8,
    pub ttl_secs: u32,
    pub max_attempts: u8,
}

impl Default for EmailOtp {
    fn default() -> Self {
        Self {
            code_digits: 6,
            ttl_secs: 600,
            max_attempts: 5,
        }
    }
}

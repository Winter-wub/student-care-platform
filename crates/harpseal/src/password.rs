//! Staff password hashing (issue #5 prototype). Every hash is a PHC string,
//! so the algorithm and its parameters travel with the hash and either
//! runtime can verify a hash made by the other.
//!
//! Pure Rust (RustCrypto `argon2` / `pbkdf2`): compiles to wasm32 and costs
//! Worker CPU time. The salt comes from the caller (`Services::random`), so
//! no `getrandom` feature is needed here.

use argon2::password_hash::phc::PasswordHash;
use argon2::password_hash::{PasswordHasher, PasswordVerifier};

use crate::{Error, Result};

/// A hashing scheme with its cost parameters.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scheme {
    /// Argon2id. OWASP minimum: `m_kib = 19456`, `t = 2`, `p = 1`.
    Argon2id { m_kib: u32, t: u32, p: u32 },
    /// PBKDF2-HMAC-SHA256. OWASP: 600 000 rounds.
    Pbkdf2Sha256 { rounds: u32 },
}

impl Scheme {
    /// OWASP Password Storage Cheat Sheet, Argon2id first option.
    pub const ARGON2ID_OWASP: Scheme = Scheme::Argon2id {
        m_kib: 19 * 1024,
        t: 2,
        p: 1,
    };
    /// Same, with 64 MiB of memory.
    pub const ARGON2ID_64M: Scheme = Scheme::Argon2id {
        m_kib: 64 * 1024,
        t: 2,
        p: 1,
    };
    /// OWASP PBKDF2-HMAC-SHA256.
    pub const PBKDF2_OWASP: Scheme = Scheme::Pbkdf2Sha256 { rounds: 600_000 };
}

fn err(e: impl core::fmt::Display) -> Error {
    Error::Invalid(format!("password hash: {e}"))
}

fn argon2(m_kib: u32, t: u32, p: u32) -> Result<argon2::Argon2<'static>> {
    let params = argon2::Params::new(m_kib, t, p, Some(32)).map_err(err)?;
    Ok(argon2::Argon2::new(
        argon2::Algorithm::Argon2id,
        argon2::Version::V0x13,
        params,
    ))
}

/// Hash `password` with a caller-supplied random `salt` (16 bytes is enough).
pub fn hash(scheme: Scheme, password: &str, salt: &[u8]) -> Result<String> {
    let phc = match scheme {
        Scheme::Argon2id { m_kib, t, p } => argon2(m_kib, t, p)?
            .hash_password_with_salt(password.as_bytes(), salt)
            .map_err(err)?,
        Scheme::Pbkdf2Sha256 { rounds } => {
            let params = pbkdf2::Params::new(rounds).map_err(err)?;
            pbkdf2::Pbkdf2::new(pbkdf2::Algorithm::Pbkdf2Sha256, params)
                .hash_password_with_salt(password.as_bytes(), salt)
                .map_err(err)?
        }
    };
    Ok(phc.to_string())
}

/// Checks `password` against a PHC string made by [`hash`]. `Ok(false)` is a
/// wrong password; `Err` is a malformed or unsupported hash.
pub fn verify(phc: &str, password: &str) -> Result<bool> {
    let parsed = PasswordHash::new(phc).map_err(err)?;
    let outcome = match parsed.algorithm.as_str() {
        "argon2id" => argon2::Argon2::default().verify_password(password.as_bytes(), &parsed),
        "pbkdf2-sha256" => pbkdf2::Pbkdf2::default().verify_password(password.as_bytes(), &parsed),
        other => return Err(err(format!("unsupported algorithm {other}"))),
    };
    match outcome {
        Ok(()) => Ok(true),
        Err(argon2::password_hash::Error::PasswordInvalid) => Ok(false),
        Err(e) => Err(err(e)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SALT: &[u8] = b"0123456789abcdef";

    #[test]
    fn argon2id_round_trip_keeps_params_in_phc() {
        let s = Scheme::Argon2id {
            m_kib: 256,
            t: 1,
            p: 1,
        };
        let h = hash(s, "correct horse battery", SALT).unwrap();
        assert!(h.starts_with("$argon2id$v=19$m=256,t=1,p=1$"), "{h}");
        assert!(verify(&h, "correct horse battery").unwrap());
        assert!(!verify(&h, "wrong").unwrap());
    }

    #[test]
    fn pbkdf2_round_trip() {
        let h = hash(Scheme::Pbkdf2Sha256 { rounds: 1000 }, "pw", SALT).unwrap();
        assert!(h.starts_with("$pbkdf2-sha256$i=1000"), "{h}");
        assert!(verify(&h, "pw").unwrap());
        assert!(!verify(&h, "px").unwrap());
    }

    /// Prints the seed Staff hash for `deployments/example/deployment/sql/0003_seed.sql`:
    /// `cargo test -p harpseal --release seed_hash -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn seed_hash() {
        let h = hash(
            Scheme::ARGON2ID_OWASP,
            "seal-bay-proto-2026",
            b"sealbay-seed-salt",
        )
        .unwrap();
        println!("{h}");
    }

    #[test]
    fn unknown_algorithm_is_an_error() {
        assert!(verify("$md5$abc$def", "x").is_err());
    }
}

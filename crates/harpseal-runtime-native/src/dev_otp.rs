//! `HARPSEAL_DEV_FIXED_OTP`: a fixed Student OTP for a local demo, read by
//! the native runtime only (the Cloudflare runtime has no such variable).

/// The environment variable: the code every Student OTP gets in dev mode.
pub const DEV_FIXED_OTP_VAR: &str = "HARPSEAL_DEV_FIXED_OTP";

/// Reads [`DEV_FIXED_OTP_VAR`] through `get` (usually
/// `|k| std::env::var(k).ok()`). Unset or empty answers `Ok(None)`.
///
/// Set while `dev_mode` is off, it is an error: a binary must refuse to
/// start rather than ignore it, so a demo setting copied into a production
/// environment is noticed. Pass the code to
/// `harpseal::EmailOtp::with_dev_fixed_code` and build the Deployment with
/// `.dev(true)`; the builder refuses the fixed code otherwise, and the
/// method checks the number of digits.
pub fn dev_fixed_otp(
    get: impl Fn(&str) -> Option<String>,
    dev_mode: bool,
) -> Result<Option<String>, String> {
    let Some(code) = get(DEV_FIXED_OTP_VAR)
        .map(|v| v.trim().to_owned())
        .filter(|v| !v.is_empty())
    else {
        return Ok(None);
    };
    if !dev_mode {
        return Err(format!(
            "{DEV_FIXED_OTP_VAR} is set but dev mode is off: a fixed Student OTP is for a \
             local demo only; unset it (never set it in production)"
        ));
    }
    Ok(Some(code))
}

/// The startup warning a binary prints when the fixed code is active.
pub const DEV_FIXED_OTP_WARNING: &str = "warning: dev mode: HARPSEAL_DEV_FIXED_OTP is set, \
     every Student OTP is the same fixed code (demo only, never in production)";

#[cfg(test)]
mod tests {
    use super::*;

    fn env(v: Option<&'static str>) -> impl Fn(&str) -> Option<String> {
        move |k| {
            assert_eq!(k, DEV_FIXED_OTP_VAR);
            v.map(str::to_owned)
        }
    }

    #[test]
    fn unset_or_empty_is_none_in_either_mode() {
        for dev in [false, true] {
            assert_eq!(dev_fixed_otp(env(None), dev), Ok(None));
            assert_eq!(dev_fixed_otp(env(Some("  ")), dev), Ok(None));
        }
    }

    #[test]
    fn set_in_dev_mode_is_the_code() {
        assert_eq!(
            dev_fixed_otp(env(Some(" 123456 ")), true),
            Ok(Some("123456".into()))
        );
    }

    #[test]
    fn set_outside_dev_mode_is_refused() {
        let e = dev_fixed_otp(env(Some("123456")), false).unwrap_err();
        assert!(
            e.contains(DEV_FIXED_OTP_VAR) && e.contains("dev mode is off"),
            "{e}"
        );
        assert!(!e.contains("123456"), "the code is not echoed: {e}");
    }
}

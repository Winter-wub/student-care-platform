//! Webhook signature: HMAC-SHA256 of the raw body under the channel
//! secret, base64 in header `x-line-signature`
//! (<https://developers.line.biz/en/reference/messaging-api/#signature-validation>).
//! Checked before the body is parsed.

use harpseal::{Error, Result};
use hmac::{Hmac, KeyInit, Mac};
use sha2::Sha256;

/// The header LINE puts the signature in.
pub const SIGNATURE_HEADER: &str = "x-line-signature";

/// The `x-line-signature` value LINE would send for `body`.
pub fn sign(channel_secret: &str, body: &[u8]) -> String {
    let mut mac = <Hmac<Sha256> as KeyInit>::new_from_slice(channel_secret.as_bytes())
        .expect("HMAC takes any key length");
    mac.update(body);
    base64(&mac.finalize().into_bytes())
}

/// `Ok` when `headers` carry the right signature for `body`. A missing or
/// wrong header, or an empty secret (anyone could sign with it), is
/// `BadSignature`. Never looks inside `body`.
pub(crate) fn verify(channel_secret: &str, headers: &http::HeaderMap, body: &[u8]) -> Result<()> {
    let given = headers
        .get(SIGNATURE_HEADER)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    if channel_secret.is_empty() || given.is_empty() {
        return Err(Error::BadSignature);
    }
    if same(
        given.trim().as_bytes(),
        sign(channel_secret, body).as_bytes(),
    ) {
        Ok(())
    } else {
        Err(Error::BadSignature)
    }
}

/// Compares in time independent of where the first difference is.
fn same(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

/// Standard base64 with padding (RFC 4648 §4).
fn base64(bytes: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for c in bytes.chunks(3) {
        let n = c
            .iter()
            .enumerate()
            .fold(0u32, |a, (i, b)| a | u32::from(*b) << (16 - 8 * i));
        for i in 0..4 {
            if i <= c.len() {
                out.push(T[(n >> (18 - 6 * i) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn headers(sig: &str) -> http::HeaderMap {
        let mut h = http::HeaderMap::new();
        h.insert(SIGNATURE_HEADER, sig.parse().unwrap());
        h
    }

    #[test]
    fn base64_matches_rfc_4648() {
        for (raw, enc) in [("", ""), ("f", "Zg=="), ("fo", "Zm8="), ("foo", "Zm9v")] {
            assert_eq!(base64(raw.as_bytes()), enc);
        }
        assert_eq!(base64(b"foobar"), "Zm9vYmFy");
    }

    #[test]
    fn hmac_matches_rfc_4231_case_2() {
        // HMAC-SHA256("Jefe", "what do ya want for nothing?"), base64.
        assert_eq!(
            sign("Jefe", b"what do ya want for nothing?"),
            "W9zBRr9gdU5qBCQmCJV1x1oAPwidJzmDnexYuWTsOEM="
        );
    }

    #[test]
    fn only_the_right_signature_passes() {
        let body = b"{\"events\":[]}";
        assert!(verify("s", &headers(&sign("s", body)), body).is_ok());
        let wrong = verify("s", &headers(&sign("t", body)), body);
        assert!(matches!(wrong, Err(Error::BadSignature)));
        let missing = verify("s", &http::HeaderMap::new(), body);
        assert!(matches!(missing, Err(Error::BadSignature)));
        let no_secret = verify("", &headers(&sign("", body)), body);
        assert!(matches!(no_secret, Err(Error::BadSignature)));
    }
}

//! UUIDv7 text ids (ADR-0007), built by hand from a millisecond timestamp
//! and 10 random bytes so the Core needs no `uuid` crate (and no `getrandom`
//! feature juggling on wasm32).

/// RFC 9562 UUIDv7: 48-bit big-endian Unix milliseconds, version `7`,
/// 12 random bits, variant `10`, 62 random bits. Negative times clamp to 0.
pub(crate) fn uuid_v7(ms: i64, rand: [u8; 10]) -> String {
    let ms = u64::try_from(ms).unwrap_or(0) & 0xFFFF_FFFF_FFFF;
    let mut b = [0u8; 16];
    b[..6].copy_from_slice(&ms.to_be_bytes()[2..]);
    b[6..].copy_from_slice(&rand);
    b[6] = 0x70 | (b[6] & 0x0F);
    b[8] = 0x80 | (b[8] & 0x3F);
    let h = hex(&b);
    format!(
        "{}-{}-{}-{}-{}",
        &h[..8],
        &h[8..12],
        &h[12..16],
        &h[16..20],
        &h[20..]
    )
}

/// Lowercase hex.
pub(crate) fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push(DIGITS[usize::from(b >> 4)] as char);
        s.push(DIGITS[usize::from(b & 0x0F)] as char);
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layout_matches_rfc_9562() {
        // RFC 9562 appendix A.6 example timestamp: 0x017F22E279B0.
        let id = uuid_v7(0x017F_22E2_79B0, [0xFF; 10]);
        assert_eq!(id, "017f22e2-79b0-7fff-bfff-ffffffffffff");
        assert_eq!(id.len(), 36);
        let zero = uuid_v7(0x017F_22E2_79B0, [0; 10]);
        assert_eq!(zero, "017f22e2-79b0-7000-8000-000000000000");
    }

    #[test]
    fn sorts_by_time_as_text() {
        let a = uuid_v7(1_000, [0xFF; 10]);
        let b = uuid_v7(1_001, [0; 10]);
        assert!(a < b);
        assert_eq!(uuid_v7(-5, [0; 10]), uuid_v7(0, [0; 10]));
    }
}

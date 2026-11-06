//! `X-Line-Retry-Key` values. LINE de-duplicates a push or multicast
//! request sent again with the same key within 24 hours: it answers 409
//! and delivers nothing twice
//! (<https://developers.line.biz/en/docs/messaging-api/retrying-api-request/>).
//!
//! The key must be the same on a retry, so it is derived, not random: a
//! SHA-256 over the Core's batch key ([`harpseal::Channel::send_batch_keyed`]),
//! the recipients and the request's message objects, laid out as a UUID
//! (version and variant bits of a v4, which is the shape LINE asks for).
//! A Job that resends the same Messages to the same account with the same
//! batch key rebuilds the same requests and so the same keys; a different
//! batch key (every interactive send gets a fresh one) never collides.

use serde_json::Value;
use sha2::{Digest, Sha256};

/// The retry key of one request: `batch_key`, the recipients' ids in
/// order and the objects.
pub fn retry_key(batch_key: &str, to: &[&str], objects: &[&Value]) -> String {
    let mut h = Sha256::new();
    h.update(b"harpseal-line-retry-key\0");
    h.update(batch_key.as_bytes());
    for id in to {
        h.update(b"\0");
        h.update(id.as_bytes());
    }
    h.update(b"\0");
    h.update(serde_json::to_vec(objects).unwrap_or_default());
    let d = h.finalize();
    let mut b = [0u8; 16];
    b.copy_from_slice(&d[..16]);
    b[6] = (b[6] & 0x0f) | 0x40;
    b[8] = (b[8] & 0x3f) | 0x80;
    let hex: String = b.iter().map(|x| format!("{x:02x}")).collect();
    format!(
        "{}-{}-{}-{}-{}",
        &hex[..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..]
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn keys_are_uuids_stable_per_request_and_distinct_otherwise() {
        let a = json!({ "type": "text", "text": "ประกาศ" });
        let b = json!({ "type": "text", "text": "ประกาศ 2" });
        let k = retry_key("job-1", &["U1"], &[&a]);
        assert_eq!(k, retry_key("job-1", &["U1"], &[&a]), "a retry reuses it");
        assert_eq!(k.len(), 36);
        let dashes: Vec<usize> = k.match_indices('-').map(|(i, _)| i).collect();
        assert_eq!(dashes, [8, 13, 18, 23]);
        assert_eq!(&k[14..15], "4");
        assert!(matches!(&k[19..20], "8" | "9" | "a" | "b"));
        assert!(k.chars().all(|c| c == '-' || c.is_ascii_hexdigit()));
        assert_ne!(k, retry_key("job-2", &["U1"], &[&a]), "another batch");
        assert_ne!(k, retry_key("job-1", &["U2"], &[&a]), "another account");
        assert_ne!(k, retry_key("job-1", &["U1"], &[&b]), "other objects");
        assert_ne!(k, retry_key("job-1", &["U1"], &[&a, &a]), "more objects");
    }
}

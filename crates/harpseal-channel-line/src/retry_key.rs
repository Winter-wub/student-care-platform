//! `X-Line-Retry-Key` values. LINE de-duplicates a push or multicast
//! request sent again with the same key within 24 hours: it answers 409
//! and delivers nothing twice
//! (<https://developers.line.biz/en/docs/messaging-api/retrying-api-request/>).
//!
//! The key must be the same on a retry, so it is derived, not random: a
//! SHA-256 over the Core's batch key ([`harpseal::Channel::send_batch_keyed`]),
//! the recipients and the request's message objects, laid out as a UUID
//! (version and variant bits of a v4, which is the shape LINE asks for).
//! Two identical requests in one batch (ten identical texts are two
//! identical Pushes of 5; a reply-token fallback equal to a later chunk)
//! must not share a key, or LINE would answer 409 to the second and the
//! Student would silently miss it. So the hash also takes the occurrence
//! number of that (recipients, objects) pair within the batch
//! ([`RetryKeys`]). A Job that resends the same Messages to the same
//! account with the same batch key rebuilds the same requests in the same
//! order and so the same keys; a different batch key (every interactive
//! send gets a fresh one) never collides.

use std::collections::HashMap;

use serde_json::Value;
use sha2::{Digest, Sha256};

/// The retry keys of one batch: [`retry_key`] with each (recipients,
/// objects) pair numbered by how often it already occurred in the batch.
#[derive(Debug)]
pub struct RetryKeys {
    batch_key: String,
    seen: HashMap<[u8; 32], u32>,
}

impl RetryKeys {
    /// Keys for the batch with the Core's `batch_key`.
    pub fn new(batch_key: impl Into<String>) -> Self {
        Self {
            batch_key: batch_key.into(),
            seen: HashMap::new(),
        }
    }

    /// The key of the next request to `to` carrying `objects`.
    pub fn next(&mut self, to: &[&str], objects: &[&Value]) -> String {
        let n = self.seen.entry(digest(to, objects)).or_insert(0);
        let occurrence = *n;
        *n += 1;
        retry_key(&self.batch_key, to, objects, occurrence)
    }
}

/// SHA-256 of the recipients and the objects.
fn digest(to: &[&str], objects: &[&Value]) -> [u8; 32] {
    let mut h = Sha256::new();
    for id in to {
        h.update(id.as_bytes());
        h.update(b"\0");
    }
    h.update(serde_json::to_vec(objects).unwrap_or_default());
    h.finalize().into()
}

/// The retry key of one request: `batch_key`, the recipients' ids in
/// order, the objects and the request's `occurrence` (0 for the first
/// request with these recipients and objects in the batch, 1 for the next).
pub fn retry_key(batch_key: &str, to: &[&str], objects: &[&Value], occurrence: u32) -> String {
    let mut h = Sha256::new();
    h.update(b"harpseal-line-retry-key\0");
    h.update(batch_key.as_bytes());
    h.update(b"\0");
    h.update(occurrence.to_be_bytes());
    h.update(digest(to, objects));
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
        let k = retry_key("job-1", &["U1"], &[&a], 0);
        assert_eq!(
            k,
            retry_key("job-1", &["U1"], &[&a], 0),
            "a retry reuses it"
        );
        assert_eq!(k.len(), 36);
        let dashes: Vec<usize> = k.match_indices('-').map(|(i, _)| i).collect();
        assert_eq!(dashes, [8, 13, 18, 23]);
        assert_eq!(&k[14..15], "4");
        assert!(matches!(&k[19..20], "8" | "9" | "a" | "b"));
        assert!(k.chars().all(|c| c == '-' || c.is_ascii_hexdigit()));
        assert_ne!(k, retry_key("job-2", &["U1"], &[&a], 0), "another batch");
        assert_ne!(k, retry_key("job-1", &["U2"], &[&a], 0), "another account");
        assert_ne!(k, retry_key("job-1", &["U1"], &[&b], 0), "other objects");
        assert_ne!(k, retry_key("job-1", &["U1"], &[&a, &a], 0), "more objects");
    }

    #[test]
    fn identical_requests_in_one_batch_get_different_keys_and_a_retry_the_same() {
        let a = json!({ "type": "text", "text": "ประกาศ" });
        let five = [&a, &a, &a, &a, &a];
        let run = || {
            let mut keys = RetryKeys::new("job-1/U1");
            let first = keys.next(&["U1"], &five);
            let second = keys.next(&["U1"], &five);
            let other = keys.next(&["U2"], &five);
            (first, second, other)
        };
        let (first, second, other) = run();
        assert_ne!(first, second, "two identical Pushes in one batch");
        assert_eq!(first, retry_key("job-1/U1", &["U1"], &five, 0));
        assert_eq!(second, retry_key("job-1/U1", &["U1"], &five, 1));
        assert_eq!(other, retry_key("job-1/U1", &["U2"], &five, 0));
        assert_eq!(
            run(),
            (first, second, other),
            "a full retry reproduces them"
        );
    }
}

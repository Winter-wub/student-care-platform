//! The LINE resume token (#62). LINE packs the parts of several Messages
//! into one request of up to 5 objects, so a batch that stops at a failed
//! request may have sent the first parts of the Message it stopped in
//! together with earlier Messages. Resuming from that Message's first part
//! would build different requests, with different retry keys: LINE would
//! deliver the parts it already took a second time.
//!
//! The token says how to rebuild the failed request and those after it:
//! how many parts of the stopped Message went out (`skip`, with a digest of
//! their objects so a token used with other Messages is ignored) and the
//! occurrence count of every (recipients, objects) pair the batch's
//! accepted Pushes used ([`crate::retry_key::RetryKeys`]). With it the
//! resume renders the Messages from the stopped one, drops the `skip`
//! parts and packs the rest from the failed request on: the same objects,
//! the same retry keys, so LINE answers 409 to a request it took (its
//! answer was lost) and sends the others once.
//!
//! Layout: `line-resume/1;<skip>;<hex digest>;<hex pair digest>=<count>,…`.

use std::collections::HashMap;

use harpseal::ResumeToken;
use serde_json::Value;

use crate::retry_key::digest;

const PREFIX: &str = "line-resume/1";

/// A decoded token.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Resume {
    /// Parts of the first Message that already went out.
    pub(crate) skip: usize,
    /// [`objects_digest`] of those parts.
    pub(crate) skipped: [u8; 32],
    /// Occurrence counts of the accepted Pushes' (recipients, objects).
    pub(crate) seen: HashMap<[u8; 32], u32>,
}

/// SHA-256 of the objects (no recipients).
pub(crate) fn objects_digest(objects: &[&Value]) -> [u8; 32] {
    digest(&[], objects)
}

impl Resume {
    pub(crate) fn encode(&self) -> ResumeToken {
        let mut seen: Vec<String> = self
            .seen
            .iter()
            .filter(|(_, n)| **n > 0)
            .map(|(d, n)| format!("{}={n}", hex(d)))
            .collect();
        seen.sort();
        ResumeToken::new(format!(
            "{PREFIX};{};{};{}",
            self.skip,
            hex(&self.skipped),
            seen.join(",")
        ))
    }

    /// `None` for a token this adapter did not write.
    pub(crate) fn decode(t: &ResumeToken) -> Option<Self> {
        let mut it = t.as_str().split(';');
        if it.next()? != PREFIX {
            return None;
        }
        let skip = it.next()?.parse().ok()?;
        let skipped = unhex(it.next()?)?;
        let mut seen = HashMap::new();
        for pair in it.next()?.split(',').filter(|p| !p.is_empty()) {
            let (d, n) = pair.split_once('=')?;
            seen.insert(unhex(d)?, n.parse().ok()?);
        }
        it.next().is_none().then_some(Self {
            skip,
            skipped,
            seen,
        })
    }
}

fn hex(d: &[u8; 32]) -> String {
    d.iter().map(|b| format!("{b:02x}")).collect()
}

fn unhex(s: &str) -> Option<[u8; 32]> {
    if s.len() != 64 || !s.is_ascii() {
        return None;
    }
    let mut out = [0u8; 32];
    for (i, b) in out.iter_mut().enumerate() {
        *b = u8::from_str_radix(&s[2 * i..2 * i + 2], 16).ok()?;
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_token_round_trips_and_a_foreign_one_is_ignored() {
        let a = json!({ "type": "text", "text": "มหาวิทยาลัยอ่าวแมวน้ำ" });
        let r = Resume {
            skip: 4,
            skipped: objects_digest(&[&a]),
            seen: HashMap::from([(digest(&["U1"], &[&a]), 2), (objects_digest(&[]), 1)]),
        };
        let t = r.encode();
        assert!(t.as_str().starts_with("line-resume/1;4;"));
        assert_eq!(Resume::decode(&t), Some(r));
        for bad in [
            "",
            "telegram;1",
            "line-resume/1;x;00;",
            "line-resume/1;1;zz;",
        ] {
            assert_eq!(Resume::decode(&ResumeToken::new(bad)), None, "{bad}");
        }
    }
}

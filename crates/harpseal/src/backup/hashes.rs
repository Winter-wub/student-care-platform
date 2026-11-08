//! The key hashes a table's check (`settle.rs`) saves for a later step:
//! one object per step that stopped inside the table, holding the hashes
//! of every part that step wrote, so the check reads one object per step
//! the table spanned, not one per part.
//!
//! Layout (integers little-endian): the signature of the indexes hashed
//! (64 hex bytes: SHA-256 of their names and the SQL selected for them,
//! so hashes taken under another schema are never used), then per part:
//! its index (`u32`), its SHA-256 (64 hex bytes), its rows (`u64`) and
//! `rows × indexes` hashes (`u64` each).

use std::collections::BTreeMap;

use super::sha256_hex;
use super::unique::Unique;

/// Bytes of a hex SHA-256.
const SHA: usize = 64;

/// One part's saved hashes.
pub(super) struct Saved {
    pub(super) sha256: String,
    pub(super) hashes: Vec<u64>,
}

/// The signature of `uniq`, as hashed.
pub(super) fn signature(uniq: &[Unique]) -> String {
    let mut text = String::new();
    for u in uniq {
        text.push_str(&u.name);
        text.push('\u{1f}');
        text.push_str(&u.select().join("\u{1e}"));
        text.push('\u{1d}');
    }
    sha256_hex(text.as_bytes())
}

/// The object for `parts` (index, SHA-256, hashes), under signature `sig`.
pub(super) fn encode(sig: &str, parts: &[(usize, &str, &[u64])], width: usize) -> Vec<u8> {
    let mut out = sig.as_bytes().to_vec();
    for (i, sha, hashes) in parts {
        let rows = hashes.len() / width.max(1);
        out.extend((*i as u32).to_le_bytes());
        out.extend(sha.as_bytes());
        out.extend((rows as u64).to_le_bytes());
        out.extend(hashes.iter().flat_map(|h| h.to_le_bytes()));
    }
    out
}

/// The parts in `raw`, by index; empty when the signature differs or the
/// object is cut short (its parts are then read again).
pub(super) fn decode(raw: &[u8], sig: &str, width: usize) -> BTreeMap<usize, Saved> {
    let mut out = BTreeMap::new();
    if raw.get(..SHA) != Some(sig.as_bytes()) {
        return out;
    }
    let mut at = SHA;
    let take = |at: &mut usize, n: usize| {
        let end = at.checked_add(n)?;
        let s = raw.get(*at..end);
        *at = end;
        s
    };
    while at < raw.len() {
        let (Some(i), Some(sha), Some(rows)) =
            (take(&mut at, 4), take(&mut at, SHA), take(&mut at, 8))
        else {
            return BTreeMap::new();
        };
        let i = u32::from_le_bytes(i.try_into().unwrap_or_default()) as usize;
        let rows = u64::from_le_bytes(rows.try_into().unwrap_or_default());
        let n = usize::try_from(rows)
            .unwrap_or(usize::MAX)
            .saturating_mul(width * 8);
        let Some(data) = take(&mut at, n) else {
            return BTreeMap::new();
        };
        let hashes = data
            .as_chunks::<8>()
            .0
            .iter()
            .map(|c| u64::from_le_bytes(*c))
            .collect();
        let sha256 = String::from_utf8_lossy(sha).into_owned();
        out.insert(i, Saved { sha256, hashes });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parts_round_trip_and_a_wrong_signature_or_cut_reads_nothing() {
        let sig = "a".repeat(64);
        let sha = "b".repeat(64);
        let raw = encode(&sig, &[(3, &sha, &[1, 2, 3, 4]), (4, &sha, &[])], 2);
        let got = decode(&raw, &sig, 2);
        assert_eq!(got.keys().copied().collect::<Vec<_>>(), [3, 4]);
        assert_eq!(got[&3].hashes, [1, 2, 3, 4]);
        assert_eq!(got[&3].sha256, sha);
        assert!(decode(&raw, &"c".repeat(64), 2).is_empty());
        assert!(decode(&raw[..raw.len() - 1], &sig, 2).is_empty());
    }
}

//! [`Cursor`]: where the table being dumped stands between two Job
//! steps (`parts.rs`), and the key range each of its parts covers.

use serde::{Deserialize, Serialize};
use serde_json::Value as Json;

use super::ManifestPart;
use super::value::decode;
use crate::port_types::Value;
use crate::{Error, Result};

/// Where the table being dumped stands between two steps.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub(crate) struct Cursor {
    /// Key of the last row of the parts written, as backup JSON values
    /// (`value.rs`); `None` before the first part.
    pub(crate) after: Option<Vec<Json>>,
    /// The parts written so far, in order.
    pub(crate) parts: Vec<ManifestPart>,
    /// The key each part ends at (`None` for an empty part), so a part can
    /// be read again over the same range.
    #[serde(default)]
    pub(crate) ends: Vec<Option<Vec<Json>>>,
    /// Every row is written; what is left is the `UNIQUE` check.
    #[serde(default)]
    pub(crate) written: bool,
    /// Parts to read again before the check (saved before any is
    /// rewritten, so a crash mid-way reads them again).
    #[serde(default)]
    pub(crate) dirty: Vec<usize>,
    /// Times the check found repeated keys and parts were read again.
    #[serde(default)]
    pub(crate) rounds: u32,
    /// Steps that stopped inside the table, each having saved the key
    /// hashes of the parts it wrote ([`sidecar`]).
    #[serde(default)]
    pub(crate) segments: u32,
}

/// Name of the key hashes saved by the `k`-th step to stop in `table`.
pub(crate) fn sidecar(table: &str, k: u32) -> String {
    format!("{table}/{k:05}.keys")
}

/// The keys a part lies between: after, up to (both as values).
pub(super) type KeyRange = (Option<Vec<Value>>, Option<Vec<Value>>);

impl Cursor {
    /// The keys part `i` lies between: after the previous part's end (from
    /// the start for the first), up to its own (`None` for an empty part).
    pub(super) fn range_of(&self, i: usize) -> Result<KeyRange> {
        let end = self
            .ends
            .get(i)
            .ok_or_else(|| Error::Invalid(format!("part {i} has no end key")))?;
        let after = match i {
            0 => None,
            _ => decode_key(&self.ends[i - 1])?,
        };
        Ok((after, decode_key(end)?))
    }
}

pub(super) fn decode_key(key: &Option<Vec<Json>>) -> Result<Option<Vec<Value>>> {
    key.as_ref()
        .map(|k| k.iter().map(decode).collect::<Result<Vec<_>>>())
        .transpose()
}

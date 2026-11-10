//! The parsed rows of an import (#46), so a chunk Job reads its 500 rows
//! instead of re-reading, re-parsing and re-validating the whole file
//! (forty XLSX decompressions for a 20 000-row file).
//!
//! The upload writes the valid records once, as JSON Lines (one record
//! per line; JSON escapes line breaks inside values), to
//! `roster-rows/<import id>.jsonl` next to the original, with their
//! SHA-256 in `counts.rows_sha256`. A chunk checks that hash and parses
//! only its own lines. When the object is missing (a Deployment restored
//! from a backup, which carries only `file_key` objects, or an import
//! uploaded before #46), the chunk falls back to the original file. It is
//! deleted with the original after a year (`prune.rs`).

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value as Json};

use super::store::Import;
use super::validate::Record;
use crate::message::Locale;
use crate::module::JobContext;
use crate::students::StudentFields;
use crate::{Error, Result};

/// The FileStore key of import `id`'s parsed rows.
pub(crate) fn key(id: &str) -> String {
    format!("roster-rows/{id}.jsonl")
}

/// One record as stored.
#[derive(Debug, Serialize, Deserialize)]
struct Line {
    row: usize,
    code: String,
    name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    email: Option<String>,
    locale: String,
    #[serde(default, skip_serializing_if = "Map::is_empty")]
    attributes: Map<String, Json>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    tags: Vec<String>,
    /// Fields whose column the sheet leaves out (#154).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    keep: Vec<String>,
}

/// The records as JSON Lines.
pub(crate) fn encode(records: &[Record]) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    for r in records {
        let line = Line {
            row: r.row,
            code: r.code.clone(),
            name: r.fields.display_name.clone(),
            email: r.fields.email.clone(),
            locale: r.fields.locale.0.clone(),
            attributes: r.fields.attributes.clone(),
            tags: r.tags.clone(),
            keep: r.keep.clone(),
        };
        serde_json::to_writer(&mut out, &line).map_err(|_| Error::Invalid("roster rows".into()))?;
        out.push(b'\n');
    }
    Ok(out)
}

/// Records `[from, to)` of `bytes`, parsing only those lines.
pub(crate) fn slice(bytes: &[u8], from: usize, to: usize) -> Result<Vec<Record>> {
    let bad = || Error::Invalid("the stored roster rows no longer read".into());
    let lines: Vec<&[u8]> = bytes
        .split(|b| *b == b'\n')
        .skip(from)
        .take(to.saturating_sub(from))
        .collect();
    if lines.len() != to.saturating_sub(from) {
        return Err(bad());
    }
    lines
        .into_iter()
        .map(|l| {
            let line: Line = serde_json::from_slice(l).map_err(|_| bad())?;
            let mut fields = StudentFields::new(line.name, line.email, Locale(line.locale))
                .with_status("active");
            fields.attributes = line.attributes;
            Ok(Record {
                row: line.row,
                code: line.code,
                fields,
                tags: line.tags,
                keep: line.keep,
            })
        })
        .collect()
}

/// The records of the chunk starting at `imp.applied_rows`: from the
/// parsed rows when they are stored, else from the original file
/// (re-read, hash-checked, re-parsed and re-validated as a whole, and
/// refused unless it gives the very rows the preview stored).
pub(super) async fn chunk(cx: &JobContext, imp: &Import, size: usize) -> Result<Vec<Record>> {
    let s = &cx.services;
    let from = usize::try_from(imp.applied_rows).unwrap_or(usize::MAX);
    let total = usize::try_from(imp.total_rows).unwrap_or(0);
    let to = from.saturating_add(size).min(total);
    if !imp.counts.rows_sha256.is_empty()
        && let Some(bytes) = s.files.get(&key(&imp.id)).await?
    {
        if crate::auth::sha256_hex(&bytes) != imp.counts.rows_sha256 {
            return Err(Error::Invalid("the stored roster rows changed".into()));
        }
        return slice(&bytes, from, to);
    }
    let bytes = s
        .files
        .get(&imp.file_key)
        .await?
        .ok_or_else(|| Error::Invalid("the stored roster file is missing".into()))?;
    if crate::auth::sha256_hex(&bytes) != imp.counts.sha256 {
        return Err(Error::Invalid("the stored roster file changed".into()));
    }
    let read = super::file::read(&bytes, &imp.file_name, &cx.config.roster)
        .map_err(|_| Error::Invalid("the stored roster file no longer reads".into()))?;
    let taken = super::warnings::Taken::default();
    let valid = super::validate::validate_in(&read.rows, &cx.config, &taken, &read.absent)
        .map_err(|_| Error::Invalid("the stored roster file no longer validates".into()))?;
    // The rows the preview counted, or the import would apply something
    // else (a `[roster]` mapping changed since the upload).
    let same = match imp.counts.rows_sha256.as_str() {
        "" => valid.records.len() == total,
        h => crate::auth::sha256_hex(&encode(&valid.records)?) == h,
    };
    if !same {
        return Err(Error::Invalid(
            "the stored roster file no longer matches the preview".into(),
        ));
    }
    Ok(valid.records.get(from..to).unwrap_or_default().to_vec())
}

#[cfg(test)]
mod tests {
    use super::{encode, slice};
    use crate::kernel::roster::{file, test_config, validate};

    #[test]
    fn the_rows_read_back_as_the_records_slice_by_slice() {
        let config = test_config();
        let bytes = include_bytes!("../../../tests/fixtures/roster/seal_bay_50.xlsx");
        let rows = file::parse(bytes, "seal_bay_50.xlsx", &config.roster).unwrap();
        let records = validate::validate(&rows, &config).unwrap().records;
        let stored = encode(&records).unwrap();
        assert_eq!(slice(&stored, 0, 50).unwrap(), records);
        assert_eq!(slice(&stored, 10, 20).unwrap(), records[10..20].to_vec());
        assert_eq!(slice(&stored, 50, 50).unwrap(), vec![]);
        assert!(slice(&stored, 45, 60).is_err(), "fewer lines than asked");
        assert!(slice(b"{\"row\":1}\n", 0, 1).is_err());
    }
}

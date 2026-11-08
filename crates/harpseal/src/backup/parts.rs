//! Table parts (#88, format 2): [`Writer::table`] dumps one table as
//! ordered parts, each its own entry pinned by SHA-256 and row count, so
//! memory is bounded by the part size and not the table size.
//!
//! A table is read page by page by key (`pages.rs`). A part ends at
//! [`Budget::part_rows`] rows (its pages are cut to fit exactly) or once
//! it passes [`Budget::part_bytes`], or when the step's budget runs out:
//! the part held is written, the key of its last row saved in a
//! [`Cursor`], and the next step reads on after that key. A table always
//! has at least one part (an empty table, one empty part).
//!
//! **Crash safety.** Before it writes a part the saved progress does not
//! name, the writer saves the progress ([`Checkpoint`]), so every entry a
//! backup may have written is named and can be deleted if it is
//! abandoned. A step re-run after a crash starts from the saved cursor and
//! rewrites the same part names.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value as Json};

use super::dump::file_keys;
use super::pages::read_page;
use super::progress::{Budget, Checkpoint, DumpProgress, Spent};
use super::schema::Table;
use super::value::{decode, encode};
use super::{BackupSink, ManifestPart, ManifestTable, PAGE, part_entry, sha256_hex};
use crate::port_types::{Rows, Value};
use crate::{Db, Error, Result};

/// Where the table being dumped stands between two steps.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub(crate) struct Cursor {
    /// Key of the last row of the parts written, as backup JSON values
    /// (`value.rs`); `None` before the first part.
    pub(crate) after: Option<Vec<Json>>,
    /// The parts written so far, in order.
    pub(crate) parts: Vec<ManifestPart>,
}

/// One step's hold on the dump while it writes a table.
pub(super) struct Writer<'a> {
    pub(super) progress: &'a mut DumpProgress,
    pub(super) sink: &'a mut dyn BackupSink,
    pub(super) budget: &'a Budget,
    pub(super) spent: &'a mut Spent,
    pub(super) checkpoint: Option<&'a dyn Checkpoint>,
    /// The part the saved progress names as next.
    pub(super) named: &'a mut Option<String>,
}

/// The part being built.
#[derive(Default)]
struct Held {
    bytes: Vec<u8>,
    rows: u64,
}

impl Writer<'_> {
    /// Dumps `t` (the next table in order) from its cursor on, at `now`.
    /// `true` when the table is finished and pushed to the progress;
    /// `false` when the step's budget ran out first (its cursor saved in
    /// the progress, the part held written).
    pub(super) async fn table(&mut self, db: &dyn Db, t: &Table, now: i64) -> Result<bool> {
        let fk = t.file_key_columns().len() as u64;
        let mut cursor = self.progress.cursor.take().unwrap_or_default();
        let mut after: Option<Vec<Value>> = match &cursor.after {
            Some(key) => Some(key.iter().map(decode).collect::<Result<_>>()?),
            None => None,
        };
        let mut held = Held::default();
        loop {
            // Room for a page, a checkpoint and the part's write, and the
            // file-key reads that end the table.
            let room = self.spent.used + 3 + fk <= self.budget.queries;
            let over = self.spent.over(self.budget, held.bytes.len() as u64);
            if self.spent.worked && (over || !room) {
                if held.rows > 0 {
                    self.flush(t, &mut cursor, &mut held, &after).await?;
                }
                self.progress.cursor = Some(cursor);
                return Ok(false);
            }
            let left = self.budget.part_rows.saturating_sub(held.rows).max(1);
            let limit = usize::try_from(left).unwrap_or(PAGE).min(PAGE);
            let page = read_page(db, t, now, after.as_deref(), limit).await?;
            self.spent.used += 1;
            self.spent.worked = true;
            let n = write_rows(&mut held.bytes, &page.rows, page.skip)?;
            held.rows += n;
            self.spent.rows += n;
            let done = page.rows.len() < limit;
            if page.last.is_some() {
                after = page.last;
            }
            let full = held.rows >= self.budget.part_rows
                || held.bytes.len() as u64 >= self.budget.part_bytes;
            // A table ends with its last rows, or with one empty part.
            if (held.rows > 0 && (done || full)) || (done && cursor.parts.is_empty()) {
                self.flush(t, &mut cursor, &mut held, &after).await?;
            }
            if done {
                break;
            }
        }
        let p = &mut *self.progress;
        for key in file_keys(db, t).await? {
            if let Err(at) = p.keys.binary_search(&key) {
                p.keys.insert(at, key);
            }
        }
        self.spent.used += fk;
        p.tables.push(ManifestTable {
            name: t.name.clone(),
            rows: cursor.parts.iter().map(|p| p.rows).sum(),
            sha256: String::new(),
            parts: cursor.parts,
            columns: t.columns.iter().map(|c| c.name.clone()).collect(),
            sql: t.sql.clone(),
        });
        Ok(true)
    }

    /// Writes the part held as the cursor's next, ending at key `after`;
    /// saves the progress first when it does not name that part yet.
    async fn flush(
        &mut self,
        t: &Table,
        cursor: &mut Cursor,
        held: &mut Held,
        after: &Option<Vec<Value>>,
    ) -> Result<()> {
        let entry = part_entry(&t.name, cursor.parts.len());
        if let Some(ck) = self.checkpoint
            && self.named.as_deref() != Some(entry.as_str())
        {
            self.progress.cursor = Some(cursor.clone());
            ck.save(self.progress).await?;
            self.progress.cursor = None;
            self.spent.used += 1;
            *self.named = Some(entry.clone());
        }
        let held = std::mem::take(held);
        self.sink.write(&entry, &held.bytes).await?;
        self.spent.used += 1;
        let len = held.bytes.len() as u64;
        self.spent.bytes += len;
        self.progress.bytes += len;
        cursor.parts.push(ManifestPart {
            entry,
            rows: held.rows,
            sha256: sha256_hex(&held.bytes),
        });
        cursor.after = after.as_ref().map(|k| k.iter().map(encode).collect());
        Ok(())
    }
}

/// Appends one JSON line per row, skipping the first `skip` columns.
pub(super) fn write_rows(out: &mut Vec<u8>, page: &Rows, skip: usize) -> Result<u64> {
    for row in &page.rows {
        let obj: Map<String, Json> = page
            .columns
            .iter()
            .zip(row)
            .skip(skip)
            .map(|(c, v)| (c.clone(), encode(v)))
            .collect();
        serde_json::to_writer(&mut *out, &Json::Object(obj))
            .map_err(|e| Error::Invalid(format!("backup row: {e}")))?;
        out.push(b'\n');
    }
    Ok(page.rows.len() as u64)
}

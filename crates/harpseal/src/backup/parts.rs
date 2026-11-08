//! Table parts (#88, format 2): [`Writer::table`] dumps one table as
//! ordered parts, each its own entry pinned by SHA-256 and row count, so
//! memory is bounded by the part size and not the table size.
//!
//! A table is read page by page by key (`pages.rs`). A part ends at
//! [`Budget::part_rows`] rows (its pages are cut to fit exactly) or once
//! it passes [`Budget::part_bytes`] (pages shrink to the bytes left once a
//! row's size is known), or when the step's budget runs out: the part held
//! is written, the key of its last row saved in a [`Cursor`], and the next
//! step reads on after that key. A table always has at least one part (an
//! empty table, one empty part). Each part lists the FileStore keys its
//! own rows name, read from the rows as they are written.
//!
//! **Crash safety.** Before it writes a part the saved progress does not
//! name, the writer saves the progress ([`Checkpoint`]), so every entry a
//! backup may have written is named and can be deleted if it is
//! abandoned. A step re-run after a crash starts from the saved cursor and
//! rewrites the same part names.
//!
//! Rows read at different times may break a `UNIQUE` index (`settle.rs`);
//! a part is then read again by its key range ([`Writer::refresh`]).

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value as Json};

use super::pages::{Range, read_page};
use super::progress::{Budget, Checkpoint, DumpProgress, Spent};
use super::schema::Table;
use super::unique::{Unique, secondary};
use super::value::{decode, encode};
use super::{BackupSink, ManifestPart, ManifestTable, PAGE, part_entry, sha256_hex};
use crate::port_types::{Rows, Value};
use crate::{Db, Error, Result};

/// Rows of the first page of a table in a step, before their size is known.
const FIRST_PAGE: u64 = 50;

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
    /// Whether key hashes of its parts were saved for a later step.
    #[serde(default)]
    pub(crate) sidecars: bool,
}

/// One step's hold on the dump while it writes a table.
pub(super) struct Writer<'a> {
    pub(super) progress: &'a mut DumpProgress,
    pub(super) sink: &'a mut dyn BackupSink,
    pub(super) budget: &'a Budget,
    pub(super) spent: &'a mut Spent,
    pub(super) checkpoint: &'a dyn Checkpoint,
    /// The part the saved progress names as next.
    pub(super) named: &'a mut Option<String>,
}

/// The table being written, as this step sees it.
pub(super) struct Tab<'t> {
    pub(super) t: &'t Table,
    pub(super) now: i64,
    /// Its `UNIQUE` indexes other than the primary key.
    pub(super) uniq: Vec<Unique>,
    /// The SQL selected ahead of each row to key it on them.
    extra: Vec<String>,
    /// Values of `extra` per index.
    widths: Vec<usize>,
    /// Key hashes per part (`uniq.len()` per row), for the parts known.
    pub(super) hashes: BTreeMap<usize, Vec<u64>>,
    /// Parts whose hashes this step computed (saved if it stops early).
    pub(super) fresh: BTreeSet<usize>,
    /// Bytes per row of the last page, to size the next one.
    avg_row: u64,
}

/// The part being built.
#[derive(Default)]
struct Held {
    bytes: Vec<u8>,
    rows: u64,
    hashes: Vec<u64>,
    files: BTreeSet<String>,
}

/// Name of the saved key hashes of part `n` of `table`.
pub(crate) fn sidecar(table: &str, n: usize) -> String {
    format!("{table}/{n:05}.keys")
}

fn decode_key(key: &Option<Vec<Json>>) -> Result<Option<Vec<Value>>> {
    key.as_ref()
        .map(|k| k.iter().map(decode).collect::<Result<Vec<_>>>())
        .transpose()
}

impl Writer<'_> {
    /// Dumps `t` (the next table in order) from its cursor on, at `now`,
    /// and checks its keys. `true` when the table is finished and pushed to
    /// the progress; `false` when the step's budget ran out first (its
    /// cursor saved in the progress, the part held written).
    pub(super) async fn table(&mut self, db: &dyn Db, t: &Table, now: i64) -> Result<bool> {
        let uniq = secondary(db, t).await?;
        self.spent.used += 1 + 2 * uniq.len() as u64;
        let extra: Vec<String> = uniq.iter().flat_map(Unique::select).collect();
        let widths = uniq.iter().map(|u| u.select().len()).collect();
        let mut tab = Tab {
            t,
            now,
            uniq,
            extra,
            widths,
            hashes: BTreeMap::new(),
            fresh: BTreeSet::new(),
            avg_row: 0,
        };
        let mut cursor = self.progress.cursor.take().unwrap_or_default();
        if !cursor.written {
            if !self.rows(db, &mut tab, &mut cursor).await? {
                return Ok(false);
            }
            cursor.written = true;
        }
        if !tab.uniq.is_empty() && !self.settle(db, &mut tab, &mut cursor).await? {
            return Ok(false);
        }
        let p = &mut *self.progress;
        if cursor.sidecars {
            p.stale
                .extend((0..cursor.parts.len()).map(|i| sidecar(&t.name, i)));
        }
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

    /// Whether `need` more subrequests (and the saved hashes) fit the step,
    /// or the step has done nothing yet.
    pub(super) fn room(&self, tab: &Tab<'_>, need: u64, held: u64) -> bool {
        let keep = tab.fresh.len() as u64 + 1;
        let fits = self.spent.used + need + keep <= self.budget.queries;
        !self.spent.worked || (fits && !self.spent.over(self.budget, held))
    }

    /// Writes the rows from the cursor on; `false` when the step stopped.
    async fn rows(&mut self, db: &dyn Db, tab: &mut Tab<'_>, cursor: &mut Cursor) -> Result<bool> {
        let mut after = decode_key(&cursor.after)?;
        let mut held = Held::default();
        loop {
            // A page, a checkpoint and the part's write.
            if !self.room(tab, 3, held.bytes.len() as u64) {
                if held.rows > 0 {
                    self.store(tab, cursor, cursor.parts.len(), held, after)
                        .await?;
                }
                self.stop(tab, cursor).await?;
                return Ok(false);
            }
            let b = self.budget;
            let mut left = b.part_rows.saturating_sub(held.rows);
            let bytes_left = b.part_bytes.saturating_sub(held.bytes.len() as u64);
            left = match bytes_left.checked_div(tab.avg_row) {
                Some(rows) => left.min(rows + 1),
                // Rows of unknown size: a short first page measures them.
                None => left.min(FIRST_PAGE),
            };
            let limit = usize::try_from(left.max(1)).unwrap_or(PAGE).min(PAGE);
            let range = Range {
                after: after.as_deref(),
                upto: None,
            };
            let page = read_page(db, tab.t, tab.now, range, limit, &tab.extra).await?;
            self.spent.used += 1;
            self.spent.worked = true;
            let n = self.take(tab, &page.rows, page.skip, &mut held)?;
            self.spent.rows += n;
            let done = page.rows.len() < limit;
            if page.last.is_some() {
                after = page.last;
            }
            let full = held.rows >= b.part_rows || held.bytes.len() as u64 >= b.part_bytes;
            // A table ends with its last rows, or with one empty part.
            if (held.rows > 0 && (done || full)) || (done && cursor.parts.is_empty()) {
                let rows = std::mem::take(&mut held);
                self.store(tab, cursor, cursor.parts.len(), rows, after.clone())
                    .await?;
            }
            if done {
                return Ok(true);
            }
        }
    }

    /// Reads part `i` again over its key range, now, and rewrites it.
    pub(super) async fn refresh(
        &mut self,
        db: &dyn Db,
        tab: &mut Tab<'_>,
        cursor: &mut Cursor,
        i: usize,
    ) -> Result<()> {
        let end = cursor.ends.get(i).cloned().ok_or_else(|| {
            Error::Invalid(format!(
                "backup of table {}: part {i} has no end key",
                tab.t.name
            ))
        })?;
        let upto = decode_key(&end)?;
        let after = match i {
            0 => None,
            _ => decode_key(&cursor.ends[i - 1])?,
        };
        let mut held = Held::default();
        let mut from = after;
        while upto.is_some() {
            let range = Range {
                after: from.as_deref(),
                upto: upto.as_deref(),
            };
            let page = read_page(db, tab.t, tab.now, range, PAGE, &tab.extra).await?;
            self.spent.used += 1;
            self.take(tab, &page.rows, page.skip, &mut held)?;
            if page.rows.len() < PAGE {
                break;
            }
            from = page.last;
        }
        self.store(tab, cursor, i, held, upto).await
    }

    /// Adds a page's rows to `held`: NDJSON, key hashes, file keys.
    fn take(
        &mut self,
        tab: &mut Tab<'_>,
        page: &Rows,
        skip: usize,
        held: &mut Held,
    ) -> Result<u64> {
        let files: Vec<usize> = tab
            .t
            .file_key_columns()
            .iter()
            .filter_map(|c| page.columns.iter().skip(skip).position(|n| n == c))
            .map(|i| i + skip)
            .collect();
        for row in &page.rows {
            let mut at = 0;
            for (u, w) in tab.uniq.iter().zip(&tab.widths) {
                held.hashes
                    .push(u.hash(row.get(at..at + w).unwrap_or_default()));
                at += w;
            }
            for &i in &files {
                if let Some(Value::Text(k)) = row.get(i)
                    && !k.is_empty()
                {
                    held.files.insert(k.clone());
                }
            }
        }
        let before = held.bytes.len();
        let n = write_rows(&mut held.bytes, page, skip)?;
        held.rows += n;
        if let Some(avg) = ((held.bytes.len() - before) as u64).checked_div(n) {
            tab.avg_row = avg.max(1);
        }
        Ok(n)
    }

    /// Writes `held` as part `i` (the next, or one being read again),
    /// ending at key `end`; saves the progress first when it does not name
    /// a new part yet.
    async fn store(
        &mut self,
        tab: &mut Tab<'_>,
        cursor: &mut Cursor,
        i: usize,
        held: Held,
        end: Option<Vec<Value>>,
    ) -> Result<()> {
        let entry = part_entry(&tab.t.name, i);
        let new = i == cursor.parts.len();
        if new && self.named.as_deref() != Some(entry.as_str()) {
            self.progress.cursor = Some(cursor.clone());
            self.checkpoint.save(self.progress).await?;
            self.progress.cursor = None;
            self.spent.used += 1;
            *self.named = Some(entry.clone());
        }
        self.sink.write(&entry, &held.bytes).await?;
        self.spent.used += 1;
        let len = held.bytes.len() as u64;
        self.spent.bytes += len;
        let part = ManifestPart {
            entry,
            rows: held.rows,
            sha256: sha256_hex(&held.bytes),
            bytes: len,
            files: held.files.into_iter().collect(),
        };
        if new {
            self.progress.bytes += len;
            cursor.parts.push(part);
            cursor
                .ends
                .push(end.as_ref().map(|k| k.iter().map(encode).collect()));
            cursor.after = cursor.ends[i].clone();
        } else {
            self.progress.bytes =
                self.progress.bytes - cursor.parts[i].bytes.min(self.progress.bytes) + len;
            cursor.parts[i] = part;
        }
        if !tab.uniq.is_empty() {
            tab.hashes.insert(i, held.hashes);
            tab.fresh.insert(i);
        }
        Ok(())
    }

    /// Ends the step inside the table: saves the key hashes this step
    /// computed (for the check at the table's end) and hands the cursor
    /// to the progress.
    pub(super) async fn stop(&mut self, tab: &Tab<'_>, cursor: &mut Cursor) -> Result<()> {
        for &i in &tab.fresh {
            let (Some(part), Some(hashes)) = (cursor.parts.get(i), tab.hashes.get(&i)) else {
                continue;
            };
            let mut bytes = part.sha256.as_bytes().to_vec();
            bytes.extend(hashes.iter().flat_map(|h| h.to_le_bytes()));
            self.checkpoint
                .put_check(&sidecar(&tab.t.name, i), bytes)
                .await?;
            self.spent.used += 1;
            cursor.sidecars = true;
        }
        self.progress.cursor = Some(std::mem::take(cursor));
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

//! Table parts (#88, format 2): [`Writer::table`] dumps one table as
//! ordered parts, each its own entry pinned by SHA-256 and row count, so
//! the rows held are bounded by the part size and not the table size. The
//! key check at the table's end (`settle.rs`) is the one exception: about
//! 16 bytes per row per `UNIQUE` index at its peak.
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

use serde_json::{Map, Value as Json};

use super::cursor::decode_key;
pub(crate) use super::cursor::{Cursor, sidecar};

use super::pages::{Range, read_page};
use super::progress::{Budget, Checkpoint, DumpProgress, Spent};
use super::schema::Table;
use super::unique::{Unique, secondary};
use super::value::encode;
use super::{BackupSink, ManifestPart, ManifestTable, PAGE, part_entry, sha256_hex};
use crate::port_types::{Rows, Value};
use crate::{Db, Error, Result};

/// Rows of the first page of a table in a step, before their size is known.
const FIRST_PAGE: u64 = 50;

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
    /// [`super::hashes::signature`] of `uniq`.
    pub(super) sig: String,
    /// Keep each row's whole key too (`settle.rs`, before it fails).
    pub(super) full: bool,
    /// Whole keys per part (`uniq.len()` per row), kept while `full`.
    pub(super) keys: BTreeMap<usize, Vec<Option<String>>>,
    /// Bytes per row of the last page, to size the next one.
    avg_row: u64,
}

/// The part being built.
#[derive(Default)]
struct Held {
    bytes: Vec<u8>,
    rows: u64,
    hashes: Vec<u64>,
    keys: Vec<Option<String>>,
    files: BTreeSet<String>,
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
        let sig = super::hashes::signature(&uniq);
        let mut tab = Tab {
            t,
            now,
            uniq,
            extra,
            widths,
            hashes: BTreeMap::new(),
            fresh: BTreeSet::new(),
            sig,
            full: false,
            keys: BTreeMap::new(),
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
        if !tab.uniq.is_empty() {
            // Every step's hashes, and one a step may have saved before
            // it died.
            p.stale
                .extend((0..=cursor.segments).map(|k| sidecar(&t.name, k)));
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
        // The hashes saved if the step stops here, and one to spare.
        let keep = if tab.uniq.is_empty() { 1 } else { 2 };
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
        let (after, upto) = cursor
            .range_of(i)
            .map_err(|e| Error::Invalid(format!("backup of table {}: {e}", tab.t.name)))?;
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
        // The branding files (#114) are named by three Kernel settings, not
        // by a column: only those, with a value of their own shape, are
        // followed. Any other `setting` value is data, never a file key.
        let col = |c: &str| page.columns.iter().skip(skip).position(|n| n == c);
        let setting = match (tab.t.name.as_str(), col("key"), col("locale"), col("value")) {
            ("setting", Some(k), Some(l), Some(v)) => Some((k + skip, l + skip, v + skip)),
            _ => None,
        };
        for row in &page.rows {
            if let Some((k, l, v)) = setting
                && let (Some(Value::Text(key)), Some(Value::Text(locale)), Some(Value::Text(file))) =
                    (row.get(k), row.get(l), row.get(v))
                && crate::kernel::branding::names_file(key, locale, file)
            {
                held.files.insert(file.clone());
            }
            let mut at = 0;
            for (u, w) in tab.uniq.iter().zip(&tab.widths) {
                let values = row.get(at..at + w).unwrap_or_default();
                held.hashes.push(u.hash(values));
                if tab.full {
                    held.keys.push(u.key(values));
                }
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
        if tab.full {
            tab.keys.insert(i, held.keys);
        }
        Ok(())
    }

    /// Ends the step inside the table: saves the key hashes this step
    /// computed (for the check at the table's end) and hands the cursor
    /// to the progress.
    pub(super) async fn stop(&mut self, tab: &Tab<'_>, cursor: &mut Cursor) -> Result<()> {
        let parts: Vec<(usize, &str, &[u64])> = tab
            .fresh
            .iter()
            .filter_map(|&i| {
                let part = cursor.parts.get(i)?;
                Some((i, part.sha256.as_str(), tab.hashes.get(&i)?.as_slice()))
            })
            .collect();
        if !parts.is_empty() {
            let bytes = super::hashes::encode(&tab.sig, &parts, tab.uniq.len());
            let name = sidecar(&tab.t.name, cursor.segments);
            self.checkpoint.put_check(&name, bytes).await?;
            self.spent.used += 1;
            cursor.segments += 1;
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

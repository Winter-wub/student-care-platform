//! [`Writer::settle`]: a table's rows must insert cleanly, though a dump
//! reads them page by page and, on Workers, over steps minutes apart with
//! no snapshot (#88 review). A row read early may change before a later
//! page is read: a Link revoked and a new one made for the same Student
//! leaves two live Links in the backup, which `link_live_student_channel`
//! (a partial `UNIQUE` index) would refuse on restore.
//!
//! So when a table's rows are all written, its keys on every `UNIQUE`
//! index other than the primary key (which keyset paging never repeats)
//! are checked across all its parts. Each row's key on each index is
//! computed by SQLite as the row is read (the index's own terms and
//! `WHERE`, `unique.rs`) and kept as an 8-byte hash: in memory for the
//! parts of this step, and saved through the [`Checkpoint`] by a step that
//! stops inside the table, one object per step (`hashes.rs`, each part
//! with its SHA-256, so hashes that do not match their part are never
//! used). A part whose hashes are missing or do not match is read again;
//! so is every part holding a repeated key, over its key range, now. The
//! database holds no two rows with one key, so reading again resolves a
//! repeat a change made while the table was read; the check runs again
//! until it finds none.
//!
//! After three rounds that still find repeats, the parts are read once
//! more keeping each row's whole key: a repeat of hashes only (a
//! collision) passes; a real one fails the step, naming the table and the
//! index. The Job retries the step, and fails the run on its last attempt;
//! the next scheduled backup starts over. A backup marked done therefore
//! inserts cleanly.
//!
//! A part read again must stay near the part size: when its key range now
//! holds more than twice [`Budget::part_rows`] rows or about twice
//! [`Budget::part_bytes`] (many rows written into it between steps, say a
//! roster import under keys that are not in time order), the step fails
//! naming the table instead of holding it in memory.
//!
//! Before any part is rewritten, the parts to read again are saved in the
//! progress, so a step that dies mid-way reads them again.
//!
//! **Memory.** The check holds 8 bytes per row per index, and while it
//! looks for repeats a sorted copy of one index's: about 16 bytes per row
//! per index at its peak, so a table of about 4 to 5 million rows per
//! `UNIQUE` index fills a 128 MB Worker.
//!
//! [`Checkpoint`]: super::progress::Checkpoint
//! [`Budget::part_rows`]: super::progress::Budget::part_rows
//! [`Budget::part_bytes`]: super::progress::Budget::part_bytes

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use super::PAGE;
use super::pages::{Range, count_range};
use super::parts::{Cursor, Tab, Writer, sidecar};
use crate::{Db, Error, Result};

/// Rounds of reading parts again before the whole keys are compared.
const ROUNDS: u32 = 3;

/// A key repeated across parts.
struct Repeat {
    /// Parts holding a row with it.
    parts: Vec<usize>,
    /// Index (in `Tab::uniq`) and its name.
    index: usize,
    name: String,
    /// Rows with a repeated hash.
    rows: usize,
    /// The hashes repeated.
    dup: HashSet<u64>,
}

impl Writer<'_> {
    /// Checks `tab`'s keys across its parts, reading parts again until no
    /// key repeats; `false` when the step's budget ran out first (the
    /// cursor saved, `written` set).
    pub(super) async fn settle(
        &mut self,
        db: &dyn Db,
        tab: &mut Tab<'_>,
        cursor: &mut Cursor,
    ) -> Result<bool> {
        loop {
            while let Some(&i) = cursor.dirty.first() {
                if !self.room(tab, 1, 0) {
                    self.stop(tab, cursor).await?;
                    return Ok(false);
                }
                self.spent.worked = true;
                let rows = self.grown(db, tab, cursor, i).await?;
                if !self.room(tab, rows / PAGE as u64 + 3, 0) {
                    self.stop(tab, cursor).await?;
                    return Ok(false);
                }
                self.refresh(db, tab, cursor, i).await?;
                cursor.dirty.remove(0);
            }
            if !self.room(tab, u64::from(cursor.segments), 0) {
                self.stop(tab, cursor).await?;
                return Ok(false);
            }
            let bad = self.load(tab, cursor).await?;
            if !bad.is_empty() {
                self.mark(cursor, bad).await?;
                continue;
            }
            let Some(r) = repeats(tab) else {
                return Ok(true);
            };
            if tab.full {
                if !confirmed(tab, &r) {
                    // Hashes alike, keys not: nothing repeats on restore.
                    return Ok(true);
                }
                return Err(Error::Invalid(format!(
                    "table {} kept changing while it was backed up: {} rows still repeat a \
                     key of {} after reading them again {ROUNDS} times, so the backup would \
                     not restore; this step fails and is retried, and if its last attempt \
                     fails too the run fails and the next scheduled backup starts over",
                    tab.t.name, r.rows, r.name
                )));
            }
            if cursor.rounds >= ROUNDS {
                // Once more, keeping the whole keys to compare.
                tab.full = true;
            } else {
                cursor.rounds += 1;
            }
            self.mark(cursor, r.parts).await?;
        }
    }

    /// Rows part `i`'s key range holds now; fails naming the table when a
    /// re-read would hold far more than a part.
    async fn grown(
        &mut self,
        db: &dyn Db,
        tab: &Tab<'_>,
        cursor: &Cursor,
        i: usize,
    ) -> Result<u64> {
        let (after, upto) = cursor.range_of(i)?;
        if upto.is_none() {
            return Ok(0);
        }
        let range = Range {
            after: after.as_deref(),
            upto: upto.as_deref(),
        };
        let rows = count_range(db, tab.t, tab.now, range).await?;
        self.spent.used += 1;
        let part = &cursor.parts[i];
        let per_row = part.bytes.checked_div(part.rows).unwrap_or(0);
        let bytes = rows.saturating_mul(per_row);
        let b = self.budget;
        if rows > 2 * b.part_rows.max(PAGE as u64) || bytes > 2 * b.part_bytes {
            return Err(Error::Invalid(format!(
                "table {}: part {} must be read again (a key repeated across the backup), \
                 but {rows} rows (about {} MB) were written into its key range while it was \
                 backed up, more than twice a part; rather than hold them in memory this \
                 step fails and is retried, and if its last attempt fails too the run fails \
                 and the next scheduled backup starts over",
                tab.t.name,
                i + 1,
                bytes.div_ceil(1024 * 1024)
            )));
        }
        Ok(rows)
    }

    /// Loads the saved hashes of the parts this step did not write, one
    /// object per step that stopped in the table; returns the parts whose
    /// hashes are missing or do not match (they must be read again).
    async fn load(&mut self, tab: &mut Tab<'_>, cursor: &Cursor) -> Result<Vec<usize>> {
        let width = tab.uniq.len();
        let mut saved = BTreeMap::new();
        let wanted = (0..cursor.parts.len()).any(|i| !tab.hashes.contains_key(&i));
        for k in (0..cursor.segments).filter(|_| wanted) {
            self.spent.used += 1;
            if let Some(raw) = self.checkpoint.get_check(&sidecar(&tab.t.name, k)).await? {
                // A later step's hashes of a part replace an earlier's.
                saved.extend(super::hashes::decode(&raw, &tab.sig, width));
            }
        }
        let mut bad = Vec::new();
        for (i, part) in cursor.parts.iter().enumerate() {
            if tab.hashes.contains_key(&i) {
                continue;
            }
            let fits = |len: usize| len as u64 == part.rows * width as u64;
            match saved.remove(&i) {
                _ if part.rows == 0 => {
                    tab.hashes.insert(i, Vec::new());
                }
                Some(s) if s.sha256 == part.sha256 && fits(s.hashes.len()) => {
                    tab.hashes.insert(i, s.hashes);
                }
                _ => bad.push(i),
            }
        }
        Ok(bad)
    }

    /// Saves `parts` as to be read again, before any is rewritten.
    async fn mark(&mut self, cursor: &mut Cursor, parts: Vec<usize>) -> Result<()> {
        cursor.dirty = parts;
        self.progress.cursor = Some(cursor.clone());
        self.checkpoint.save(self.progress).await?;
        self.progress.cursor = None;
        self.spent.used += 1;
        Ok(())
    }
}

/// The first index whose hashes repeat across the parts, if any.
fn repeats(tab: &Tab<'_>) -> Option<Repeat> {
    let width = tab.uniq.len();
    for (j, u) in tab.uniq.iter().enumerate() {
        let column = |h: &Vec<u64>| h.iter().skip(j).step_by(width).copied().collect::<Vec<_>>();
        let mut all: Vec<u64> = tab
            .hashes
            .values()
            .flat_map(column)
            .filter(|h| *h != 0)
            .collect();
        all.sort_unstable();
        let dup: HashSet<u64> = all
            .windows(2)
            .filter(|w| w[0] == w[1])
            .map(|w| w[0])
            .collect();
        drop(all);
        if dup.is_empty() {
            continue;
        }
        let mut parts = BTreeSet::new();
        let mut rows = 0;
        for (&i, h) in &tab.hashes {
            let n = column(h).iter().filter(|x| dup.contains(x)).count();
            if n > 0 {
                parts.insert(i);
                rows += n;
            }
        }
        let name = u.name.clone();
        let parts = parts.into_iter().collect();
        return Some(Repeat {
            parts,
            index: j,
            name,
            rows,
            dup,
        });
    }
    None
}

/// Whether the rows behind `r`'s repeated hashes really share a key (their
/// whole keys, kept while `tab.full`); a part without whole keys counts
/// as a real repeat.
fn confirmed(tab: &Tab<'_>, r: &Repeat) -> bool {
    let width = tab.uniq.len();
    let mut seen: HashMap<&str, usize> = HashMap::new();
    for &i in &r.parts {
        let (Some(h), Some(k)) = (tab.hashes.get(&i), tab.keys.get(&i)) else {
            return true;
        };
        for (row, hash) in h.iter().skip(r.index).step_by(width).enumerate() {
            if !r.dup.contains(hash) {
                continue;
            }
            if let Some(Some(key)) = k.get(row * width + r.index) {
                *seen.entry(key.as_str()).or_default() += 1;
            }
        }
    }
    seen.values().any(|n| *n > 1)
}

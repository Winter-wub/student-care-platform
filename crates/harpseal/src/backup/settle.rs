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
//! parts of this step, saved through the [`Checkpoint`] for a later step
//! (with the part's SHA-256, so a hash list that does not match its part
//! is never used). A part whose hashes are missing or do not match is read
//! again; so is every part holding a repeated key, over its key range, now.
//! The database holds no two rows with one key, so reading again resolves
//! a repeat a change made while the table was read; the check runs again
//! until it finds none. Three such rounds that still find repeats fail the
//! step with the table and index named; the Job retries the step later and
//! fails the run on its last attempt, so a backup marked done always
//! inserts cleanly.
//!
//! Before any part is rewritten, the parts to read again are saved in the
//! progress, so a step that dies mid-way reads them again. The hashes of a
//! table take 8 bytes per row per index, about twice over while checked.

use std::collections::{BTreeSet, HashSet};

use super::PAGE;
use super::parts::{Cursor, Tab, Writer, sidecar};
use crate::{Db, Error, Result};

/// Rounds of reading parts again before the step fails.
const ROUNDS: u32 = 3;

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
                let rows = cursor.parts.get(i).map_or(0, |p| p.rows);
                if !self.room(tab, rows / PAGE as u64 + 3, 0) {
                    self.stop(tab, cursor).await?;
                    return Ok(false);
                }
                self.spent.worked = true;
                self.refresh(db, tab, cursor, i).await?;
                cursor.dirty.remove(0);
            }
            let missing: Vec<usize> = (0..cursor.parts.len())
                .filter(|i| !tab.hashes.contains_key(i))
                .collect();
            if !self.room(tab, missing.len() as u64, 0) {
                self.stop(tab, cursor).await?;
                return Ok(false);
            }
            let mut bad = Vec::new();
            for i in missing {
                if !self.load(tab, cursor, i).await? {
                    bad.push(i);
                }
            }
            if !bad.is_empty() {
                self.mark(cursor, bad).await?;
                continue;
            }
            let Some((parts, index, rows)) = repeats(tab) else {
                return Ok(true);
            };
            if cursor.rounds >= ROUNDS {
                return Err(Error::Invalid(format!(
                    "table {} kept changing while it was backed up: {rows} rows still repeat \
                     a key of {index} after reading them again {ROUNDS} times, so the backup \
                     would not restore; the backup is tried again",
                    tab.t.name
                )));
            }
            cursor.rounds += 1;
            self.mark(cursor, parts).await?;
        }
    }

    /// Loads the saved hashes of part `i`; `false` when they are missing
    /// or do not match the part (it must be read again).
    async fn load(&mut self, tab: &mut Tab<'_>, cursor: &Cursor, i: usize) -> Result<bool> {
        let part = &cursor.parts[i];
        let width = tab.uniq.len();
        if part.rows == 0 {
            tab.hashes.insert(i, Vec::new());
            return Ok(true);
        }
        self.spent.used += 1;
        let Some(raw) = self.checkpoint.get_check(&sidecar(&tab.t.name, i)).await? else {
            return Ok(false);
        };
        let head = part.sha256.len();
        let fits = raw.len() == head + part.rows as usize * width * 8;
        if !fits || raw.get(..head) != Some(part.sha256.as_bytes()) {
            return Ok(false);
        }
        let hashes = raw[head..]
            .as_chunks::<8>()
            .0
            .iter()
            .map(|c| u64::from_le_bytes(*c))
            .collect();
        tab.hashes.insert(i, hashes);
        Ok(true)
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

/// The parts holding a key that repeats on some index, that index's name
/// and how many rows repeat it; `None` when no key repeats.
fn repeats(tab: &Tab<'_>) -> Option<(Vec<usize>, String, usize)> {
    let width = tab.uniq.len();
    let mut parts = BTreeSet::new();
    let mut first: Option<(String, usize)> = None;
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
        let mut rows = 0;
        for (&i, h) in &tab.hashes {
            let n = column(h).iter().filter(|x| dup.contains(x)).count();
            if n > 0 {
                parts.insert(i);
                rows += n;
            }
        }
        first.get_or_insert((u.name.clone(), rows));
    }
    let (name, rows) = first?;
    Some((parts.into_iter().collect(), name, rows))
}

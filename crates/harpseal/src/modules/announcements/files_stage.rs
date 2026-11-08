//! Saving announcement files (#109; PDFs #113 the same way): the objects
//! are put first, then one database batch lists them, then
//! [`Staged::settle`] deletes only the objects no row lists.

use super::files::{FileEdits, Stored};
use super::gate::GATED;
use super::store::text;
use crate::port_types::Statement;
use crate::{Result, Services};

/// Bytes of a token (128 bits).
const TOKEN_BYTES: usize = 16;

/// One new file, staged: its object is in the FileStore; the bytes are
/// not kept.
#[derive(Debug)]
struct NewRow {
    kind: &'static str,
    n: u8,
    content_type: &'static str,
    size: Option<(u32, u32)>,
    name: String,
    bytes: i64,
    token: String,
    key: String,
}

/// A save's files, staged: the objects already put, the statements that
/// list them, and the keys to settle once the save is over.
#[derive(Debug, Default)]
pub(crate) struct Staged {
    rows: Vec<NewRow>,
    /// (kind, slot) of every slot the save empties or fills.
    emptied: Vec<(&'static str, u8)>,
    /// Supersede instead of delete: the announcement was sent, and Cards
    /// already delivered show the old files. Superseded rows are not
    /// capped: only Staff holding `announcements.send` make them, one per
    /// slot per audited edit, each file within its kind's limit; a cap
    /// would block the edit that removes a file showing personal data.
    keep_old: bool,
    /// Objects of the files this save deletes (not sent yet only).
    old_keys: Vec<String>,
}

/// Puts the objects of `edits` (the bytes moved, not copied) for
/// announcement `id`, whose current files are `existing` (every kind).
/// With `sent`, the files removed or replaced stay listed and served,
/// superseded ([`Staged::statements`]).
pub(crate) async fn stage(
    s: &Services,
    id: &str,
    edits: FileEdits,
    existing: &[Stored],
    sent: bool,
) -> Result<Staged> {
    let mut staged = Staged {
        keep_old: sent,
        ..Staged::default()
    };
    let touched = edits
        .remove
        .iter()
        .copied()
        .chain(edits.put.iter().map(|p| (p.kind, p.n)));
    for (kind, n) in touched {
        if staged.emptied.contains(&(kind, n)) {
            continue;
        }
        staged.emptied.push((kind, n));
        let old = existing.iter().find(|f| f.kind == kind && f.n == n);
        if let Some(old) = old.filter(|_| !sent) {
            staged.old_keys.push(old.file_key.clone());
        }
    }
    for f in edits.put {
        let token = s.random_hex(TOKEN_BYTES)?;
        let key = format!("announcement-file/{id}/{token}.{}", f.ext);
        let bytes = f.bytes.len() as i64;
        if let Err(e) = s.files.put(&key, f.bytes, f.content_type).await {
            staged.settle(s).await;
            return Err(e);
        }
        staged.rows.push(NewRow {
            kind: f.kind,
            n: f.n,
            content_type: f.content_type,
            size: f.size,
            name: f.name,
            bytes,
            token,
            key,
        });
    }
    Ok(staged)
}

impl Staged {
    /// The statements: empty every slot touched (delete the row, or mark it
    /// superseded once sent), then list the new files; each gated on
    /// `gate` when given (an edit's compare-and-set).
    pub(crate) fn statements(&self, id: &str, gate: Option<&str>, now: i64) -> Vec<Statement> {
        let cond = |sql: &str| match gate {
            Some(_) => format!("{sql} AND {GATED}"),
            None => sql.to_string(),
        };
        let with_gate = |st: Statement| match gate {
            Some(g) => st.bind(g),
            None => st,
        };
        let empty = if self.keep_old {
            format!("UPDATE announcement_file SET superseded_at = {now} WHERE")
        } else {
            "DELETE FROM announcement_file WHERE".to_string()
        };
        // A delete only while nothing was delivered: whatever the page saw,
        // the batch itself refuses to delete a sent announcement's file.
        let unsent = if self.keep_old {
            ""
        } else {
            " AND EXISTS (SELECT 1 FROM announcement WHERE id = ? \
             AND status IN ('draft', 'scheduled'))"
        };
        let mut out = Vec::new();
        for (kind, n) in &self.emptied {
            let sql = cond(&format!(
                "{empty} announcement_id = ? AND kind = ? AND n = ? \
                 AND superseded_at IS NULL{unsent}"
            ));
            let st = Statement::new(sql).bind(id).bind(*kind).bind(i64::from(*n));
            let st = if self.keep_old { st } else { st.bind(id) };
            out.push(with_gate(st));
        }
        for r in &self.rows {
            let sql = cond(
                "INSERT INTO announcement_file (announcement_id, kind, n, content_type, \
                 size_bytes, width, height, file_name, token, file_key, created_at) \
                 SELECT ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ? WHERE 1",
            );
            let (w, h) = r.size.unzip();
            let st = Statement::new(sql)
                .bind(id)
                .bind(r.kind)
                .bind(i64::from(r.n))
                .bind(r.content_type)
                .bind(r.bytes)
                .bind(w.map(i64::from))
                .bind(h.map(i64::from))
                .bind(r.name.as_str())
                .bind(r.token.as_str())
                .bind(r.key.as_str())
                .bind(now);
            out.push(with_gate(st));
        }
        out
    }

    /// After the save, whatever its result (a batch may commit and still
    /// report an error): re-reads which of the keys it touched are listed,
    /// and deletes the objects of those that are not (new files of a save
    /// that did not commit, files a committed save deleted). When the
    /// re-read fails, nothing is deleted: an unlisted object is never
    /// served, a deleted listed one would be lost.
    pub(crate) async fn settle(&self, s: &Services) {
        let keys: Vec<&str> = self
            .rows
            .iter()
            .map(|r| r.key.as_str())
            .chain(self.old_keys.iter().map(String::as_str))
            .collect();
        for chunk in keys.chunks(90) {
            let marks = vec!["?"; chunk.len()].join(", ");
            let sql = format!("SELECT file_key FROM announcement_file WHERE file_key IN ({marks})");
            let st = chunk.iter().fold(Statement::new(sql), |st, k| st.bind(*k));
            let Ok(rows) = s.db.query(st).await else {
                return;
            };
            let listed: Vec<String> = (0..rows.len())
                .map(|i| text(&rows, i, "file_key"))
                .collect();
            for key in chunk.iter().filter(|k| !listed.iter().any(|l| l == *k)) {
                // A failed delete leaves bytes only: the key is unlisted.
                let _ = s.files.delete(key).await;
            }
        }
    }
}

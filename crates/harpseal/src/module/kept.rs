//! Keeping a Module's upload until the Staff member confirms (#74), like
//! the roster's stored file: [`SheetUpload::keep`] puts the file in the
//! FileStore under `module-upload/<module>/<id>.<csv|xlsx>` and records
//! it (Kernel table `module_upload`: Module, Staff member, file name,
//! kind, size, SHA-256, expiry), with the audit row `module.upload`, in
//! one batch. [`KeptUpload::open`] reads it back for the confirm (or the
//! Job the confirm enqueues) and refuses a file whose hash changed.
//! [`KeptUpload::discard`] deletes it (cancel, or the Job once done).
//! Each tick deletes up to [`PER_TICK`] uploads past their expiry
//! ([`sweep_uploads`]), so a preview nobody confirmed leaves nothing
//! behind.

use serde_json::json;

use super::upload::{SheetKind, SheetUpload, UPLOAD_KEEP_MS};
use crate::audit::{self, Actor, RequestCtx, Target};
use crate::port_types::{Rows, Statement, Value};
use crate::sheet::{SheetError, Workbook};
use crate::{CurrentStaff, Error, Result, Services};

/// Expired uploads one tick deletes, at most.
const PER_TICK: i64 = 50;

/// A kept upload, read back with its bytes.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct KeptUpload {
    /// Its id (a UUIDv7), to put in the confirm and cancel URLs.
    pub id: String,
    /// The Module that kept it.
    pub module: String,
    /// The Staff member who uploaded it (`staff.id`).
    pub staff_id: String,
    /// The file name, as [`SheetUpload::file_name`].
    pub file_name: String,
    /// What the file holds.
    pub kind: SheetKind,
    /// SHA-256 of the bytes, lowercase hex.
    pub sha256: String,
    /// When it was kept, ms.
    pub created_at: i64,
    /// When the tick deletes it, ms.
    pub expires_at: i64,
    /// The file.
    pub bytes: Vec<u8>,
}

impl SheetUpload {
    /// Keeps the file for a later confirm, for [`UPLOAD_KEEP_MS`] (24
    /// hours): stored in the FileStore under a key of `module` (the
    /// Module's [`super::Module::name`]), recorded with its SHA-256, and
    /// audited (`module.upload`, by `staff`). Only `module` can open or
    /// discard it.
    pub async fn keep(
        &self,
        s: &Services,
        module: &str,
        staff: &CurrentStaff,
        ctx: &RequestCtx,
    ) -> Result<KeptUpload> {
        if module.is_empty() || module.contains('/') {
            return Err(Error::Invalid(format!("module name {module:?}")));
        }
        let id = s.new_id()?;
        let key = format!("module-upload/{module}/{id}.{}", self.kind.as_str());
        crate::check_file_key(&key)?;
        let now = s.now_ms();
        let kept = KeptUpload {
            id: id.clone(),
            module: module.to_string(),
            staff_id: staff.id.clone(),
            file_name: self.file_name.clone(),
            kind: self.kind,
            sha256: crate::auth::sha256_hex(&self.bytes),
            created_at: now,
            expires_at: now + UPLOAD_KEEP_MS,
            bytes: self.bytes.clone(),
        };
        let size = i64::try_from(self.bytes.len()).unwrap_or(i64::MAX);
        let insert = Statement::new(
            "INSERT INTO module_upload (id, module, staff_id, file_key, file_name, kind, \
             size_bytes, sha256, created_at, expires_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(id.as_str())
        .bind(module)
        .bind(staff.id.as_str())
        .bind(key.as_str())
        .bind(kept.file_name.as_str())
        .bind(kept.kind.as_str())
        .bind(size)
        .bind(kept.sha256.as_str())
        .bind(now)
        .bind(kept.expires_at);
        let after = json!({
            "module": module,
            "file_name": kept.file_name,
            "kind": kept.kind.as_str(),
            "size_bytes": size,
            "sha256": kept.sha256,
        });
        let target = Target::new("module_upload", id.as_str());
        let audit = audit::entry(
            s,
            &Actor::staff(staff),
            "module.upload",
            Some(&target),
            None,
            Some(after),
            ctx,
        )?;
        let content_type = self.kind.content_type();
        s.files.put(&key, self.bytes.clone(), content_type).await?;
        if let Err(e) = s.db.batch(vec![insert, audit]).await {
            // Nothing points at the object: do not leave it behind.
            let _ = s.files.delete(&key).await;
            return Err(e);
        }
        Ok(kept)
    }
}

impl KeptUpload {
    /// Upload `id` of `module`, with its bytes: `None` when there is no
    /// such upload for this Module, it expired, or its file is gone.
    /// [`Error::Invalid`] when the file's SHA-256 is no longer the one
    /// recorded at upload: never import it.
    pub async fn open(s: &Services, module: &str, id: &str) -> Result<Option<Self>> {
        let q = Statement::new(
            "SELECT id, module, staff_id, file_key, file_name, kind, sha256, created_at, \
             expires_at FROM module_upload WHERE id = ? AND module = ? AND expires_at > ?",
        )
        .bind(id)
        .bind(module)
        .bind(s.now_ms());
        let rows = s.db.query(q).await?;
        if rows.is_empty() {
            return Ok(None);
        }
        let text = |c: &str| text(&rows, 0, c);
        let int = |c: &str| rows.get(0, c).and_then(Value::as_i64).unwrap_or(0);
        let kind = SheetKind::parse(&text("kind"))
            .ok_or_else(|| Error::Invalid("module upload kind".into()))?;
        let Some(bytes) = s.files.get(&text("file_key")).await? else {
            return Ok(None);
        };
        let sha256 = text("sha256");
        if crate::auth::sha256_hex(&bytes) != sha256 {
            return Err(Error::Invalid(format!(
                "module upload {id}: the kept file changed since it was uploaded"
            )));
        }
        Ok(Some(Self {
            id: text("id"),
            module: text("module"),
            staff_id: text("staff_id"),
            file_name: text("file_name"),
            kind,
            sha256,
            created_at: int("created_at"),
            expires_at: int("expires_at"),
            bytes,
        }))
    }

    /// Deletes upload `id` of `module` (its file, then its record);
    /// `false` when there was none. Call it on cancel, and from the import
    /// Job once it is done.
    pub async fn discard(s: &Services, module: &str, id: &str) -> Result<bool> {
        let q = Statement::new("SELECT file_key FROM module_upload WHERE id = ? AND module = ?")
            .bind(id)
            .bind(module);
        let rows = s.db.query(q).await?;
        if rows.is_empty() {
            return Ok(false);
        }
        delete(s, id, &text(&rows, 0, "file_key")).await?;
        Ok(true)
    }

    /// Reads the file with the roster import's reader ([`crate::sheet`]).
    pub fn workbook(&self) -> Result<Workbook, SheetError> {
        Workbook::open(&self.bytes, &self.file_name)
    }
}

fn text(rows: &Rows, i: usize, c: &str) -> String {
    rows.get(i, c)
        .and_then(Value::as_text)
        .unwrap_or_default()
        .to_string()
}

/// The object first: a failed delete of the row is retried by the sweep,
/// and deleting a missing object is not an error.
async fn delete(s: &Services, id: &str, key: &str) -> Result<()> {
    s.files.delete(key).await?;
    let stmt = Statement::new("DELETE FROM module_upload WHERE id = ?").bind(id);
    s.db.execute(stmt).await.map(|_| ())
}

/// Deletes the uploads expired at `now`: how many, and the ones that
/// failed (each is retried on the next tick).
pub(crate) async fn sweep_uploads(s: &Services, now: i64) -> Result<(u64, Vec<String>)> {
    let due = Statement::new(
        "SELECT id, file_key FROM module_upload WHERE expires_at <= ? \
         ORDER BY expires_at, id LIMIT ?",
    )
    .bind(now)
    .bind(PER_TICK);
    let rows = s.db.query(due).await?;
    let (mut count, mut errors) = (0, Vec::new());
    for i in 0..rows.len() {
        let id = text(&rows, i, "id");
        match delete(s, &id, &text(&rows, i, "file_key")).await {
            Ok(()) => count += 1,
            Err(e) => errors.push(format!("module upload {id}: {e}")),
        }
    }
    Ok((count, errors))
}

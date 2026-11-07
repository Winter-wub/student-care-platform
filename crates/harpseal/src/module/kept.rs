//! Keeping a Module's upload until the Staff member confirms (#74), like
//! the roster's stored file. Back-office handlers go through [`Uploads`],
//! an extractor bound to the Module that registered the route and to the
//! signed-in Staff member:
//!
//! - [`Uploads::keep`] records the upload (Kernel table `module_upload`:
//!   Module, uploader, file name, kind, size, SHA-256, expiry 24 h ahead)
//!   with the audit row `module.upload`, then puts the file in the
//!   FileStore under `module-upload/<module>/<id>.<csv|xlsx>`. The row goes
//!   first, so a crash leaves a row the sweep deletes, never an orphan file.
//! - [`Uploads::open`] reads it back and refuses a file whose hash changed.
//! - [`Uploads::confirm`] pins it for the import Job (`module.upload.confirm`):
//!   its expiry moves [`CONFIRMED_KEEP_MS`] ahead, so a confirm a minute
//!   before the 24 hours run out still reaches the Job.
//! - [`Uploads::discard`] deletes it (`module.upload.cancel`).
//!
//! By default only the uploader sees an upload; [`Uploads::shared_with`]
//! opens every upload of the Module to Staff holding a Permission the
//! Module chooses. The Job reads a confirmed upload with
//! [`KeptUpload::open_confirmed`] and deletes it with
//! [`KeptUpload::finish`]. Each tick deletes up to [`PER_TICK`] uploads
//! past their expiry ([`sweep_uploads`]). Backups leave them out.

use axum::extract::FromRequestParts;
use axum::http::StatusCode;
use axum::http::request::Parts;
use serde_json::json;

use super::AppState;
use super::upload::{SheetKind, SheetUpload, UPLOAD_KEEP_MS};
use crate::audit::{self, Actor, RequestCtx, Target};
use crate::port_types::{Rows, Statement, Value};
use crate::sheet::{SheetError, Workbook};
use crate::{CurrentStaff, Error, Result, Services};

/// Expired uploads one tick deletes, at most.
const PER_TICK: i64 = 50;
/// How long a confirmed upload waits for its Job: a week, well past the
/// Job's retries.
pub const CONFIRMED_KEEP_MS: i64 = 7 * 86_400_000;

const COLUMNS: &str = "id, module, staff_id, object_key, file_name, kind, sha256, \
                       created_at, expires_at, confirmed_at";

/// The Module whose back-office route is running (set by the guard).
#[derive(Debug, Clone, Copy)]
pub(crate) struct RouteModule(pub(crate) &'static str);

/// A kept upload, read back with its bytes.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct KeptUpload {
    /// Its id (a UUIDv7), for the preview, confirm and cancel URLs.
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
    /// When it was confirmed, ms ([`Uploads::confirm`]).
    pub confirmed_at: Option<i64>,
    /// The file.
    pub bytes: Vec<u8>,
}

impl KeptUpload {
    /// Reads the file with the roster import's reader ([`crate::sheet`]).
    pub fn workbook(&self) -> Result<Workbook, SheetError> {
        Workbook::open(&self.bytes, &self.file_name)
    }

    /// For the import Job: confirmed upload `id` of `module` (the Module's
    /// [`super::Module::name`]), with its bytes. `None` when there is none:
    /// not confirmed, deleted after the import ([`KeptUpload::finish`]) or
    /// expired. A Job must not read `None` as "done" unless its own rows
    /// show the import happened. [`Error::Invalid`] when the hash changed.
    pub async fn open_confirmed(s: &Services, module: &str, id: &str) -> Result<Option<Self>> {
        let q = Statement::new(format!(
            "SELECT {COLUMNS} FROM module_upload WHERE id = ? AND module = ? \
             AND confirmed_at IS NOT NULL AND expires_at > ?"
        ))
        .bind(id)
        .bind(module)
        .bind(s.now_ms());
        read(s, q).await
    }

    /// For the import Job, once the import is written: deletes confirmed
    /// upload `id` of `module`; `false` when there was none.
    pub async fn finish(s: &Services, module: &str, id: &str) -> Result<bool> {
        let q = Statement::new(
            "SELECT object_key FROM module_upload WHERE id = ? AND module = ? \
             AND confirmed_at IS NOT NULL",
        )
        .bind(id)
        .bind(module);
        let rows = s.db.query(q).await?;
        if rows.is_empty() {
            return Ok(false);
        }
        delete(s, id, &text(&rows, 0, "object_key")).await?;
        Ok(true)
    }
}

/// The kept uploads of the Module whose back-office route is running, as
/// the signed-in Staff member sees them. Extract it in a handler of a
/// route registered with [`super::ModuleRegistrar::sheet_upload`],
/// `back_office` or `back_office_page`; elsewhere it answers 500.
#[derive(Debug, Clone)]
pub struct Uploads {
    services: Services,
    module: &'static str,
    staff: CurrentStaff,
    shared: bool,
}

impl FromRequestParts<AppState> for Uploads {
    type Rejection = StatusCode;

    async fn from_request_parts(parts: &mut Parts, s: &AppState) -> Result<Self, StatusCode> {
        let module = parts.extensions.get::<RouteModule>().copied();
        let staff = parts.extensions.get::<CurrentStaff>().cloned();
        let (Some(RouteModule(module)), Some(staff)) = (module, staff) else {
            return Err(StatusCode::INTERNAL_SERVER_ERROR);
        };
        Ok(Self {
            services: s.services.clone(),
            module,
            staff,
            shared: false,
        })
    }
}

impl Uploads {
    /// The Module the uploads belong to.
    pub fn module(&self) -> &'static str {
        self.module
    }

    /// Opens every upload of the Module (not only the Staff member's own)
    /// when the Staff member holds `permission`, e.g. a supervisor's.
    #[must_use]
    pub fn shared_with(mut self, permission: &str) -> Self {
        self.shared = self.staff.can(permission);
        self
    }

    /// The condition that limits a query to what this Staff member sees.
    fn visible(&self, stmt: Statement) -> Statement {
        let sql = format!("{} AND module = ? AND (staff_id = ? OR ?)", stmt.sql);
        let mut out = stmt;
        out.sql = sql.into();
        out.bind(self.module)
            .bind(self.staff.id.as_str())
            .bind(self.shared)
    }

    /// Keeps `up` for a later confirm, for [`UPLOAD_KEEP_MS`] (24 hours),
    /// and returns its id. Audited `module.upload`.
    pub async fn keep(&self, up: SheetUpload, ctx: &RequestCtx) -> Result<String> {
        let s = &self.services;
        let id = s.new_id()?;
        let key = format!("module-upload/{}/{id}.{}", self.module, up.kind.as_str());
        crate::check_file_key(&key)?;
        let now = s.now_ms();
        let sha256 = crate::auth::sha256_hex(&up.bytes);
        let size = i64::try_from(up.bytes.len()).unwrap_or(i64::MAX);
        let insert = Statement::new(
            "INSERT INTO module_upload (id, module, staff_id, object_key, file_name, kind, \
             size_bytes, sha256, created_at, expires_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(id.as_str())
        .bind(self.module)
        .bind(self.staff.id.as_str())
        .bind(key.as_str())
        .bind(up.file_name.as_str())
        .bind(up.kind.as_str())
        .bind(size)
        .bind(sha256.as_str())
        .bind(now)
        .bind(now + UPLOAD_KEEP_MS);
        let after = json!({
            "module": self.module,
            "file_name": up.file_name,
            "kind": up.kind.as_str(),
            "size_bytes": size,
            "sha256": sha256,
        });
        let audit = self.audit("module.upload", &id, after, ctx)?;
        // The row first: a crash before the object leaves a row the sweep
        // deletes, never an object nothing points at.
        s.db.batch(vec![insert, audit]).await?;
        if let Err(e) = s.files.put(&key, up.bytes, up.kind.content_type()).await {
            let _ = delete(s, &id, &key).await;
            return Err(e);
        }
        Ok(id)
    }

    /// Upload `id` with its bytes: `None` when it is not this Module's, not
    /// this Staff member's (unless [`Uploads::shared_with`]), deleted or
    /// expired. [`Error::Invalid`] when the file's SHA-256 is no longer the
    /// one recorded at upload: never import it.
    pub async fn open(&self, id: &str) -> Result<Option<KeptUpload>> {
        let q = Statement::new(format!(
            "SELECT {COLUMNS} FROM module_upload WHERE id = ? AND expires_at > ?"
        ))
        .bind(id)
        .bind(self.services.now_ms());
        read(&self.services, self.visible(q)).await
    }

    /// Pins upload `id` for the import Job: marks it confirmed and moves
    /// its expiry [`CONFIRMED_KEEP_MS`] ahead, audited
    /// `module.upload.confirm`. `true` when this call confirmed it; `false`
    /// when it was already confirmed or is not visible ([`Uploads::open`]).
    /// Enqueue the Job after it (with `enqueue_unique` on the id, so a
    /// retry after `false` is harmless).
    pub async fn confirm(&self, id: &str, ctx: &RequestCtx) -> Result<bool> {
        let s = &self.services;
        let now = s.now_ms();
        let mut cond = self.visible(
            Statement::new(
                "SELECT 1 FROM module_upload WHERE id = ? AND expires_at > ? \
                 AND confirmed_at IS NULL",
            )
            .bind(id)
            .bind(now),
        );
        cond.sql = format!("EXISTS ({})", cond.sql).into();
        let actor = Actor::staff(&self.staff);
        let target = Target::new("module_upload", id);
        let after = json!({ "module": self.module, "keep_until": now + CONFIRMED_KEEP_MS });
        let (audit, gate) = audit::entry_if(
            s,
            &actor,
            "module.upload.confirm",
            Some(&target),
            None,
            Some(after),
            ctx,
            cond,
        )?;
        let pin = Statement::new(
            "UPDATE module_upload SET confirmed_at = ?, confirmed_by = ?, expires_at = ? \
             WHERE id = ? AND EXISTS (SELECT 1 FROM audit_log WHERE id = ?)",
        )
        .bind(now)
        .bind(self.staff.id.as_str())
        .bind(now + CONFIRMED_KEEP_MS)
        .bind(id)
        .bind(gate);
        let done = s.db.batch(vec![audit, pin]).await?;
        Ok(done.get(1).is_some_and(|r| r.rows_affected == 1))
    }

    /// Deletes upload `id` (its file, then its record), audited
    /// `module.upload.cancel`; `false` when it is not visible
    /// ([`Uploads::open`], expiry aside).
    pub async fn discard(&self, id: &str, ctx: &RequestCtx) -> Result<bool> {
        let q =
            Statement::new("SELECT object_key, file_name FROM module_upload WHERE id = ?").bind(id);
        let rows = self.services.db.query(self.visible(q)).await?;
        if rows.is_empty() {
            return Ok(false);
        }
        delete(&self.services, id, &text(&rows, 0, "object_key")).await?;
        let after = json!({ "module": self.module, "file_name": text(&rows, 0, "file_name") });
        let audit = self.audit("module.upload.cancel", id, after, ctx)?;
        self.services.db.execute(audit).await?;
        Ok(true)
    }

    fn audit(
        &self,
        action: &str,
        id: &str,
        after: serde_json::Value,
        ctx: &RequestCtx,
    ) -> Result<Statement> {
        let target = Target::new("module_upload", id);
        let actor = Actor::staff(&self.staff);
        audit::entry(
            &self.services,
            &actor,
            action,
            Some(&target),
            None,
            Some(after),
            ctx,
        )
    }
}

/// The one upload `q` selects ([`COLUMNS`]), with its bytes checked.
async fn read(s: &Services, q: Statement) -> Result<Option<KeptUpload>> {
    let rows = s.db.query(q).await?;
    if rows.is_empty() {
        return Ok(None);
    }
    let text = |c: &str| text(&rows, 0, c);
    let int = |c: &str| rows.get(0, c).and_then(Value::as_i64);
    let kind = SheetKind::parse(&text("kind"))
        .ok_or_else(|| Error::Invalid("module upload kind".into()))?;
    let Some(bytes) = s.files.get(&text("object_key")).await? else {
        return Ok(None);
    };
    let sha256 = text("sha256");
    if crate::auth::sha256_hex(&bytes) != sha256 {
        return Err(Error::Invalid(format!(
            "module upload {}: the kept file changed since it was uploaded",
            text("id")
        )));
    }
    Ok(Some(KeptUpload {
        id: text("id"),
        module: text("module"),
        staff_id: text("staff_id"),
        file_name: text("file_name"),
        kind,
        sha256,
        created_at: int("created_at").unwrap_or(0),
        expires_at: int("expires_at").unwrap_or(0),
        confirmed_at: int("confirmed_at"),
        bytes,
    }))
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
        "SELECT id, object_key FROM module_upload WHERE expires_at <= ? \
         ORDER BY expires_at, id LIMIT ?",
    )
    .bind(now)
    .bind(PER_TICK);
    let rows = s.db.query(due).await?;
    let (mut count, mut errors) = (0, Vec::new());
    for i in 0..rows.len() {
        let id = text(&rows, i, "id");
        match delete(s, &id, &text(&rows, i, "object_key")).await {
            Ok(()) => count += 1,
            Err(e) => errors.push(format!("module upload {id}: {e}")),
        }
    }
    Ok((count, errors))
}

//! Retention: after a successful backup, the runs started more than
//! `[backup] retention_days` ago (done or failed, never the newest done
//! one, never a running one) are deleted with their FileStore objects,
//! each audited `backup.prune`. At most [`LIMIT`] per backup, so a long
//! backlog drains over nights.

use serde_json::json;

use super::store::{self, Run};
use super::{DAY_MS, progress_key};
use crate::audit::{self, Actor, RequestCtx, Target};
use crate::backup::progress::DumpProgress;
use crate::backup::{FileStoreBackup, MANIFEST, Manifest};
use crate::manifest::Job;
use crate::port_types::Statement;
use crate::{Deployment, Error, Result, Services};

/// Runs deleted per successful backup.
const LIMIT: i64 = 20;

/// Deletes the expired runs; returns how many.
pub(super) async fn expired(d: &Deployment, job: &Job) -> Result<usize> {
    let s = d.services();
    let days = i64::from(d.config().backup.retention_days);
    let before = s.now_ms() - days * DAY_MS;
    let runs = store::expired(s, before, LIMIT).await?;
    for r in &runs {
        let backup = FileStoreBackup::new(s.files.clone(), r.prefix.clone());
        backup.delete(&entries(s, r).await?).await?;
        s.files.delete(&progress_key(&r.prefix)).await?;
        let gone = Statement::new("DELETE FROM backup_run WHERE id = ? AND state <> 'running'")
            .bind(r.id.as_str());
        let before = json!({
            "prefix": r.prefix, "state": r.state, "started_at": r.started_at,
            "bytes": r.bytes,
        });
        let target = Target::new("backup_run", r.id.as_str());
        let audit = audit::entry(
            s,
            &Actor::Job(job.id.clone()),
            "backup.prune",
            Some(&target),
            Some(before),
            Some(json!({ "retention_days": days })),
            &RequestCtx::default(),
        )?;
        s.db.batch(vec![gone, audit]).await?;
    }
    Ok(runs.len())
}

/// Every entry `r` wrote or may have written: from its manifest when it
/// finished, else from its saved progress, else none (a run that saved no
/// progress wrote no entry).
pub(super) async fn entries(s: &Services, r: &Run) -> Result<Vec<String>> {
    let backup = FileStoreBackup::new(s.files.clone(), r.prefix.clone());
    if s.files.get(&backup.key(MANIFEST)).await?.is_some() {
        let (m, _) = Manifest::read(&backup).await?;
        return Ok(m.entries());
    }
    let Some(raw) = s.files.get(&progress_key(&r.prefix)).await? else {
        return Ok(Vec::new());
    };
    let p: DumpProgress = serde_json::from_slice(&raw)
        .map_err(|e| Error::Invalid(format!("backup progress of {}: {e}", r.prefix)))?;
    Ok(p.entries())
}

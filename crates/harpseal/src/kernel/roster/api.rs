//! The roster import from code: the same path the back office uses
//! (`POST /admin/roster/upload`, then `…/{id}/confirm`), for a CLI or a
//! seed. The acting Staff member must be active and hold
//! `kernel.roster.import`, as on the pages; every write is audited as
//! that Staff member.

use super::{preview, service};
use crate::audit::{Actor, RequestCtx};
use crate::{Deployment, Result};

/// The Permission both functions require.
pub const IMPORT_PERMISSION: &str = "kernel.roster.import";

/// What [`upload`] did.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Uploaded {
    /// Read, validated and previewed: the import id, ready for [`confirm`].
    Previewed(String),
    /// Refused with a report (state `failed`): the import id.
    Refused(String),
    /// Another import holds the slot: its id.
    Busy(String),
}

/// What [`confirm`] did.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Confirmed {
    /// Queued: the Job `kernel.roster.import` applies it on the next ticks.
    Queued,
    /// No longer `previewed` (confirmed, cancelled or replaced): its state.
    NotPreviewed(String),
    /// No such import.
    Missing,
}

/// Stores `bytes` as `file_name` (CSV or XLSX, told apart by content and
/// name), reads it with `[roster]`, validates every row and previews the
/// change. `mode` is `full` (Students missing from the file become
/// `left`) or `upsert`. Audit `roster.import.upload`.
pub async fn upload(
    d: &Deployment,
    file_name: &str,
    bytes: Vec<u8>,
    mode: &str,
    actor: &Actor,
    ctx: &RequestCtx,
) -> Result<Uploaded> {
    let staff = crate::auth::roles::require(d.services(), actor, IMPORT_PERMISSION).await?;
    Ok(
        match preview::upload(d, file_name, bytes, mode, &staff, ctx).await? {
            preview::Uploaded::Previewed(id) => Uploaded::Previewed(id),
            preview::Uploaded::Refused(id) => Uploaded::Refused(id),
            preview::Uploaded::Busy(b) => Uploaded::Busy(b.id),
        },
    )
}

/// Confirms previewed import `id`: queues the Job that applies it. Audit
/// `roster.import`.
pub async fn confirm(
    d: &Deployment,
    id: &str,
    actor: &Actor,
    ctx: &RequestCtx,
) -> Result<Confirmed> {
    crate::auth::roles::require(d.services(), actor, IMPORT_PERMISSION).await?;
    Ok(
        match service::confirm(d.services(), d.jobs(), id, actor, ctx).await? {
            service::Outcome::Done => Confirmed::Queued,
            service::Outcome::NotPreviewed(s) => Confirmed::NotPreviewed(s),
            service::Outcome::Missing => Confirmed::Missing,
        },
    )
}

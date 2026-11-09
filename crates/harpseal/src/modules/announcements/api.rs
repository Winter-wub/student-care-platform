//! Announcements from code: the same path the back office uses
//! (`POST /admin/announcements/new`, then "ส่ง" on the detail page), for a
//! CLI, a seed, or a Module's own page or Job (pass
//! [`crate::PageState::deployment`] or [`crate::JobContext::deployment`]).
//! The acting Staff member must be active and hold
//! `announcements.send`, as on the pages; every write is audited as that
//! Staff member.

use super::form::Fields;
use super::service;
use crate::audience::Audience;
use crate::audit::{Actor, RequestCtx};
use crate::{Deployment, Error, Result};

use super::view::{input_date, input_datetime};

/// The Permission both functions require.
pub const SEND_PERMISSION: &str = "announcements.send";

/// What Staff fill in on the create form.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct NewAnnouncement {
    /// Thai title (required, at most 100 characters).
    pub title_th: String,
    /// Thai body (required, at most 2 000 characters).
    pub body_th: String,
    /// English title and body: both or neither.
    pub en: Option<(String, String)>,
    /// Who receives it (checked against `[roster]`).
    pub audience: Audience,
    /// Ask Students to acknowledge ("รับทราบ").
    pub require_ack: bool,
    /// Expires at the end of the local day containing this instant (ms).
    pub expires_at: Option<i64>,
    /// Send at this instant (ms, to the minute) instead of at once.
    pub send_at: Option<i64>,
}

impl NewAnnouncement {
    /// A Thai announcement to everyone, sent at once, without ack.
    pub fn new(title_th: impl Into<String>, body_th: impl Into<String>) -> Self {
        Self {
            title_th: title_th.into(),
            body_th: body_th.into(),
            en: None,
            audience: Audience::everyone(),
            require_ack: false,
            expires_at: None,
            send_at: None,
        }
    }

    fn fields(&self, offset: i32) -> Fields {
        let (title_en, body_en) = self.en.clone().unwrap_or_default();
        Fields {
            title_th: self.title_th.clone(),
            body_th: self.body_th.clone(),
            title_en,
            body_en,
            require_ack: self.require_ack,
            expires: self
                .expires_at
                .map(|e| input_date(e + 1, offset))
                .unwrap_or_default(),
            when: if self.send_at.is_some() {
                "later"
            } else {
                "now"
            }
            .into(),
            send_at: self
                .send_at
                .map(|t| input_datetime(t, offset))
                .unwrap_or_default(),
            attrs: self.audience.attributes.clone(),
            tags: self.audience.tags.join(","),
            students: self.audience.students.join("\n"),
            unreachable: self.audience.include_unreachable,
            // An `Audience` from code is explicit, everyone included.
            scope: super::form::scope_of(&self.audience).into(),
            notify: false,
            version: None,
            submit: String::new(),
        }
    }
}

/// Creates a draft with the create form's checks; returns its id (version
/// 0). Audit `announcement.create`.
pub async fn create_draft(
    d: &Deployment,
    new: &NewAnnouncement,
    actor: &Actor,
    ctx: &RequestCtx,
) -> Result<String> {
    crate::auth::roles::require(d.services(), actor, SEND_PERMISSION).await?;
    let offset = d.config().utc_offset_minutes().unwrap_or(0);
    let now = d.services().now_ms();
    let draft = new
        .fields(offset)
        .draft(&d.config().roster, offset, now)
        .map_err(|errors| {
            let keys: Vec<&str> = errors.iter().map(|(_, k)| *k).collect();
            Error::Invalid(format!("announcement refused: {}", keys.join(", ")))
        })?;
    service::create(d, draft, actor, ctx).await
}

/// Confirms draft `id` seen at `version` (0 for a new draft): scheduled
/// when its `send_at` is still ahead, otherwise sending at once (the Jobs
/// run on the next ticks). Returns whether it was scheduled. Audit
/// `announcement.send` or `announcement.schedule`; a stale `version` is
/// `Error::Conflict`.
///
/// The caller owns the confirmation: there is no review step, no recount
/// of a send to everyone (#120) and no refusal of a send that reaches
/// nobody (#135) here, only the Permission and the version. A page that calls this should show the reach first, as
/// `/admin/announcements/{id}/review` does.
pub async fn confirm(
    d: &Deployment,
    id: &str,
    version: i64,
    actor: &Actor,
    ctx: &RequestCtx,
) -> Result<bool> {
    crate::auth::roles::require(d.services(), actor, SEND_PERMISSION).await?;
    service::confirm(d, id, version, actor, ctx).await
}

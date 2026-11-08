//! Branding from code (#114): what `/admin/branding` does, for a
//! Deployment's own tools and for tests. Each call requires an active
//! Staff member holding `kernel.setting.edit` ([`crate::Error::Forbidden`]
//! otherwise), then checks, audits and saves as the page does.

pub use super::apply::MenuImagesSaved;
use super::apply::{self, Change};
use crate::audit::{Actor, RequestCtx};
use crate::{Deployment, Result};

/// Replaces the Menu images (`None` keeps the one in use; PNG bytes as
/// the Channels' [`crate::MenuArt::check`] wants them). The new rich
/// menus are created on the platform first: if that fails, the error is
/// returned and nothing is saved, audited or re-linked. Then every
/// account is re-linked by the Kernel's Jobs (ADR-0011) and the replaced
/// versions retired. [`crate::Error::Invalid`] when no Channel draws its
/// Menu on an image.
pub async fn set_menu_images(
    d: &Deployment,
    locked: Option<Vec<u8>>,
    unlocked: Option<Vec<u8>>,
    actor: &Actor,
    ctx: &RequestCtx,
) -> Result<MenuImagesSaved> {
    crate::auth::roles::require(d.services(), actor, super::EDIT).await?;
    let change = Change {
        locked,
        unlocked,
        reset: false,
    };
    apply::save(d, change, actor, ctx).await
}

/// Returns both Menu images to the Channels' configured ones (bundled or
/// from the environment), through the same path as
/// [`set_menu_images`].
pub async fn reset_menu_images(
    d: &Deployment,
    actor: &Actor,
    ctx: &RequestCtx,
) -> Result<MenuImagesSaved> {
    crate::auth::roles::require(d.services(), actor, super::EDIT).await?;
    let change = Change {
        locked: None,
        unlocked: None,
        reset: true,
    };
    apply::save(d, change, actor, ctx).await
}

//! Branding from code (#114): what `/admin/branding` does, for a
//! Deployment's own tools and for tests. Each call requires an active
//! Staff member holding `kernel.setting.edit` ([`crate::Error::Forbidden`]
//! otherwise), then checks, audits and saves as the page does.
//!
//! The display name and primary colour (#164) also have `*_as_system`
//! variants for a seed or the CLI: no Permission check, audited with actor
//! `system`, the same checks otherwise (as
//! [`crate::Deployment::save_setting_as_system`]). Never use them for a
//! Staff action. [`saved`] tells whether a value was ever saved, so a seed
//! can leave an Administrator's choice alone.

pub use super::apply::MenuImagesSaved;
use super::apply::{self, Change};
use super::save::{self, Logo, Outcome, Values};
use super::{COLOR_KEY, Chrome, LOGO_KEY, NAME_KEY, Stored, check};
use crate::audit::{Actor, RequestCtx};
use crate::{Deployment, Error, Result};

/// The display name and primary colour saved on `/admin/branding`, each
/// `None` while `deployment.toml` applies (never saved, reset, or saved
/// equal to the file's value).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct SavedBranding {
    /// `kernel.branding.name`.
    pub name: Option<String>,
    /// `kernel.branding.primary_color`, `#rrggbb`.
    pub primary_color: Option<String>,
}

/// What is saved now (one query).
pub async fn saved(d: &Deployment) -> Result<SavedBranding> {
    let stored = Stored::load(d.services()).await?;
    Ok(SavedBranding {
        name: stored.get(NAME_KEY).map(str::to_string),
        primary_color: stored.get(COLOR_KEY).map(str::to_string),
    })
}

/// Saves the institution's display name as `/admin/branding` does: trimmed,
/// 1 to 80 characters, no control characters ([`Error::Invalid`] naming the
/// catalog key otherwise); the other values are kept. A name equal to
/// `deployment.toml`'s is no setting: it follows the file. Audit
/// `branding.update` (or `branding.reset` once nothing is saved). Returns
/// whether anything changed; a concurrent save is [`Error::Conflict`].
pub async fn set_name(d: &Deployment, name: &str, actor: &Actor, ctx: &RequestCtx) -> Result<bool> {
    crate::auth::roles::require(d.services(), actor, super::EDIT).await?;
    set(d, Some(name), None, actor, ctx).await
}

/// [`set_name`] as the system, with no Permission check (a seed, the CLI).
pub async fn set_name_as_system(d: &Deployment, name: &str, ctx: &RequestCtx) -> Result<bool> {
    set(d, Some(name), None, &Actor::System, ctx).await
}

/// Saves the primary colour as `/admin/branding` does: `#rrggbb` (any
/// case, stored lowercase) on which white text reaches a WCAG contrast of
/// 4.5 ([`Error::Invalid`] naming the catalog key and the ratio
/// otherwise); the other values are kept. Otherwise as [`set_name`].
pub async fn set_primary_color(
    d: &Deployment,
    color: &str,
    actor: &Actor,
    ctx: &RequestCtx,
) -> Result<bool> {
    crate::auth::roles::require(d.services(), actor, super::EDIT).await?;
    set(d, None, Some(color), actor, ctx).await
}

/// [`set_primary_color`] as the system, with no Permission check.
pub async fn set_primary_color_as_system(
    d: &Deployment,
    color: &str,
    ctx: &RequestCtx,
) -> Result<bool> {
    set(d, None, Some(color), &Actor::System, ctx).await
}

fn refused(r: &check::Refused) -> Error {
    let args: Vec<String> = r.args.iter().map(|(k, v)| format!("{k} {v}")).collect();
    let args = if args.is_empty() {
        String::new()
    } else {
        format!(" ({})", args.join(", "))
    };
    Error::Invalid(format!("branding refused: {}{args}", r.key))
}

/// Checks the values given and saves them over the stored ones, the others
/// (and the logo) kept.
async fn set(
    d: &Deployment,
    name: Option<&str>,
    color: Option<&str>,
    actor: &Actor,
    ctx: &RequestCtx,
) -> Result<bool> {
    let name = name.map(check::name).transpose().map_err(|r| refused(&r))?;
    let color = color
        .map(check::color)
        .transpose()
        .map_err(|r| refused(&r))?;
    let stored = Stored::load(d.services()).await?;
    // A value equal to `deployment.toml`'s is no setting: it follows the file.
    let toml = Chrome::of(d.config(), &Stored::default());
    let kept = |k: &str| stored.get(k).map(str::to_string);
    let logo = match stored
        .get(LOGO_KEY)
        .filter(|k| super::store::is_logo_key(k))
    {
        Some(key) => Logo::Keep(key.to_string()),
        None => Logo::None,
    };
    let values = Values {
        name: match name {
            Some(n) => (n != toml.name).then_some(n),
            None => kept(NAME_KEY),
        },
        color: match color {
            Some(c) => (c != toml.color.to_ascii_lowercase()).then_some(c),
            None => kept(COLOR_KEY),
        },
        logo,
    };
    match save::save(d, &stored, values, actor, ctx).await? {
        Outcome::Saved => Ok(true),
        Outcome::Unchanged => Ok(false),
        Outcome::Stale => Err(Error::Conflict("branding changed meanwhile".into())),
    }
}

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

//! The seed's Kernel settings (#164): the display name and primary colour
//! (`/admin/branding`) and the LINE package (`/admin/usage`), saved through
//! [`harpseal::branding`] and [`harpseal::usage`] as the acting
//! Administrator, each only while nothing is saved, so an Administrator's
//! own choice survives a forced re-run.
//!
//! "Unset" is what the read says: a value saved equal to `deployment.toml`'s
//! is no setting, so it reads as unset and the seed's value replaces it;
//! and the read and the save are two steps, so a save landing in between
//! can be overwritten. Good enough for demo data, which is all this is.

use harpseal::usage::LinePackage;
use harpseal::{Actor, Deployment, RequestCtx};

use super::SeedError;

/// The display name the seed saves (Thai; `deployment.toml` has the
/// English one).
pub const BRAND_NAME: &str = "มหาวิทยาลัยอ่าวแมวน้ำ";
/// The primary colour the seed saves: a deeper blue than the TOML's,
/// white text on it well above the 4.5 contrast the page asks for.
pub const BRAND_COLOR: &str = "#1f4e79";
/// The LINE package the seed picks.
pub const LINE_PACKAGE: LinePackage = LinePackage::Basic;

/// Saves what is unset; returns how many values were written (0 to 3).
/// The package is skipped on a Deployment without LINE.
pub(super) async fn look(
    d: &Deployment,
    actor: &Actor,
    ctx: &RequestCtx,
) -> Result<usize, SeedError> {
    let mut written = 0;
    let saved = harpseal::branding::saved(d).await?;
    if saved.name.is_none() && harpseal::branding::set_name(d, BRAND_NAME, actor, ctx).await? {
        written += 1;
    }
    if saved.primary_color.is_none()
        && harpseal::branding::set_primary_color(d, BRAND_COLOR, actor, ctx).await?
    {
        written += 1;
    }
    let unset = harpseal::usage::line_package(d).await? == LinePackage::FollowConfig;
    if unset
        && d.channel("line").is_some()
        && harpseal::usage::set_line_package(d, LINE_PACKAGE, actor, ctx).await?
    {
        written += 1;
    }
    Ok(written)
}

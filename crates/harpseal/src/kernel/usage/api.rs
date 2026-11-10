//! The LINE package from code (#164): what the form behind
//! `POST /admin/usage/package` does, for a Deployment's own tools.
//! [`set_line_package`] requires an active Staff member holding
//! `kernel.setting.edit` ([`Error::Forbidden`] otherwise);
//! [`set_line_package_as_system`] is for a seed or the CLI (no Permission
//! check, audited with actor `system`; never for a Staff action). Both
//! audit `setting.update` (or `setting.reset`) with the value before and
//! after, in the same batch as the value, as the page does.

use super::package::{self, Choice, KEY};
use crate::audit::{Actor, RequestCtx};
use crate::{Deployment, Error, Result};

/// A LINE Official Account package, or what the monthly Quota follows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum LinePackage {
    /// Free: 300 messages a month, no more can be bought.
    Free,
    /// Basic: 15,000 messages a month.
    Basic,
    /// Pro: 35,000 messages a month.
    Pro,
    /// Another monthly number, 1 to 100,000,000.
    Custom(u64),
    /// Nothing saved: `[quotas.line] metered_per_month` applies (the
    /// form's "ตามค่าในไฟล์ตั้งค่า"). Setting it removes the saved choice.
    FollowConfig,
}

impl LinePackage {
    /// The choice it stands for, or why not.
    fn choice(self) -> Result<Option<Choice>> {
        let named = |id: &str| package::package(id).map(|p| Some(Choice::Package(p)));
        let found = match self {
            LinePackage::Free => named("free"),
            LinePackage::Basic => named("basic"),
            LinePackage::Pro => named("pro"),
            LinePackage::Custom(n) => {
                package::custom(&n.to_string()).map(|n| Some(Choice::Custom(n)))
            }
            LinePackage::FollowConfig => Some(None),
        };
        found.ok_or_else(|| {
            Error::Invalid(format!(
                "LINE package refused: kernel.usage.package.err_custom ({self:?})"
            ))
        })
    }

    fn of(choice: Choice) -> Option<Self> {
        match choice {
            Choice::Package(p) => match p.id {
                "free" => Some(LinePackage::Free),
                "basic" => Some(LinePackage::Basic),
                "pro" => Some(LinePackage::Pro),
                _ => None,
            },
            Choice::Custom(n) => Some(LinePackage::Custom(n)),
            Choice::Config(_) => None,
        }
    }
}

/// The package saved now: [`LinePackage::FollowConfig`] while nothing is
/// saved, or the saved row cannot be read (the page then asks Staff to
/// choose again).
pub async fn line_package(d: &Deployment) -> Result<LinePackage> {
    let cur = package::current(d.services(), d.config()).await?;
    Ok(LinePackage::of(cur.choice).unwrap_or(LinePackage::FollowConfig))
}

/// Saves the LINE package as the usage page does. Returns whether
/// anything changed (choosing what is already in force writes nothing);
/// [`Error::Invalid`] when the Deployment has no LINE Channel or the
/// Custom number is out of range; a save in between is [`Error::Conflict`].
pub async fn set_line_package(
    d: &Deployment,
    choice: LinePackage,
    actor: &Actor,
    ctx: &RequestCtx,
) -> Result<bool> {
    crate::auth::roles::require(d.services(), actor, super::package_page::EDIT).await?;
    set(d, choice, actor, ctx).await
}

/// [`set_line_package`] as the system, with no Permission check.
pub async fn set_line_package_as_system(
    d: &Deployment,
    choice: LinePackage,
    ctx: &RequestCtx,
) -> Result<bool> {
    set(d, choice, &Actor::System, ctx).await
}

async fn set(d: &Deployment, choice: LinePackage, actor: &Actor, ctx: &RequestCtx) -> Result<bool> {
    if !super::store::kinds(d).iter().any(|k| k == package::KIND) {
        return Err(Error::Invalid(
            "LINE package refused: kernel.usage.package.err_no_line".into(),
        ));
    }
    let want = choice.choice()?;
    let s = d.services();
    let cur = package::current(s, d.config()).await?;
    let conflict = || Error::Conflict("the LINE package changed meanwhile".into());
    let Some(want) = want else {
        let Some(row) = cur.stored.as_deref() else {
            return Ok(false);
        };
        // `null` in the audit row when no Quota applies after the reset.
        let fallback = Choice::Config(cur.config).encode();
        let reset = crate::kernel::admin::reset_setting_row(
            s,
            actor,
            KEY,
            "",
            row,
            fallback.as_deref(),
            ctx,
            &|_| Ok(Vec::new()),
        )
        .await?;
        return if reset { Ok(true) } else { Err(conflict()) };
    };
    let value = want.encode().unwrap_or_default();
    // What is in force now as a stored value: the row, or the TOML number
    // as Custom. Choosing that again changes nothing.
    let before = cur.stored.clone().or_else(|| cur.choice.encode());
    if before.as_deref() == Some(value.as_str()) {
        return Ok(false);
    }
    let saved = crate::kernel::admin::save_setting_row(
        s,
        actor,
        KEY,
        "",
        &value,
        cur.stored.as_deref(),
        before.as_deref(),
        ctx,
        &|_| Ok(Vec::new()),
    )
    .await?;
    if saved { Ok(true) } else { Err(conflict()) }
}

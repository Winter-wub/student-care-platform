//! Saving the display name, primary colour and logo (#114): one batch
//! gated on its audit row (`branding.update`, or `branding.reset` when
//! every value returns to `deployment.toml`), written only while the
//! settings still hold what the save read. The new logo goes to the
//! FileStore under its content key first; the one it replaced is deleted
//! once no setting names it.

use serde_json::json;

use super::apply::{by, put, still};
use super::check::LogoKind;
use super::{COLOR_KEY, LOGO_KEY, NAME_KEY, Stored, store};
use crate::audit::{self, Actor, RequestCtx, Target};
use crate::{Deployment, Result};

/// Where logos go.
const LOGO_DIR: &str = "branding";

/// The values to save, checked ([`super::check`]). `None` returns that
/// value to `deployment.toml`.
#[derive(Debug, Clone, Default)]
pub(crate) struct Values {
    pub(crate) name: Option<String>,
    pub(crate) color: Option<String>,
    /// A new logo, or the stored key to keep, or none.
    pub(crate) logo: Logo,
}

/// The logo of a save.
#[derive(Debug, Clone, Default)]
pub(crate) enum Logo {
    /// Back to `deployment.toml`'s.
    #[default]
    None,
    /// The stored one.
    Keep(String),
    /// A new file.
    New(Vec<u8>, LogoKind),
}

/// What a save did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Outcome {
    Saved,
    Unchanged,
    Stale,
}

/// Saves `v` over `before` (what the page was rendered from, read again
/// by the caller); refreshes this process's chrome.
pub(crate) async fn save(
    d: &Deployment,
    before: &Stored,
    v: Values,
    actor: &Actor,
    ctx: &RequestCtx,
) -> Result<Outcome> {
    let s = d.services();
    let logo = match &v.logo {
        Logo::None => None,
        Logo::Keep(k) => Some(k.clone()),
        Logo::New(bytes, kind) => Some(store::key_of(LOGO_DIR, bytes, kind.ext())),
    };
    let old = |k| before.get(k).map(str::to_string);
    let after = [
        (NAME_KEY, v.name.clone()),
        (COLOR_KEY, v.color.clone()),
        (LOGO_KEY, logo.clone()),
    ];
    if after.iter().all(|(k, val)| *val == old(k)) {
        return Ok(Outcome::Unchanged);
    }
    if let Logo::New(bytes, kind) = &v.logo {
        store::put(s, LOGO_DIR, bytes, kind.ext(), kind.content_type()).await?;
    }
    let shown = |name: Option<String>, color: Option<String>, logo: Option<String>| json!({ "name": name, "primary_color": color, "logo": logo });
    let condition = still(&after.clone().map(|(k, _)| (k, old(k))));
    let reset = after.iter().all(|(_, val)| val.is_none());
    let (row, gate) = audit::entry_if(
        s,
        actor,
        if reset {
            "branding.reset"
        } else {
            "branding.update"
        },
        Some(&Target::new("setting", "kernel.branding")),
        Some(shown(old(NAME_KEY), old(COLOR_KEY), old(LOGO_KEY))),
        Some(shown(v.name.clone(), v.color.clone(), logo.clone())),
        ctx,
        condition,
    )?;
    let now = s.now_ms();
    let mut stmts = vec![row];
    for (k, val) in &after {
        stmts.push(put(k, val.as_deref(), by(actor), now, &gate));
    }
    let done = s.db.batch(stmts).await?;
    let written = done.first().is_some_and(|r| r.rows_affected == 1);
    // The file no setting names any more: the replaced logo, or the new
    // one when another save came first.
    let replaced: Vec<String> = match (written, &v.logo) {
        (true, _) => old(LOGO_KEY)
            .into_iter()
            .filter(|k| Some(k) != logo.as_ref())
            .collect(),
        (false, Logo::New(..)) => logo.into_iter().collect(),
        (false, _) => Vec::new(),
    };
    store::forget(s, &replaced).await;
    if !written {
        return Ok(Outcome::Stale);
    }
    if let Ok(stored) = Stored::load(s).await {
        d.branding().put(stored, now);
    }
    Ok(Outcome::Saved)
}

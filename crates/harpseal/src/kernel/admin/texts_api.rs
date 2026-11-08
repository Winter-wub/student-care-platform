//! Writing one text: the single path the Texts page (`POST
//! /admin/texts/{key}`, save) and the public [`set_override`] share, so the
//! two cannot drift. The public function checks the acting Staff member's
//! `kernel.setting.edit` (the page's guard does it for the page) and
//! audits the write as `setting.update` by that Staff member.

use super::settings::{self, Refused};
use crate::audit::{Actor, RequestCtx};
use crate::{Deployment, Error, Result};

/// The Permission [`set_override`] requires.
pub const SETTING_PERMISSION: &str = "kernel.setting.edit";

/// What [`write`] did.
#[derive(Debug)]
pub(super) enum Written {
    /// Not a key the Texts page edits, or a Locale it does not offer.
    NotEditable,
    /// The form was rendered from another stored value (stale version).
    Stale,
    /// The stored override already is the value: nothing written.
    Unchanged,
    /// The value breaks a rule (empty, too long, placeholders).
    Refused(Refused),
    /// Written (`true`), or the row changed meanwhile (`false`).
    Saved(bool),
}

/// Stores `value` (CRLF made LF) as the text of `key` in `locale`.
/// `version`, when given, is the form's compare-and-set token
/// ([`settings::version`] of the value it was rendered from).
/// A save of a Menu label also records the next Menu revision and
/// enqueues its re-link, in the same batch ([`crate::kernel::menu`]).
#[allow(clippy::too_many_arguments)]
pub(super) async fn write(
    d: &Deployment,
    key: &str,
    locale: &str,
    value: &str,
    version: Option<&str>,
    actor: &Actor,
    ctx: &RequestCtx,
) -> Result<Written> {
    let (s, catalog, config) = (d.services(), d.catalog(), d.config());
    if !settings::editable(catalog, key)
        || locale.trim().is_empty()
        || !settings::locales(catalog, config)
            .iter()
            .any(|l| l == locale)
    {
        return Ok(Written::NotEditable);
    }
    let stored = settings::settings(s).await?;
    let current = stored.get(&(key.to_string(), locale.to_string()));
    if version.is_some_and(|v| settings::version(current.map(String::as_str)) != v) {
        return Ok(Written::Stale);
    }
    // Browsers send textarea line breaks as CRLF.
    let value = value.replace("\r\n", "\n");
    if current.is_some_and(|c| *c == value) {
        return Ok(Written::Unchanged);
    }
    let row = settings::row(catalog, config, &Default::default(), key, locale);
    if let Err(why) = settings::check(&row.base, &value) {
        return Ok(Written::Refused(why));
    }
    let expected = current.map(String::as_str);
    let before = expected.unwrap_or(&row.base);
    let relink =
        |gate: &str| crate::kernel::menu::on_save(d, &stored, key, locale, Some(&value), gate);
    let done = settings::save(
        s,
        actor,
        key,
        locale,
        &value,
        expected,
        Some(before),
        ctx,
        &relink,
    )
    .await?;
    Ok(Written::Saved(done))
}

/// Stores `value` as the text of `key` in `locale` (a `setting` row; live
/// on the next message): the same path as the Texts page. Refused like on
/// the page: the key must be one the Texts page edits, the Locale one it
/// offers, and `value` must keep the `{name}` placeholders of the text it
/// replaces, be non-empty and at most 5 000 characters. Returns `false`,
/// writing nothing, when the stored override already is `value` or the row
/// changed meanwhile.
pub async fn set_override(
    d: &Deployment,
    key: &str,
    locale: &str,
    value: &str,
    actor: &Actor,
    ctx: &RequestCtx,
) -> Result<bool> {
    crate::auth::roles::require(d.services(), actor, SETTING_PERMISSION).await?;
    match write(d, key, locale, value, None, actor, ctx).await? {
        Written::Saved(done) => Ok(done),
        Written::Unchanged | Written::Stale => Ok(false),
        Written::NotEditable => Err(Error::Invalid(format!(
            "`{key}` in Locale `{locale}` is not an editable text"
        ))),
        Written::Refused(why) => Err(Error::Invalid(format!(
            "text `{key}` refused: {} {}",
            why.key,
            why.names.join(", ")
        ))),
    }
}

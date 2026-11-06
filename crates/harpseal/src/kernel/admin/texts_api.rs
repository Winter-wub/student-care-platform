//! Text overrides from code: the same path the back office's Texts page
//! uses (`POST /admin/texts/{key}`), for a CLI or a seed. The acting Staff
//! member must be active and hold `kernel.setting.edit`, as on the page;
//! the write is audited as `setting.update` by that Staff member.

use super::settings;
use crate::audit::{Actor, RequestCtx};
use crate::{Deployment, Error, Result};

/// The Permission [`set_override`] requires.
pub const SETTING_PERMISSION: &str = "kernel.setting.edit";

/// Stores `value` as the text of `key` in `locale` (a `setting` row; live
/// on the next message). Refused like on the page: the key must be one
/// the Texts page edits, the Locale one it offers, and `value` must keep
/// the `{name}` placeholders of the text it replaces, be non-empty and at
/// most 5 000 characters. Returns `false`, writing nothing, when the
/// stored override already is `value` or the row changed meanwhile.
pub async fn set_override(
    d: &Deployment,
    key: &str,
    locale: &str,
    value: &str,
    actor: &Actor,
    ctx: &RequestCtx,
) -> Result<bool> {
    crate::auth::roles::require(d.services(), actor, SETTING_PERMISSION).await?;
    let (catalog, config) = (d.catalog(), d.config());
    if !settings::editable(catalog, key) {
        return Err(Error::Invalid(format!("`{key}` is not an editable text")));
    }
    if !settings::locales(catalog, config)
        .iter()
        .any(|l| l == locale)
    {
        return Err(Error::Invalid(format!("Locale `{locale}` is not offered")));
    }
    let value = value.replace("\r\n", "\n");
    let stored = settings::settings(d.services()).await?;
    let current = stored.get(&(key.to_string(), locale.to_string()));
    if current.is_some_and(|c| *c == value) {
        return Ok(false);
    }
    let row = settings::row(catalog, config, &Default::default(), key, locale);
    if let Err(why) = settings::check(&row.base, &value) {
        return Err(Error::Invalid(format!(
            "text `{key}` refused: {} {}",
            why.key,
            why.names.join(", ")
        )));
    }
    let expected = current.map(String::as_str);
    let before = expected.unwrap_or(&row.base);
    settings::save(
        d.services(),
        actor,
        key,
        locale,
        &value,
        expected,
        before,
        ctx,
    )
    .await
}

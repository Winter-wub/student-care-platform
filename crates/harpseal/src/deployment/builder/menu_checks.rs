//! Build-time checks on the Menu keys and the Menu variants (#182).

use crate::config::MenuConfig;
use crate::error::BuildError;
use crate::kernel::app::AppLink;
use crate::manifest::Catalog;
use crate::manifest::MenuEntrySpec;

/// Every Locale of the catalog maps to its own Menu key, so the Kernel's
/// Menus (keyed by the effective Locale) always carry one; so does each
/// Menu variant in each Locale (`<locale key><variant key>`), and no two
/// of these keys are the same.
pub(super) fn keys(catalog: &Catalog, menu: &MenuConfig) -> Result<(), BuildError> {
    let mut seen: Vec<(crate::MenuKey, String)> = Vec::new();
    for locale in catalog.locales() {
        let bad = |reason: String| BuildError::MenuKey {
            locale: locale.0.clone(),
            reason,
        };
        let key = crate::MenuKey::for_locale(&locale).map_err(|e| bad(e.to_string()))?;
        let mut keys = vec![(key.clone(), format!("Locale `{}`", locale.0))];
        for v in &menu.variants {
            let k = crate::MenuKey::new(format!("{}{}", key.as_str(), v.key))
                .map_err(|e| bad(format!("Menu variant `{}`: {e}", v.key)))?;
            keys.push((
                k,
                format!("Menu variant `{}` in Locale `{}`", v.key, locale.0),
            ));
        }
        for (k, what) in keys {
            if let Some((_, other)) = seen.iter().find(|(s, _)| *s == k) {
                return Err(bad(format!(
                    "`{}` of {what} is also the key of {other}",
                    k.as_str()
                )));
            }
            seen.push((k, what));
        }
    }
    Ok(())
}

/// Every Menu variant keeps only registered Menu entries and Mini App
/// home links.
pub(super) fn variants(
    menu: &MenuConfig,
    entries: &[MenuEntrySpec],
    links: &[AppLink],
) -> Result<(), BuildError> {
    for v in &menu.variants {
        let bad = |why: String| {
            BuildError::InvalidConfig(format!("[[menu.variants]] `{}`: {why}", v.key))
        };
        if let Some(a) = v
            .entries
            .iter()
            .find(|a| !entries.iter().any(|e| e.action.as_str() == a.as_str()))
        {
            return Err(bad(format!("entries: no Menu entry has Action `{a}`")));
        }
        if let Some(p) = v
            .app_links
            .iter()
            .find(|p| !links.iter().any(|l| l.path == p.as_str()))
        {
            return Err(bad(format!("app_links: no Mini App home link to `{p}`")));
        }
    }
    Ok(())
}

//! Build-time checks on names and texts, apart from `builder.rs`.

use crate::config::DeploymentConfig;
use crate::error::BuildError;
use crate::manifest::Catalog;
use crate::module::{NavSpec, StatusSpec};
use crate::{Module, ModuleRegistrar};

/// A Module name that claims the Kernel's namespace: `kernel` or
/// `kernel.…`. Checked on the Modules a Deployment adds, before the
/// duplicate-name check, so the error names the real problem.
pub(super) fn module_names(modules: &[Box<dyn Module>]) -> Result<(), BuildError> {
    match modules.iter().map(|m| m.name()).find(|n| reserved(n)) {
        Some(name) => Err(claim(name, "name", name)),
        None => Ok(()),
    }
}

/// What a Module registered must stay out of the Kernel's namespace: its
/// Permissions, Action ids and prefixes and text keys may not start with
/// `kernel.` (setting keys have their own check). The Kernel itself is
/// exempt.
pub(super) fn kernel_namespace(r: &ModuleRegistrar) -> Result<(), BuildError> {
    if r.module == crate::kernel::NAME {
        return Ok(());
    }
    let own = |k: &str| k.starts_with("kernel.");
    let found = r
        .permissions
        .iter()
        .map(|p| ("permission", p.id))
        .chain(r.actions.iter().map(|(id, _)| ("action", id.as_str())))
        .chain(r.action_prefixes.iter().map(|(p, _)| ("action prefix", *p)))
        .chain(r.catalog.iter().map(|e| ("text key", e.key)))
        .find(|(_, key)| own(key));
    match found {
        Some((what, key)) => Err(claim(r.module, what, key)),
        None => Ok(()),
    }
}

fn reserved(name: &str) -> bool {
    name == crate::kernel::NAME || name.starts_with("kernel.")
}

fn claim(module: &str, what: &'static str, key: &str) -> BuildError {
    BuildError::KernelNamespace {
        module: module.into(),
        what,
        key: key.into(),
    }
}

/// `[texts]` may only override keys some Module declares: a typo would
/// otherwise be ignored silently.
pub(super) fn toml_texts(config: &DeploymentConfig, catalog: &Catalog) -> Result<(), BuildError> {
    for (locale, entries) in &config.texts {
        if let Some(key) = entries.keys().find(|k| !catalog.knows(k)) {
            return Err(BuildError::UnknownText {
                source: format!("deployment.toml [texts.{locale}]"),
                key: key.clone(),
            });
        }
    }
    Ok(())
}

/// Navigation entries: a known title in the default Locale, no parameters.
pub(super) fn nav(nav: &[(&'static str, NavSpec)], catalog: &Catalog) -> Result<(), BuildError> {
    let locale = catalog.default_locale();
    for (module, n) in nav {
        if n.path.contains('{') {
            return Err(BuildError::InvalidRoute {
                module,
                path: n.path.into(),
            });
        }
        if !catalog.has(&locale, n.title_key) {
            return Err(BuildError::UnknownText {
                source: format!("module `{module}`"),
                key: n.title_key.into(),
            });
        }
    }
    Ok(())
}

pub(super) fn nav_items(nav: Vec<(&'static str, NavSpec)>) -> Vec<crate::auth::NavItem> {
    let mut items: Vec<crate::auth::NavItem> = nav
        .into_iter()
        .map(|(_, n)| crate::auth::NavItem {
            permission: n.permission,
            path: format!("{}{}", crate::deployment::routes::ADMIN, n.path),
            title_key: n.title_key,
            order: n.order,
            group: n.group,
        })
        .collect();
    items.sort_by_key(|n| (n.group, n.order));
    items
}

/// Status cards (#123): each guarded by a declared Permission, sorted by
/// `order` (registration order breaks ties).
pub(super) fn status_cards(
    cards: Vec<(&'static str, StatusSpec)>,
    permissions: &[crate::manifest::Permission],
) -> Result<Vec<StatusSpec>, BuildError> {
    for (module, c) in &cards {
        if !permissions.iter().any(|p| p.id == c.permission) {
            let permission = c.permission;
            return Err(BuildError::UnguardedRoute { module, permission });
        }
    }
    let mut cards: Vec<StatusSpec> = cards.into_iter().map(|(_, c)| c).collect();
    cards.sort_by_key(|c| c.order);
    Ok(cards)
}

//! The Deployment's unlocked Menus: the whole one per Locale and each
//! Menu variant's (#182). Kept apart from `mod.rs` so each file stays small.

use super::Deployment;
use crate::kernel;
use crate::message::{ActionId, Locale, Menu, MenuEntry};

impl Deployment {
    /// The unlocked Menu in `locale`: the Modules' entries by `order`,
    /// then the Kernel's "unlink" entry (`kernel.link.unlink`) when
    /// `[student] self_unlink` is on. Its key is that of the effective
    /// Locale ([`crate::Catalog::effective`], [`MenuKey::for_locale`]): a Student
    /// in `TH` or `en-US` with no texts of their own reads the default
    /// Locale, and gets its Menu and key. `build()` checked that every
    /// catalog Locale has a key of its own.
    ///
    /// [`MenuKey::for_locale`]: crate::MenuKey::for_locale
    pub fn menu_for(&self, locale: &Locale) -> Menu {
        self.menu_with(locale, |key| self.catalog.t(locale, key))
    }

    /// The unlocked Menu in `locale` with labels from `t`, a label key to
    /// its text (the chat flow passes texts with `setting` overrides);
    /// keyed as in [`Deployment::menu_for`], whatever the labels.
    pub fn menu_with(&self, locale: &Locale, t: impl Fn(&str) -> String) -> Menu {
        self.menu_in(locale, None, t)
    }

    /// [`Deployment::menu_with`] for the Students of Menu variant
    /// `variant` (`[[menu.variants]]`, #182): only the entries it keeps,
    /// then the unlink entry as always; keyed `<locale key><variant key>`
    /// and marked with [`Menu::variant`]. `None` is the whole Menu.
    pub fn menu_in(
        &self,
        locale: &Locale,
        variant: Option<&crate::MenuVariant>,
        t: impl Fn(&str) -> String,
    ) -> Menu {
        let mut entries: Vec<MenuEntry> = self
            .registry
            .menu
            .iter()
            .filter(|e| variant.is_none_or(|v| v.keeps_entry(e.action.as_str())))
            .map(|e| MenuEntry {
                label: t(e.label_key),
                action: e.action.clone(),
            })
            .collect();
        if self.config.student.self_unlink
            && let Ok(action) = ActionId::new(kernel::flow::actions::UNLINK)
        {
            entries.push(MenuEntry {
                label: t("kernel.menu.unlink"),
                action,
            });
        }
        let menu = Menu::new(entries);
        let key = crate::MenuKey::for_locale(&self.catalog.effective(locale));
        let key = match variant {
            Some(v) => key.and_then(|k| crate::MenuKey::new(format!("{}{}", k.as_str(), v.key))),
            None => key,
        };
        // Unreachable: the effective Locale is a catalog Locale, and
        // `build()` refused any without a key, or whose variant keys are
        // not valid (`BuildError::MenuKey`).
        debug_assert!(key.is_ok(), "{locale:?}: {key:?}");
        let menu = match key {
            Ok(key) => menu.with_key(key),
            Err(_) => menu,
        };
        match variant {
            Some(v) => menu.with_variant(v.key.clone()),
            None => menu,
        }
    }

    /// The Menu variant of a Student with `attrs` (#182), if any.
    pub fn menu_variant(
        &self,
        attrs: &serde_json::Map<String, serde_json::Value>,
    ) -> Option<&crate::MenuVariant> {
        self.config.menu.variant_for(attrs)
    }

    /// The catalog keys of the unlocked Menu's labels: the Modules'
    /// entries, then `kernel.menu.unlink` when `[student] self_unlink` is
    /// on. A back-office save of one of them re-links the Menu (#66).
    pub(crate) fn menu_label_keys(&self) -> Vec<&'static str> {
        let mut keys: Vec<&'static str> = self.registry.menu.iter().map(|e| e.label_key).collect();
        if self.config.student.self_unlink {
            keys.push("kernel.menu.unlink");
        }
        keys
    }
}

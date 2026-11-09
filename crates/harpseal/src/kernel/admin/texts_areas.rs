//! Where a text shows (#125): the page or place Staff know it from, so the
//! Texts page can label and group texts by purpose instead of by key.
//! Kernel texts go by their area (`kernel.<area>.…`); a Module's texts by
//! the title of its first back-office page, else its own `<module>.title`
//! or `<module>.menu` text, else its name.

use super::view::View;

/// A page or place texts are grouped under.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(super) struct Area {
    /// The filter value: `kernel.<area>` or the Module name.
    pub(super) id: String,
    /// What Staff read.
    pub(super) name: String,
}

/// The catalog key naming each Kernel area; the rest are "ทั่วไป".
fn kernel_area(area: &str) -> (&'static str, &'static str) {
    match area {
        "action" | "consent" | "flow" | "inbound" | "link" | "notice" | "otp" => {
            ("kernel.chat", "kernel.admin.texts.area_chat")
        }
        "app" => ("kernel.app", "kernel.admin.texts.area_app"),
        "menu" => ("kernel.menu", "kernel.admin.texts.area_menu"),
        "backup" => ("kernel.backup", "kernel.admin.texts.area_backup"),
        "branding" => ("kernel.branding", "kernel.admin.texts.area_branding"),
        "home" | "backoffice" => ("kernel.home", "kernel.admin.texts.area_home"),
        "usage" => ("kernel.usage", "kernel.admin.texts.area_usage"),
        "roster" => ("kernel.roster", "kernel.admin.texts.area_roster"),
        _ => ("kernel.other", "kernel.admin.texts.area_other"),
    }
}

/// The area of `key`.
pub(super) fn area(v: &View<'_>, key: &str) -> Area {
    let mut parts = key.split('.');
    let module = parts.next().unwrap_or(key);
    if module == crate::kernel::NAME {
        let (id, name_key) = kernel_area(parts.next().unwrap_or(""));
        return Area {
            id: id.into(),
            name: v.t(name_key),
        };
    }
    Area {
        id: module.into(),
        name: module_name(v, module),
    }
}

/// A Module as Staff know it.
fn module_name(v: &View<'_>, module: &str) -> String {
    let page = super::roles_pages::group_name(v, module);
    if page != module {
        return page;
    }
    [format!("{module}.title"), format!("{module}.menu")]
        .iter()
        .find(|k| v.s.catalog.knows(k))
        .map_or_else(|| module.to_string(), |k| v.t(k))
}

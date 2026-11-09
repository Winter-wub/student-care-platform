//! Where a text shows (#125): the page or place Staff know it from, so the
//! Texts page can label and group texts by purpose instead of by key.
//! Kernel texts go by their area (`kernel.<area>.…`); a Module's texts by
//! [`super::roles_pages::group_name`].

use std::collections::HashMap;

use super::view::View;

/// A page or place texts are grouped under.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Area {
    /// The filter value and grouping: `kernel.<area>` or the Module name.
    pub(super) id: String,
    /// What Staff read.
    pub(super) name: String,
}

/// The area id and the catalog key naming each Kernel area the Texts page
/// can list; the rest are "ทั่วไป".
fn kernel_area(area: &str) -> (&'static str, &'static str) {
    match area {
        "action" | "consent" | "flow" | "inbound" | "link" | "notice" | "otp" => {
            ("kernel.chat", "kernel.admin.texts.area_chat")
        }
        "app" => ("kernel.app", "kernel.admin.texts.area_app"),
        "menu" => ("kernel.menu", "kernel.admin.texts.area_menu"),
        "backup" => ("kernel.backup", "kernel.admin.texts.area_backup"),
        "branding" => ("kernel.branding", "kernel.admin.texts.area_branding"),
        "usage" => ("kernel.usage", "kernel.admin.texts.area_usage"),
        _ => ("kernel.other", "kernel.admin.texts.area_other"),
    }
}

/// Areas by Kernel area or Module, each worked out once per page render
/// (a Module's name looks through the navigation and the catalog).
pub(super) struct Areas<'v, 'a> {
    v: &'v View<'a>,
    seen: HashMap<String, Area>,
}

impl<'v, 'a> Areas<'v, 'a> {
    pub(super) fn new(v: &'v View<'a>) -> Self {
        Self {
            v,
            seen: HashMap::new(),
        }
    }

    /// The area of `key`.
    pub(super) fn of(&mut self, key: &str) -> Area {
        let mut parts = key.split('.');
        let module = parts.next().unwrap_or(key);
        let slot = if module == crate::kernel::NAME {
            kernel_area(parts.next().unwrap_or("")).0
        } else {
            module
        };
        if let Some(a) = self.seen.get(slot) {
            return a.clone();
        }
        let area = if module == crate::kernel::NAME {
            let (id, name_key) = kernel_area(key.split('.').nth(1).unwrap_or(""));
            Area {
                id: id.into(),
                name: self.v.t(name_key),
            }
        } else {
            Area {
                id: module.into(),
                name: super::roles_pages::group_name(self.v, module),
            }
        };
        self.seen.insert(slot.to_string(), area.clone());
        area
    }
}

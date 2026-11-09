//! Which catalog texts the Texts page edits (#27, #125): those read
//! through `Texts::load`, where a saved `setting` row takes effect.

use crate::Catalog;

/// Prefixes of texts the back office reads from the build-time catalog
/// (its own pages), never through `setting`: an edit would not show, so
/// the Texts page leaves them out.
const BUILD_TIME: [&str; 5] = [
    "kernel.admin.",
    "kernel.auth.",
    "kernel.backoffice.",
    "kernel.students.",
    "kernel.roster.",
];

/// Whether `key` is a catalog key the Texts page edits: read through
/// `Texts::load` (chat flow, Module Actions, `/notice`, emails), not a
/// back-office page text. Back-office pages read the catalog only (the
/// TOML's `[texts]` over the defaults), so a `setting` row there would
/// never show: those keys are left off the page.
pub(super) fn editable(catalog: &Catalog, key: &str) -> bool {
    catalog.knows(key)
        && !BUILD_TIME.iter().any(|p| key.starts_with(p))
        && !label_only(key)
        && !page_only(key)
}

/// Labels the back office reads from the catalog (#125): Role and Locale
/// names, and a Module's audit actions (`<module>.audit.<action>`).
/// Change them in `[texts]`.
fn label_only(key: &str) -> bool {
    key.starts_with("kernel.role.")
        || key.starts_with("kernel.locale.")
        || key.split('.').nth(1) == Some("audit")
}

/// Texts of the home, Backups and usage pages; their emails (`…email_…`)
/// go through `Texts::load` and stay editable.
fn page_only(key: &str) -> bool {
    ["kernel.home.", "kernel.backup.", "kernel.usage."]
        .iter()
        .any(|p| key.starts_with(p))
        && !key.contains("email_")
}

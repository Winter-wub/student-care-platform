//! The example's LINE rich menus: the locked Menu (one "เริ่มใช้งาน" entry)
//! and the unlocked one, each drawn on a bundled PNG that follows the
//! Core's grid (`docs/guide/channels.md`, "Rich menu images").
//!
//! The PNGs are made by `deployments/example/assets/richmenu/make.sh`. When
//! a Menu gains or loses an entry, run it again and set the cell counts
//! below: `tests/menu_images.rs` fails while the counts and the Menus
//! disagree.
//!
//! Native binaries may name other files in [`LOCKED_VAR`] and
//! [`UNLOCKED_VAR`] ([`load`]); Workers use the bundled bytes (a customer's
//! Deployment bundles its own, or reads an asset binding).

use harpseal::{ActionId, Menu, MenuEntry};

/// The locked Menu's image, compiled in.
pub const LOCKED_PNG: &[u8] = include_bytes!("../../assets/richmenu/locked.png");
/// The unlocked Menu's image, compiled in.
pub const UNLOCKED_PNG: &[u8] = include_bytes!("../../assets/richmenu/unlocked.png");
/// Cells [`LOCKED_PNG`] is drawn with: the entries of [`locked_menu`].
pub const LOCKED_CELLS: usize = 1;
/// Cells [`UNLOCKED_PNG`] is drawn with: the entries of the unlocked Menu
/// in the default Locale (latest news, contact Staff, unlink).
pub const UNLOCKED_CELLS: usize = 3;

/// Environment variable naming a PNG file for the locked Menu.
pub const LOCKED_VAR: &str = "LINE_MENU_IMAGE_LOCKED";
/// Environment variable naming a PNG file for the unlocked Menu.
pub const UNLOCKED_VAR: &str = "LINE_MENU_IMAGE_UNLOCKED";

/// The locked Menu: one "เริ่มใช้งาน" entry that starts Verification, like a
/// Follow (`kernel.link.start`).
pub fn locked_menu() -> Menu {
    Menu::new(vec![MenuEntry {
        label: "เริ่มใช้งาน".into(),
        action: ActionId::new("kernel.link.start").expect("Action id is ≤ 64 bytes"),
    }])
}

/// The locked and unlocked images: the file named by [`LOCKED_VAR`] /
/// [`UNLOCKED_VAR`] when `get` has a non-empty value, else the bundled one.
/// `read` reads a file (`std::fs::read` natively). An unreadable file is an
/// error naming the variable and the path.
pub fn load(
    get: impl Fn(&str) -> Option<String>,
    read: impl Fn(&str) -> std::io::Result<Vec<u8>>,
) -> Result<(Vec<u8>, Vec<u8>), String> {
    let one = |var: &str, bundled: &[u8]| match get(var).filter(|p| !p.is_empty()) {
        Some(path) => read(&path).map_err(|e| format!("{var}={path}: {e}")),
        None => Ok(bundled.to_vec()),
    };
    Ok((
        one(LOCKED_VAR, LOCKED_PNG)?,
        one(UNLOCKED_VAR, UNLOCKED_PNG)?,
    ))
}

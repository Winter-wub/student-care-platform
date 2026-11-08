//! Menu images edited in the back office (#114): the Channel Capability
//! [`MenuArt`] for an adapter whose Menu is drawn on an image (LINE rich
//! menus), and the images the Kernel hands it ([`MenuImages`]).
//!
//! The adapter keeps the images it was configured with (bundled, or from
//! the environment) as the defaults. An Administrator may upload others
//! on `/admin/branding`; the Kernel then draws every Menu through
//! [`MenuArt::drawn_on`], a view of the Channel with those images, so
//! `set_menu`, `relink_menu` and `retire_menus` keep their signatures.

use std::sync::Arc;

use async_trait::async_trait;

use super::Channel;
use crate::Result;
use crate::message::Menu;

/// Which Menu an image is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum MenuSlot {
    /// The locked Menu, before a Link (`set_menu(None)`).
    Locked,
    /// The unlocked Menu of a linked account.
    Unlocked,
}

/// Images to draw the Menus on instead of the adapter's configured ones.
/// `None` keeps the configured image. The locked Menu has no Menu
/// revision of its own, so once its image changed (a reset back to the
/// configured one too) it comes with one (`locked_revision`, bumped by
/// the Kernel on each change; 0: never changed) and the Deployment's
/// revision salt: an adapter names each version apart and
/// may retire the older ones like those of an unlocked Menu
/// ([`Channel::retire_menus`] with a Menu keyed [`crate::MenuKey::locked`]).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct MenuImages {
    /// The locked Menu's image, if replaced.
    pub locked: Option<Arc<[u8]>>,
    /// The unlocked Menu's image, if replaced.
    pub unlocked: Option<Arc<[u8]>>,
    /// The locked Menu's revision (0: never replaced).
    pub locked_revision: u32,
    /// The Deployment's revision salt, if drawn.
    pub salt: Option<String>,
}

impl MenuImages {
    /// The configured images: nothing replaced.
    pub fn configured() -> Self {
        Self::default()
    }
    /// With `image` for `slot`.
    pub fn with(mut self, slot: MenuSlot, image: impl Into<Arc<[u8]>>) -> Self {
        let image = Some(image.into());
        match slot {
            MenuSlot::Locked => self.locked = image,
            MenuSlot::Unlocked => self.unlocked = image,
        }
        self
    }
    /// With the locked Menu at `revision`, named with `salt`.
    pub fn with_locked_revision(mut self, revision: u32, salt: Option<String>) -> Self {
        self.locked_revision = revision;
        self.salt = salt;
        self
    }
    /// The replaced image of `slot`, if any.
    pub fn get(&self, slot: MenuSlot) -> Option<&[u8]> {
        match slot {
            MenuSlot::Locked => self.locked.as_deref(),
            MenuSlot::Unlocked => self.unlocked.as_deref(),
        }
    }
    /// Whether nothing is replaced or revised.
    pub fn is_configured(&self) -> bool {
        self.locked.is_none() && self.unlocked.is_none() && self.locked_revision == 0
    }
}

/// One tap area of a Menu drawn on an image, in image pixels.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct MenuArea {
    /// Left edge.
    pub x: u32,
    /// Top edge.
    pub y: u32,
    /// Width.
    pub width: u32,
    /// Height.
    pub height: u32,
    /// The entry's label (as the platform shows it).
    pub label: String,
}

impl MenuArea {
    /// An area.
    pub fn new(x: u32, y: u32, width: u32, height: u32, label: impl Into<String>) -> Self {
        Self {
            x,
            y,
            width,
            height,
            label: label.into(),
        }
    }
}

/// How a Menu lies on an image: its size and tap areas.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct MenuLayout {
    /// Image width, px.
    pub width: u32,
    /// Image height, px.
    pub height: u32,
    /// The tap areas, in Menu order.
    pub areas: Vec<MenuArea>,
}

impl MenuLayout {
    /// A layout.
    pub fn new(width: u32, height: u32, areas: Vec<MenuArea>) -> Self {
        Self {
            width,
            height,
            areas,
        }
    }
}

/// Why [`MenuArt::check`] refused an image; the Kernel words it for Staff.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum MenuImageProblem {
    /// Not an image of [`MenuArt::content_type`].
    NotImage,
    /// Larger than `max_bytes`.
    TooLarge {
        /// The limit, in bytes.
        max_bytes: usize,
    },
    /// A size the platform refuses.
    Size {
        /// Width of the image, px.
        width: u32,
        /// Height of the image, px.
        height: u32,
        /// The sizes it takes (width, height).
        allowed: Vec<(u32, u32)>,
    },
    /// The Menu's grid wants another size.
    Grid {
        /// Rows of the Menu's grid.
        rows: u32,
        /// The size it wants (width, height).
        want: (u32, u32),
    },
    /// The Menu has no entries, so the image is never shown.
    NoMenu,
}

/// Channel Capability: the Menu is drawn on an image the back office may
/// replace (#114).
#[async_trait]
pub trait MenuArt: Send + Sync {
    /// The configured image of `slot` (the default a reset returns to);
    /// empty when none.
    fn configured_image(&self, slot: MenuSlot) -> &[u8];

    /// MIME type of the images (e.g. `image/png`).
    fn content_type(&self) -> &'static str;

    /// Checks `image` for `slot` against the platform's rules and each
    /// Menu of `menus` (the unlocked Menu of every Locale; for
    /// [`MenuSlot::Locked`] the adapter checks its own locked Menu and
    /// ignores `menus`): each Menu's layout, for a preview that draws the
    /// tap areas, or why the platform would refuse the image or the Menu's
    /// grid does not fit it.
    fn check(
        &self,
        slot: MenuSlot,
        image: &[u8],
        menus: &[Menu],
    ) -> std::result::Result<Vec<MenuLayout>, MenuImageProblem>;

    /// This Channel drawing its Menus on `images` (the configured image
    /// where `None`). Shares the adapter's caches.
    fn drawn_on(&self, images: &MenuImages) -> Arc<dyn Channel>;

    /// Creates, before anything is saved, the platform objects `images`
    /// need: one per Menu of `unlocked` (each at the revision and with the
    /// salt it will be shown with) and, when `locked` is set, the locked
    /// Menu's. An error leaves nothing new behind that this call made, and
    /// no account is touched: the Kernel then saves nothing. The default
    /// stages nothing.
    async fn stage(&self, images: &MenuImages, unlocked: &[Menu], locked: bool) -> Result<()> {
        let _ = (images, unlocked, locked);
        Ok(())
    }
}

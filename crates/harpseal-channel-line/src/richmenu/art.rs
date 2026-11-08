//! Rich menu images replaced in the back office (#114): the
//! [`harpseal::MenuArt`] Capability of the LINE Channel.
//!
//! - `check`: [`super::png_size`]'s rules (PNG, 2500 x 1686 or 2500 x 843 px, at most
//!   1 MB, the #92 rules) and the Core's grid: the image must be the size
//!   [`render::menu`] gives the Menu (one row: 843 px high; more: 1686),
//!   so each tile lies under its tap area. The layout is
//!   [`render::menu_sized`] on that image, the object `set_menu` sends.
//! - `drawn_on`: a [`LineChannel`] whose configuration holds the replaced
//!   images (and, once the locked image was changed, the locked Menu at
//!   its revision with the Deployment's salt, so it is named
//!   `harpseal-locked-<generation>-r<revision>-<salt>-<hash>` and can be
//!   retired like an unlocked revision), sharing the rich menu cache.
//! - `stage`: creates each rich menu the new images need (list once, then
//!   create and upload what is missing) before the Kernel saves anything;
//!   on a failure it deletes what it created and returns the error.

use std::sync::Arc;

use async_trait::async_trait;
use harpseal::{
    Channel, Error, Menu, MenuArea, MenuArt, MenuImageProblem, MenuImages, MenuKey, MenuLayout,
    MenuSlot, Result,
};
use serde_json::Value;

use super::image::PNG_MAGIC;
use super::{IMAGE_MAX_BYTES, IMAGE_SIZES, keeper, rich_menu};
use crate::{LineChannel, LineConfig, render};

/// Most rich menus a LINE channel holds
/// (<https://developers.line.biz/en/reference/messaging-api/#create-rich-menu>).
pub(crate) const MAX_RICH_MENUS: usize = 1000;

impl LineChannel {
    /// This Channel's configuration with `images` in place.
    fn drawn_config(&self, images: &MenuImages) -> LineConfig {
        let c = &self.cfg;
        let image = |slot, configured: &Vec<u8>| {
            images
                .get(slot)
                .map_or_else(|| configured.clone(), <[u8]>::to_vec)
        };
        let mut locked_menu = c.locked_menu.clone();
        // Revisioned from the first change of the locked image on, the
        // configured one included after a reset, so every version can be
        // retired by revision.
        if images.locked_revision > 0
            && let Some(salt) = &images.salt
        {
            locked_menu = locked_menu
                .with_revision(images.locked_revision)
                .with_salt(salt.clone());
        }
        LineConfig {
            channel_secret: c.channel_secret.clone(),
            access_token: c.access_token.clone(),
            liff_id: c.liff_id.clone(),
            login_channel_id: c.login_channel_id.clone(),
            launch_base: c.launch_base.clone(),
            locked_menu,
            locked_menu_image: image(MenuSlot::Locked, &c.locked_menu_image),
            unlocked_menu_image: image(MenuSlot::Unlocked, &c.unlocked_menu_image),
            menu_generation: c.menu_generation,
        }
    }

    fn drawn(&self, images: &MenuImages) -> LineChannel {
        LineChannel {
            cfg: self.drawn_config(images),
            http: self.http.clone(),
            menus: self.menus.clone(),
        }
    }

    /// The (name, object, image) of each rich menu to stage.
    fn staged(&self, unlocked: &[Menu], locked: bool) -> Result<Vec<(String, Value, &[u8])>> {
        let generation = self.cfg.menu_generation();
        let mut out = Vec::new();
        for m in unlocked.iter().filter(|m| !m.entries.is_empty()) {
            let Some(key) = m.key() else { continue };
            let image = self.cfg.unlocked_menu_image.as_slice();
            let (name, obj) = rich_menu(Some((key, generation)), m, image)?;
            out.push((name, obj, image));
        }
        if locked && !self.cfg.locked_menu.entries.is_empty() {
            let image = self.cfg.locked_menu_image.as_slice();
            let tag = (&MenuKey::locked(), generation);
            let (name, obj) = rich_menu(Some(tag), &self.cfg.locked_menu, image)?;
            out.push((name, obj, image));
        }
        Ok(out)
    }
}

/// The layout of `menu` on an image of `w` x `h`, refused when the
/// Core's grid wants another size.
fn layout(menu: &Menu, w: usize, h: usize) -> std::result::Result<MenuLayout, MenuImageProblem> {
    let want = render::menu(menu);
    let want_h = want["size"]["height"].as_u64().unwrap_or(0) as usize;
    if want_h != h {
        return Err(MenuImageProblem::Grid {
            rows: row_count(&want),
            want: (2500, want_h as u32),
        });
    }
    let obj = render::menu_sized(menu, w, h);
    let n = |v: &Value| v.as_u64().unwrap_or(0) as u32;
    let areas = obj["areas"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|a| {
            let b = &a["bounds"];
            let label = a["action"]["label"].as_str().unwrap_or_default();
            MenuArea::new(
                n(&b["x"]),
                n(&b["y"]),
                n(&b["width"]),
                n(&b["height"]),
                label,
            )
        })
        .collect();
    Ok(MenuLayout::new(w as u32, h as u32, areas))
}

/// Distinct rows of a rich menu object's areas.
fn row_count(obj: &Value) -> u32 {
    let mut ys: Vec<u64> = obj["areas"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|a| a["bounds"]["y"].as_u64())
        .collect();
    ys.sort_unstable();
    ys.dedup();
    ys.len() as u32
}

/// [`super::png_size`]'s rules, saying which one `image` breaks.
fn png(image: &[u8]) -> std::result::Result<(usize, usize), MenuImageProblem> {
    if image.len() > IMAGE_MAX_BYTES {
        return Err(MenuImageProblem::TooLarge {
            max_bytes: IMAGE_MAX_BYTES,
        });
    }
    if image.len() < 24 || &image[..8] != PNG_MAGIC || &image[12..16] != b"IHDR" {
        return Err(MenuImageProblem::NotImage);
    }
    let be = |b: &[u8]| u32::from_be_bytes([b[0], b[1], b[2], b[3]]);
    let (w, h) = (be(&image[16..20]), be(&image[20..24]));
    if !IMAGE_SIZES.contains(&(w as usize, h as usize)) {
        return Err(MenuImageProblem::Size {
            width: w,
            height: h,
            allowed: IMAGE_SIZES
                .iter()
                .map(|(w, h)| (*w as u32, *h as u32))
                .collect(),
        });
    }
    Ok((w as usize, h as usize))
}

#[async_trait]
impl MenuArt for LineChannel {
    fn configured_image(&self, slot: MenuSlot) -> &[u8] {
        match slot {
            MenuSlot::Locked => &self.cfg.locked_menu_image,
            _ => &self.cfg.unlocked_menu_image,
        }
    }

    fn content_type(&self) -> &'static str {
        "image/png"
    }

    fn check(
        &self,
        slot: MenuSlot,
        image: &[u8],
        menus: &[Menu],
    ) -> std::result::Result<Vec<MenuLayout>, MenuImageProblem> {
        let (w, h) = png(image)?;
        let locked = [self.cfg.locked_menu.clone()];
        let menus = match slot {
            MenuSlot::Locked => &locked[..],
            _ => menus,
        };
        let shown: Vec<&Menu> = menus.iter().filter(|m| !m.entries.is_empty()).collect();
        if shown.is_empty() {
            return Err(MenuImageProblem::NoMenu);
        }
        shown.into_iter().map(|m| layout(m, w, h)).collect()
    }

    fn drawn_on(&self, images: &MenuImages) -> Arc<dyn Channel> {
        Arc::new(self.drawn(images))
    }

    async fn stage(&self, images: &MenuImages, unlocked: &[Menu], locked: bool) -> Result<()> {
        let view = self.drawn(images);
        let wanted = view.staged(unlocked, locked)?;
        if wanted.is_empty() {
            return Ok(());
        }
        let list = view.rich_menu_list().await?;
        let missing = wanted
            .iter()
            .filter(|(n, _, _)| keeper(&list, n).is_none())
            .count();
        if list.len() + missing > MAX_RICH_MENUS {
            // Refused before creating any: a half-staged change would leave
            // some Locales without their new rich menu.
            return Err(Error::Platform(format!(
                "LINE holds {} rich menus; {missing} more would pass its limit of \
                 {MAX_RICH_MENUS}: delete unused ones (or bump menu_generation) first",
                list.len()
            )));
        }
        let mut made: Vec<(String, String)> = Vec::new();
        for (name, obj, image) in &wanted {
            if let Some(id) = keeper(&list, name) {
                view.menus.put(name, &id);
                continue;
            }
            match view.create(name, obj, image).await {
                Ok(id) => made.push((name.clone(), id)),
                Err(e) => {
                    // Nothing new is left behind: no account was linked to
                    // these yet.
                    for (n, id) in &made {
                        view.menus.forget(n);
                        view.delete_rich_menu(id).await;
                    }
                    return Err(e);
                }
            }
        }
        Ok(())
    }
}

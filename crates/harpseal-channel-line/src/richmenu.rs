//! Rich menus (<https://developers.line.biz/en/reference/messaging-api/#rich-menu>).
//!
//! `set_menu(to, Some(menu))` shows the unlocked Menu, `set_menu(to, None)`
//! the locked one from [`crate::LineConfig::locked_menu`]. Each is a rich
//! menu object built from the Menu ([`render::menu_sized`], areas are
//! postbacks whose `data` is the ActionId) on the configured PNG image. The
//! object and the image are hashed; the rich menu is named
//! `harpseal-<hash>`, so the same Menu and image reuse one rich menu:
//!
//! 1. the in-memory cache (hash -> rich menu id), else
//! 2. `GET /v2/bot/richmenu/list`, looking for that name, else
//! 3. `POST /v2/bot/richmenu`, then upload the image
//!    (`POST api-data.line.me/v2/bot/richmenu/{id}/content`);
//!
//! then `POST /v2/bot/user/{userId}/richmenu/{richMenuId}` links it. On
//! Workers the cache lives per isolate, so the list call is the source of
//! truth. A Menu without entries (or an empty `locked_menu`) unlinks
//! instead (`DELETE /v2/bot/user/{userId}/richmenu`): the account then sees
//! the default rich menu set in the LINE Official Account Manager, if any.
//!
//! Images: PNG, 2500 x 1686 or 2500 x 843 px, at most 1 MB
//! (<https://developers.line.biz/en/reference/messaging-api/#upload-rich-menu-image-requirements>).

use std::collections::HashMap;
use std::sync::Mutex;

use harpseal::{ChannelAccountId, Error, Menu, Result};
use http::Method;
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::api::{API, API_DATA, Failure, parse, path_id};
use crate::{LineChannel, render};

/// Largest rich menu image.
pub const IMAGE_MAX_BYTES: usize = 1_048_576;
/// The image sizes accepted (width, height).
pub const IMAGE_SIZES: [(usize, usize); 2] = [(2500, 1686), (2500, 843)];
const PNG_MAGIC: &[u8; 8] = b"\x89PNG\r\n\x1a\n";
const NAME_PREFIX: &str = "harpseal-";

/// Rich menu ids by content hash, for this process (or Worker isolate).
#[derive(Debug, Default)]
pub(crate) struct MenuCache(Mutex<HashMap<String, String>>);

impl MenuCache {
    fn get(&self, hash: &str) -> Option<String> {
        self.0
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .get(hash)
            .cloned()
    }
    fn put(&self, hash: String, id: String) {
        self.0
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .insert(hash, id);
    }
}

/// Width and height of a PNG within LINE's limits, or why not.
pub fn png_size(image: &[u8]) -> Result<(usize, usize)> {
    let bad = |why: &str| Error::Invalid(format!("LINE rich menu image: {why}"));
    if image.is_empty() {
        return Err(bad("not configured"));
    }
    if image.len() > IMAGE_MAX_BYTES {
        return Err(bad("over 1 MB"));
    }
    if image.len() < 24 || &image[..8] != PNG_MAGIC || &image[12..16] != b"IHDR" {
        return Err(bad("not a PNG"));
    }
    let be = |b: &[u8]| u32::from_be_bytes([b[0], b[1], b[2], b[3]]) as usize;
    let size = (be(&image[16..20]), be(&image[20..24]));
    if !IMAGE_SIZES.contains(&size) {
        return Err(bad(&format!(
            "{} x {} px, must be 2500 x 1686 or 2500 x 843",
            size.0, size.1
        )));
    }
    Ok(size)
}

/// The rich menu object for `menu` on `image`, named by its hash.
pub fn rich_menu(menu: &Menu, image: &[u8]) -> Result<(String, Value)> {
    let (w, h) = png_size(image)?;
    let mut obj = render::menu_sized(menu, w, h);
    let mut hasher = Sha256::new();
    hasher.update(obj.to_string().as_bytes());
    hasher.update(image);
    let hash: String = hasher.finalize()[..16]
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    obj["name"] = Value::String(format!("{NAME_PREFIX}{hash}"));
    Ok((hash, obj))
}

fn fail(f: Failure) -> Error {
    f.into_error(false)
}

impl LineChannel {
    pub(crate) async fn rich_menu_set(
        &self,
        to: &ChannelAccountId,
        menu: Option<&Menu>,
    ) -> Result<()> {
        let user = path_id(&to.external_id)?;
        let (menu, image) = match menu {
            Some(m) => (m, &self.cfg.unlocked_menu_image),
            None => (&self.cfg.locked_menu, &self.cfg.locked_menu_image),
        };
        if menu.entries.is_empty() {
            let url = format!("{API}/v2/bot/user/{user}/richmenu");
            return self
                .call(Method::DELETE, &url, None, Vec::new())
                .await?
                .map(drop)
                .map_err(fail);
        }
        let (hash, obj) = rich_menu(menu, image)?;
        let id = self.rich_menu_id(&hash, &obj, image).await?;
        let url = format!("{API}/v2/bot/user/{user}/richmenu/{}", path_id(&id)?);
        self.call(Method::POST, &url, None, Vec::new())
            .await?
            .map(drop)
            .map_err(fail)
    }

    async fn rich_menu_id(&self, hash: &str, obj: &Value, image: &[u8]) -> Result<String> {
        if let Some(id) = self.menus.get(hash) {
            return Ok(id);
        }
        if let Some(id) = self.find_rich_menu(&obj["name"]).await? {
            self.menus.put(hash.to_string(), id.clone());
            return Ok(id);
        }
        let created = self
            .post_json(&format!("{API}/v2/bot/richmenu"), obj)
            .await?
            .map_err(fail)?;
        let id = created["richMenuId"]
            .as_str()
            .filter(|s| !s.is_empty())
            .ok_or_else(|| Error::Platform("LINE created a rich menu without an id".into()))?
            .to_string();
        let url = format!("{API_DATA}/v2/bot/richmenu/{}/content", path_id(&id)?);
        let uploaded = self
            .call(Method::POST, &url, Some("image/png"), image.to_vec())
            .await;
        if let Err(e) = uploaded.and_then(|r| r.map_err(fail)) {
            // A rich menu without an image cannot be linked: remove it so
            // the list lookup never finds it.
            let del = format!("{API}/v2/bot/richmenu/{id}");
            let _ = self.call(Method::DELETE, &del, None, Vec::new()).await;
            return Err(e);
        }
        self.menus.put(hash.to_string(), id.clone());
        Ok(id)
    }

    async fn find_rich_menu(&self, name: &Value) -> Result<Option<String>> {
        let url = format!("{API}/v2/bot/richmenu/list");
        let body = self
            .call(Method::GET, &url, None, Vec::new())
            .await?
            .map_err(fail)?;
        let list = parse(&body);
        let found = list["richmenus"]
            .as_array()
            .into_iter()
            .flatten()
            .find(|m| &m["name"] == name)
            .and_then(|m| m["richMenuId"].as_str())
            .map(str::to_string);
        Ok(found)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    pub(crate) fn png(w: u32, h: u32) -> Vec<u8> {
        let mut v = PNG_MAGIC.to_vec();
        v.extend([0, 0, 0, 13]);
        v.extend(b"IHDR");
        v.extend(w.to_be_bytes());
        v.extend(h.to_be_bytes());
        v.extend([8, 6, 0, 0, 0]);
        v
    }

    #[test]
    fn images_must_be_png_of_the_documented_size() {
        assert_eq!(png_size(&png(2500, 1686)).unwrap(), (2500, 1686));
        assert_eq!(png_size(&png(2500, 843)).unwrap(), (2500, 843));
        assert!(png_size(&png(1200, 810)).is_err());
        assert!(png_size(&[]).is_err());
        assert!(png_size(b"GIF89a................................").is_err());
        let mut big = png(2500, 843);
        big.resize(IMAGE_MAX_BYTES + 1, 0);
        assert!(png_size(&big).is_err());
    }

    #[test]
    fn the_name_changes_with_the_menu_or_the_image() {
        let menu = harpseal::fixtures::menu_unlocked();
        let (a, obj) = rich_menu(&menu, &png(2500, 1686)).unwrap();
        assert_eq!(obj["name"], format!("harpseal-{a}"));
        assert_eq!(obj["size"]["height"], 1686);
        let (b, _) = rich_menu(&menu, &png(2500, 843)).unwrap();
        let (c, _) = rich_menu(&harpseal::fixtures::menu_locked(), &png(2500, 1686)).unwrap();
        assert_ne!(a, b);
        assert_ne!(a, c);
        assert_eq!(rich_menu(&menu, &png(2500, 1686)).unwrap().0, a);
    }
}

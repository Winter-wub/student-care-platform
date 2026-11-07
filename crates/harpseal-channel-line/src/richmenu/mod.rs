//! Rich menus (<https://developers.line.biz/en/reference/messaging-api/#rich-menu>).
//!
//! `set_menu(to, Some(menu))` shows the unlocked Menu, `set_menu(to, None)`
//! the locked one from [`crate::LineConfig::locked_menu`]. Each is a rich
//! menu object built from the Menu ([`render::menu_sized`], areas are
//! postbacks whose `data` is the ActionId) on the configured PNG image.
//!
//! # Names
//!
//! The object and the image are hashed (`<hash>`, 32 hex). A Menu with a
//! *Menu key* is named `harpseal-<key>-<hash>`: the locked Menu has key
//! [`LOCKED_KEY`], an unlocked Menu the key it was registered under with
//! [`crate::LineChannel::with_menus`] (e.g. its Locale, `th`). A Menu that
//! was not registered (a label changed by a setting override, say) is
//! named `harpseal-<hash>` and is never cleaned up. The same Menu and
//! image reuse one rich menu:
//!
//! 1. the in-memory cache (name -> rich menu id), else
//! 2. `GET /v2/bot/richmenu/list`, looking for that name (with several,
//!    the smallest id), else
//! 3. `POST /v2/bot/richmenu`, then upload the image
//!    (`POST api-data.line.me/v2/bot/richmenu/{id}/content`);
//!
//! then `POST /v2/bot/user/{userId}/richmenu/{richMenuId}` links it. On
//! Workers the cache lives per isolate, so the list call is the source of
//! truth. A Menu without entries (or an empty `locked_menu`) unlinks
//! instead (`DELETE /v2/bot/user/{userId}/richmenu`): the account then sees
//! the default rich menu set in the LINE Official Account Manager, if any.
//!
//! # Stale ids
//!
//! A link answered 404 (the rich menu was deleted, e.g. in the console)
//! drops the cache entry and runs steps 2-3 again, without the cache, then
//! links once more. A second 404 is `Error::Platform`: one recreate per
//! call at most.
//!
//! # Cleanup
//!
//! A channel holds at most 1000 rich menus
//! (<https://developers.line.biz/en/reference/messaging-api/#create-rich-menu>).
//! Versions pile up when a Menu changes, so after a successful link that
//! went through the list (cold cache), the adapter lists the rich menus
//! (once more only when it has just created one) and deletes, at most
//! [`MAX_DELETES`] per call:
//!
//! - duplicates: the other rich menus with the same name (two cold
//!   `set_menu` calls in two isolates can both create one). Every caller
//!   keeps the smallest id, re-linking its account to it first;
//! - superseded versions: `harpseal-<key>-<other hash>` with the same Menu
//!   key as the Menu just linked.
//!
//! Rich menus without a key, with a key this adapter does not know, or not
//! named `harpseal-*` are left alone (another Deployment or a person may
//! own them). A failed delete never fails `set_menu`.
//!
//! Residual risk: LINE cannot list the accounts linked to a rich menu, so
//! an account still linked to a superseded version when it is deleted sees
//! the default rich menu (or none) until its next `set_menu`.
//!
//! Images: PNG, 2500 x 1686 or 2500 x 843 px, at most 1 MB
//! (<https://developers.line.biz/en/reference/messaging-api/#upload-rich-menu-image-requirements>).

mod tidy;

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
/// Every rich menu this adapter creates is named with this prefix.
pub const NAME_PREFIX: &str = "harpseal-";
/// The Menu key of the locked Menu.
pub const LOCKED_KEY: &str = "locked";
/// Most rich menus deleted by one `set_menu`.
pub const MAX_DELETES: usize = 20;
/// Longest Menu key.
pub const KEY_MAX_LEN: usize = 16;
const PNG_MAGIC: &[u8; 8] = b"\x89PNG\r\n\x1a\n";

/// Rich menu ids by name, and the registered Menu keys by content hash,
/// for this process (or Worker isolate); shared by clones of the channel.
#[derive(Debug, Default)]
pub(crate) struct MenuCache(Mutex<Cached>);

#[derive(Debug, Default)]
struct Cached {
    ids: HashMap<String, String>,
    keys: HashMap<String, String>,
}

impl MenuCache {
    fn lock(&self) -> std::sync::MutexGuard<'_, Cached> {
        self.0.lock().unwrap_or_else(|p| p.into_inner())
    }
    fn get(&self, name: &str) -> Option<String> {
        self.lock().ids.get(name).cloned()
    }
    fn put(&self, name: &str, id: &str) {
        self.lock().ids.insert(name.into(), id.into());
    }
    fn forget(&self, name: &str) {
        self.lock().ids.remove(name);
    }
    fn key(&self, hash: &str) -> Option<String> {
        self.lock().keys.get(hash).cloned()
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

/// The content hash of `menu` on `image` and its rich menu object (not
/// yet named).
fn hashed(menu: &Menu, image: &[u8]) -> Result<(String, Value)> {
    let (w, h) = png_size(image)?;
    let obj = render::menu_sized(menu, w, h);
    let mut hasher = Sha256::new();
    hasher.update(obj.to_string().as_bytes());
    hasher.update(image);
    let hash = hasher.finalize()[..16]
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    Ok((hash, obj))
}

/// The rich menu object for `menu` on `image` under Menu key `key`, and
/// its name: `harpseal-<key>-<hash>`, or `harpseal-<hash>` without a key.
pub fn rich_menu(key: Option<&str>, menu: &Menu, image: &[u8]) -> Result<(String, Value)> {
    let (hash, mut obj) = hashed(menu, image)?;
    let name = match key {
        Some(k) => format!("{NAME_PREFIX}{}-{hash}", valid_key(k)?),
        None => format!("{NAME_PREFIX}{hash}"),
    };
    obj["name"] = Value::String(name.clone());
    Ok((name, obj))
}

/// The Menu key of a rich menu name: `Some(Some(key))` for
/// `harpseal-<key>-<hash>`, `Some(None)` for `harpseal-<hash>`, `None` for
/// a name this adapter did not make.
pub fn name_key(name: &str) -> Option<Option<&str>> {
    let rest = name.strip_prefix(NAME_PREFIX)?;
    match rest.rsplit_once('-') {
        Some((key, _)) => valid_key(key).ok().map(Some),
        None => Some(None),
    }
}

/// A Menu key: 1 to [`KEY_MAX_LEN`] lowercase ASCII letters and digits.
fn valid_key(key: &str) -> Result<&str> {
    let ok = !key.is_empty()
        && key.len() <= KEY_MAX_LEN
        && key
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit());
    if ok {
        Ok(key)
    } else {
        Err(Error::Invalid(format!(
            "LINE Menu key {key:?}: 1 to {KEY_MAX_LEN} of a-z and 0-9"
        )))
    }
}

pub(crate) fn fail(f: Failure) -> Error {
    f.into_error(false)
}

/// Where the id to link came from.
struct Found {
    id: String,
    /// The list fetched to find (or not find) it; `None` from the cache.
    list: Option<Vec<Listed>>,
    /// Created by this call.
    created: bool,
}

/// One entry of `GET /v2/bot/richmenu/list`.
#[derive(Debug, Clone)]
struct Listed {
    id: String,
    name: String,
}

impl LineChannel {
    /// Registers the unlocked Menus in use, each under a Menu key (its
    /// Locale, say: `[("th", d.menu_for(&Locale::th())), ...]`), so that
    /// their superseded rich menus are deleted (see [`crate::richmenu`]).
    /// Replaces any earlier registration; shared with every clone. Keys are
    /// 1 to [`KEY_MAX_LEN`] of `a-z0-9`, never [`LOCKED_KEY`].
    pub fn with_menus<K: Into<String>>(
        &self,
        menus: impl IntoIterator<Item = (K, Menu)>,
    ) -> Result<&Self> {
        let mut keys = HashMap::new();
        for (key, menu) in menus {
            let key = key.into();
            if valid_key(&key)? == LOCKED_KEY {
                return Err(Error::Invalid(format!(
                    "LINE Menu key {LOCKED_KEY:?} is the locked Menu's"
                )));
            }
            let (hash, _) = hashed(&menu, &self.cfg.unlocked_menu_image)?;
            keys.insert(hash, key);
        }
        self.menus.lock().keys = keys;
        Ok(self)
    }

    pub(crate) async fn rich_menu_set(
        &self,
        to: &ChannelAccountId,
        menu: Option<&Menu>,
    ) -> Result<()> {
        let user = path_id(&to.external_id)?;
        let (menu, image, key) = match menu {
            Some(m) => {
                let (hash, _) = hashed(m, &self.cfg.unlocked_menu_image)?;
                (m, &self.cfg.unlocked_menu_image, self.menus.key(&hash))
            }
            None => (
                &self.cfg.locked_menu,
                &self.cfg.locked_menu_image,
                Some(LOCKED_KEY.to_string()),
            ),
        };
        if menu.entries.is_empty() {
            let url = format!("{API}/v2/bot/user/{user}/richmenu");
            return self
                .call(Method::DELETE, &url, None, Vec::new())
                .await?
                .map(drop)
                .map_err(fail);
        }
        let (name, obj) = rich_menu(key.as_deref(), menu, image)?;
        let mut found = self.rich_menu_id(&name, &obj, image, true).await?;
        if let Err(f) = self.link(user, &found.id).await? {
            if f.status != 404 {
                return Err(fail(f));
            }
            // Deleted behind our back: find or create it again, once.
            self.menus.forget(&name);
            found = self.rich_menu_id(&name, &obj, image, false).await?;
            self.link(user, &found.id).await?.map_err(fail)?;
        }
        self.tidy(user, key.as_deref(), &name, found).await;
        Ok(())
    }

    /// Links rich menu `id` to `user`.
    async fn link(&self, user: &str, id: &str) -> Result<std::result::Result<(), Failure>> {
        let url = format!("{API}/v2/bot/user/{user}/richmenu/{}", path_id(id)?);
        Ok(self
            .call(Method::POST, &url, None, Vec::new())
            .await?
            .map(drop))
    }

    async fn rich_menu_id(
        &self,
        name: &str,
        obj: &Value,
        image: &[u8],
        cached: bool,
    ) -> Result<Found> {
        if let Some(id) = self.menus.get(name).filter(|_| cached) {
            return Ok(Found {
                id,
                list: None,
                created: false,
            });
        }
        let list = self.rich_menu_list().await?;
        if let Some(id) = keeper(&list, name) {
            self.menus.put(name, &id);
            return Ok(Found {
                id,
                list: Some(list),
                created: false,
            });
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
            self.delete_rich_menu(&id).await;
            return Err(e);
        }
        self.menus.put(name, &id);
        Ok(Found {
            id,
            list: Some(list),
            created: true,
        })
    }

    async fn rich_menu_list(&self) -> Result<Vec<Listed>> {
        let url = format!("{API}/v2/bot/richmenu/list");
        let body = self
            .call(Method::GET, &url, None, Vec::new())
            .await?
            .map_err(fail)?;
        let list = parse(&body);
        Ok(list["richmenus"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|m| {
                Some(Listed {
                    id: m["richMenuId"].as_str()?.to_string(),
                    name: m["name"].as_str()?.to_string(),
                })
            })
            .collect())
    }

    /// `DELETE /v2/bot/richmenu/{id}`; whether LINE accepted it.
    async fn delete_rich_menu(&self, id: &str) -> bool {
        let Ok(id) = path_id(id) else { return false };
        let url = format!("{API}/v2/bot/richmenu/{id}");
        matches!(
            self.call(Method::DELETE, &url, None, Vec::new()).await,
            Ok(Ok(_))
        )
    }
}

/// The rich menu every caller keeps among those named `name`: the
/// smallest id, so that racing isolates agree.
fn keeper(list: &[Listed], name: &str) -> Option<String> {
    list.iter()
        .filter(|m| m.name == name)
        .map(|m| &m.id)
        .min()
        .cloned()
}

#[cfg(test)]
mod tests;

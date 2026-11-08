//! Branding files in the FileStore, content-addressed, and the Menu
//! images the Kernel draws every Menu on (#114).

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use sha2::{Digest, Sha256};

use super::{LOCKED_KEY, LOCKED_REVISION_KEY, UNLOCKED_KEY};
use crate::channel::{MenuImages, MenuSlot};
use crate::port_types::{Statement, Value};
use crate::{Channel, Deployment, Result, Services};

/// Lowercase hex SHA-256 of `bytes`.
pub(crate) fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// The FileStore key of `bytes` under `dir` (`branding` or
/// `branding/menu`), with extension `ext`.
pub(crate) fn key_of(dir: &str, bytes: &[u8], ext: &str) -> String {
    format!("{dir}/{}.{ext}", sha256_hex(bytes))
}

/// Stores `bytes` under its content key; the key.
pub(crate) async fn put(
    s: &Services,
    dir: &str,
    bytes: &[u8],
    ext: &str,
    content_type: &str,
) -> Result<String> {
    let key = key_of(dir, bytes, ext);
    s.files.put(&key, bytes.to_vec(), content_type).await?;
    Ok(key)
}

/// Deletes each of `keys` that no `setting` row names any more (a save
/// replaced or reset it). An older backup holds its own copy. A failure
/// only leaks the file.
pub(crate) async fn forget(s: &Services, keys: &[String]) {
    for key in keys {
        let named =
            s.db.query(
                Statement::new("SELECT 1 FROM setting WHERE value = ? LIMIT 1").bind(key.as_str()),
            )
            .await;
        if matches!(named, Ok(r) if r.is_empty()) {
            let _ = s.files.delete(key).await;
        }
    }
}

/// Image bytes by FileStore key, and Channels drawn on them, for this
/// process (keys are content addresses, so an entry never goes stale).
#[derive(Default)]
pub(crate) struct ImageCache {
    bytes: Mutex<HashMap<String, Arc<[u8]>>>,
    drawn: Mutex<HashMap<String, Arc<dyn Channel>>>,
}

/// The cache keeps at most this many images and views; past it, it starts
/// over (a few saves a year).
const CACHE_MAX: usize = 16;

impl ImageCache {
    /// The bytes of `key`, read once per process; `None` when the object
    /// is missing (the configured image is drawn instead).
    pub(crate) async fn get(&self, s: &Services, key: &str) -> Result<Option<Arc<[u8]>>> {
        let lock = || self.bytes.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(b) = lock().get(key) {
            return Ok(Some(b.clone()));
        }
        let Some(bytes) = s.files.get(key).await? else {
            return Ok(None);
        };
        let bytes: Arc<[u8]> = bytes.into();
        let mut map = lock();
        if map.len() >= CACHE_MAX {
            map.clear();
        }
        map.insert(key.to_string(), bytes.clone());
        Ok(Some(bytes))
    }

    fn view(&self, sig: &str, make: impl FnOnce() -> Arc<dyn Channel>) -> Arc<dyn Channel> {
        let mut map = self.drawn.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(c) = map.get(sig) {
            return c.clone();
        }
        if map.len() >= CACHE_MAX {
            map.clear();
        }
        let c = make();
        map.insert(sig.to_string(), c.clone());
        c
    }
}

/// The saved Menu images as stored: FileStore keys, the locked revision
/// and the Deployment's revision salt. Read fresh (one query): a Menu set
/// right after a save must draw the new image under the new revision.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Saved {
    pub(crate) locked: Option<String>,
    pub(crate) unlocked: Option<String>,
    pub(crate) locked_revision: u32,
    pub(crate) salt: Option<String>,
}

impl Saved {
    pub(crate) async fn load(s: &Services) -> Result<Self> {
        let rows =
            s.db.query(
                Statement::new(
                    "SELECT key, value FROM setting WHERE locale = '' AND key IN (?, ?, ?) \
                     UNION ALL SELECT 'salt', salt FROM menu_salt",
                )
                .bind(LOCKED_KEY)
                .bind(UNLOCKED_KEY)
                .bind(LOCKED_REVISION_KEY),
            )
            .await?;
        let mut out = Saved::default();
        for i in 0..rows.len() {
            let text = |c: &str| rows.get(i, c).and_then(Value::as_text).map(str::to_string);
            let (Some(k), Some(v)) = (text("key"), text("value")) else {
                continue;
            };
            match k.as_str() {
                LOCKED_KEY => out.locked = Some(v),
                UNLOCKED_KEY => out.unlocked = Some(v),
                LOCKED_REVISION_KEY => out.locked_revision = v.parse().unwrap_or(0),
                _ => out.salt = Some(v),
            }
        }
        Ok(out)
    }

    /// The key saved for `slot`.
    pub(crate) fn key(&self, slot: MenuSlot) -> Option<&str> {
        match slot {
            MenuSlot::Locked => self.locked.as_deref(),
            _ => self.unlocked.as_deref(),
        }
    }

    /// Whether every Channel draws its configured images, unrevised.
    pub(crate) fn is_configured(&self) -> bool {
        self.locked.is_none() && self.unlocked.is_none() && self.locked_revision == 0
    }

    /// The images to hand a Channel.
    pub(crate) async fn images(&self, d: &Deployment) -> Result<MenuImages> {
        let cache = &d.branding().images;
        let s = d.services();
        let mut images =
            MenuImages::configured().with_locked_revision(self.locked_revision, self.salt.clone());
        for slot in [MenuSlot::Locked, MenuSlot::Unlocked] {
            if let Some(key) = self.key(slot)
                && let Some(bytes) = cache.get(s, key).await?
            {
                images = images.with(slot, bytes);
            }
        }
        Ok(images)
    }

    fn sig(&self, kind: &str) -> String {
        format!(
            "{kind}|{}|{}|{}|{}",
            self.locked.as_deref().unwrap_or_default(),
            self.unlocked.as_deref().unwrap_or_default(),
            self.locked_revision,
            self.salt.as_deref().unwrap_or_default()
        )
    }
}

/// `ch` drawing its Menus on the saved images: `None` when nothing was
/// ever saved or `ch` has no Menu images (use `ch` as it is).
pub(crate) async fn drawn(d: &Deployment, ch: &dyn Channel) -> Result<Option<Arc<dyn Channel>>> {
    let Some(art) = ch.menu_art() else {
        return Ok(None);
    };
    let saved = Saved::load(d.services()).await?;
    if saved.is_configured() {
        return Ok(None);
    }
    drawn_as(d, ch, art, &saved).await.map(Some)
}

/// `ch` drawn on `saved`.
pub(crate) async fn drawn_as(
    d: &Deployment,
    ch: &dyn Channel,
    art: &dyn crate::MenuArt,
    saved: &Saved,
) -> Result<Arc<dyn Channel>> {
    let images = saved.images(d).await?;
    let sig = saved.sig(ch.kind().as_str());
    Ok(d.branding().images.view(&sig, || art.drawn_on(&images)))
}

/// Every Channel of the Deployment, each drawn on the saved images.
pub(crate) async fn drawn_all(d: &Deployment) -> Result<Vec<Arc<dyn Channel>>> {
    let saved = Saved::load(d.services()).await?;
    let mut out = Vec::new();
    for ch in d.channels() {
        out.push(match ch.menu_art() {
            Some(art) if !saved.is_configured() => drawn_as(d, ch.as_ref(), art, &saved).await?,
            _ => ch.clone(),
        });
    }
    Ok(out)
}

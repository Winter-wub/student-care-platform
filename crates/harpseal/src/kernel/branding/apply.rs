//! Saving the Menu images (#114): check, stage on the platform, then one
//! batch.
//!
//! 1. Each new image is checked by every Channel with Menu images
//!    ([`crate::MenuArt::check`]) against every Locale's Menu.
//! 2. The new images go to the FileStore under their content keys, and the
//!    revision salt is stored (if there is none) and read back, so the
//!    staged names and every later retire agree.
//! 3. Each Channel stages the platform objects the change needs
//!    ([`crate::MenuArt::stage`]: on LINE the rich menus of the next
//!    revision of every Locale's unlocked Menu, and of the locked Menu's
//!    next revision). A failure deletes the new files no setting names and
//!    returns the error: nothing saved, nothing audited, every account on
//!    the Menu it had.
//! 4. One batch, gated on its audit row (`branding.menu_images`, written
//!    only while the settings still hold what step 1 read): the settings,
//!    the next Menu revision of every Locale and its re-link Job
//!    (ADR-0011, when the unlocked image changed), the locked revision and
//!    the [`super::relock::RELOCK`] Job (when the locked image changed).
//! 5. The files the save replaced are deleted once no setting names them.
//!
//! A reset is the same with the configured images (no file): every
//! account is re-linked to them, and the replaced versions are retired.

use std::collections::HashMap;

use serde_json::json;

use super::relock::{RELOCK, payload};
use super::store::{self, Saved};
use super::{LOCKED_KEY, LOCKED_REVISION_KEY, UNLOCKED_KEY};
use crate::audit::{self, Actor, RequestCtx, Target};
use crate::channel::{MenuImageProblem, MenuImages, MenuSlot};
use crate::kernel::menu;
use crate::kernel::text::Texts;
use crate::message::{Locale, Menu};
use crate::port_types::{Statement, Value};
use crate::{Deployment, Error, Result};

/// Where the Menu image files go.
pub(crate) const MENU_DIR: &str = "branding/menu";

/// What a save of the Menu images did.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum MenuImagesSaved {
    /// Saved; the re-link Jobs are queued.
    Saved,
    /// The images are those saved already: nothing done.
    Unchanged,
    /// Another save came first: nothing done.
    Stale,
    /// A Channel refused an image: nothing done.
    Refused(Vec<(MenuSlot, MenuImageProblem)>),
}

/// The new images: `None` keeps the saved (or configured) one; `reset`
/// returns both to the configured images.
pub(crate) struct Change {
    pub(crate) locked: Option<Vec<u8>>,
    pub(crate) unlocked: Option<Vec<u8>>,
    pub(crate) reset: bool,
}

/// Every Locale's unlocked Menu with the saved Menu labels.
pub(crate) async fn menus(d: &Deployment) -> Result<(Texts, Vec<(Locale, Menu)>)> {
    let catalog = d.catalog_arc();
    let texts = Texts::load(d.services(), catalog.clone(), catalog.default_locale()).await?;
    let menus = catalog
        .locales()
        .into_iter()
        .map(|l| {
            let t = texts.in_locale(&l);
            let m = d.menu_with(&l, |k| t.t(k));
            (l, m)
        })
        .collect();
    Ok((texts, menus))
}

/// What every Channel with Menu images says of `image` for `slot`.
pub(crate) fn check(
    d: &Deployment,
    slot: MenuSlot,
    image: &[u8],
    menus: &[Menu],
) -> std::result::Result<Vec<crate::MenuLayout>, MenuImageProblem> {
    let mut layouts = Vec::new();
    for art in d.channels().iter().filter_map(|c| c.menu_art()) {
        layouts.extend(art.check(slot, image, menus)?);
    }
    Ok(layouts)
}

/// Checks, stages and saves `change` (the module docs).
pub(crate) async fn save(
    d: &Deployment,
    change: Change,
    actor: &Actor,
    ctx: &RequestCtx,
) -> Result<MenuImagesSaved> {
    let s = d.services();
    let arts: Vec<_> = d.channels().iter().filter_map(|c| c.menu_art()).collect();
    let Some(first) = arts.first() else {
        return Err(Error::Invalid(
            "no Channel draws its Menu on an image".into(),
        ));
    };
    let saved = Saved::load(s).await?;
    let (texts, menus) = menus(d).await?;
    let unlocked_menus: Vec<Menu> = menus.iter().map(|(_, m)| m.clone()).collect();
    let mut refused = Vec::new();
    let mut keys = HashMap::new();
    for (slot, bytes) in [
        (MenuSlot::Locked, &change.locked),
        (MenuSlot::Unlocked, &change.unlocked),
    ] {
        let key = match (bytes, change.reset) {
            (_, true) => None,
            (None, false) => saved.key(slot).map(str::to_string),
            (Some(b), false) => match check(d, slot, b, &unlocked_menus) {
                Ok(_) => Some(store::key_of(MENU_DIR, b, "png")),
                Err(p) => {
                    refused.push((slot, p));
                    None
                }
            },
        };
        keys.insert(slot, key);
    }
    if !refused.is_empty() {
        return Ok(MenuImagesSaved::Refused(refused));
    }
    let new = |slot| keys.get(&slot).cloned().flatten();
    let locked_changed = new(MenuSlot::Locked).as_deref() != saved.key(MenuSlot::Locked);
    let unlocked_changed = new(MenuSlot::Unlocked).as_deref() != saved.key(MenuSlot::Unlocked);
    if !locked_changed && !unlocked_changed {
        return Ok(MenuImagesSaved::Unchanged);
    }
    // 2. The files, by content key.
    let mut written = Vec::new();
    for (slot, bytes) in [
        (MenuSlot::Locked, &change.locked),
        (MenuSlot::Unlocked, &change.unlocked),
    ] {
        if let (Some(b), Some(_)) = (bytes, new(slot)) {
            written.push(store::put(s, MENU_DIR, b, "png", first.content_type()).await?);
        }
    }
    // The salt the staged rich menus are named with must be the one every
    // later retire reads: store it first if there is none (a concurrent
    // first save may win; read back whichever did). Drawing it is no
    // visible change, so a failed stage may leave it.
    let salt = match &saved.salt {
        Some(salt) => salt.clone(),
        None => stored_salt(s).await?,
    };
    let locked_revision = saved.locked_revision + u32::from(locked_changed);
    let next = Saved {
        locked: new(MenuSlot::Locked),
        unlocked: new(MenuSlot::Unlocked),
        locked_revision,
        salt: Some(salt.clone()),
    };
    // 3. Stage: the platform objects, before anything is saved.
    let images: MenuImages = next.images(d).await?;
    let staged = if unlocked_changed {
        next_revisions(d, &menus, &salt).await?
    } else {
        Vec::new()
    };
    for art in &arts {
        match art.stage(&images, &staged, locked_changed).await {
            Ok(None) => {}
            Ok(Some(problem)) => {
                store::forget(s, &written).await;
                let slot = if unlocked_changed {
                    MenuSlot::Unlocked
                } else {
                    MenuSlot::Locked
                };
                return Ok(MenuImagesSaved::Refused(vec![(slot, problem)]));
            }
            Err(e) => {
                store::forget(s, &written).await;
                return Err(e);
            }
        }
    }
    // 4. One batch.
    let stmts = commit(d, &saved, &next, &texts, unlocked_changed, actor, ctx)?;
    let done = s.db.batch(stmts).await?;
    if !done.first().is_some_and(|r| r.rows_affected == 1) {
        store::forget(s, &written).await;
        return Ok(MenuImagesSaved::Stale);
    }
    // 5. The replaced files.
    let replaced: Vec<String> = [saved.locked.clone(), saved.unlocked.clone()]
        .into_iter()
        .flatten()
        .filter(|k| Some(k) != next.locked.as_ref() && Some(k) != next.unlocked.as_ref())
        .collect();
    store::forget(s, &replaced).await;
    Ok(MenuImagesSaved::Saved)
}

/// The Deployment's revision salt, drawn and stored if there is none.
async fn stored_salt(s: &crate::Services) -> Result<String> {
    let insert = Statement::new(
        "INSERT INTO menu_salt (salt) SELECT ? WHERE NOT EXISTS (SELECT 1 FROM menu_salt)",
    )
    .bind(menu::new_salt(s)?);
    s.db.execute(insert).await?;
    let rows =
        s.db.query(Statement::new("SELECT salt FROM menu_salt LIMIT 1"))
            .await?;
    rows.get(0, "salt")
        .and_then(Value::as_text)
        .map(str::to_string)
        .ok_or_else(|| Error::Platform("no revision salt after storing one".into()))
}

/// Each Locale's unlocked Menu at its next revision, with `salt`: the
/// rich menus the re-link Jobs will link.
async fn next_revisions(d: &Deployment, menus: &[(Locale, Menu)], salt: &str) -> Result<Vec<Menu>> {
    let rows = d
        .services()
        .db
        .query(Statement::new(
            "SELECT locale, MAX(revision) AS r FROM menu_revision GROUP BY locale",
        ))
        .await?;
    let newest: HashMap<String, i64> = (0..rows.len())
        .filter_map(|i| {
            let l = rows.get(i, "locale").and_then(Value::as_text)?;
            Some((l.to_string(), rows.get(i, "r").and_then(Value::as_i64)?))
        })
        .collect();
    Ok(menus
        .iter()
        .map(|(l, m)| {
            let r = newest.get(l.as_str()).map_or(1, |r| r + 1);
            menu::revised(m.clone(), u32::try_from(r).unwrap_or(1), Some(salt))
        })
        .collect())
}

/// `(SELECT value FROM setting WHERE key = ? AND locale = '') IS ?`, for
/// each key and the value it must still hold.
pub(crate) fn still(pairs: &[(&str, Option<String>)]) -> Statement {
    let sql = vec!["(SELECT value FROM setting WHERE key = ? AND locale = '') IS ?"; pairs.len()];
    let mut st = Statement::new(sql.join(" AND "));
    for (k, v) in pairs {
        st = st.bind(*k).bind(v.clone());
    }
    st
}

/// Writes `value` to `key` (`None`: deletes the row), if `gate` was written.
pub(crate) fn put(
    key: &str,
    value: Option<&str>,
    by: Option<&str>,
    now: i64,
    gate: &str,
) -> Statement {
    match value {
        Some(v) => Statement::new(
            "INSERT INTO setting (key, locale, value, updated_at, updated_by) \
             SELECT ?, '', ?, ?, ? WHERE EXISTS (SELECT 1 FROM audit_log WHERE id = ?) \
             ON CONFLICT (key, locale) DO UPDATE SET value = excluded.value, \
             updated_at = excluded.updated_at, updated_by = excluded.updated_by",
        )
        .bind(key)
        .bind(v)
        .bind(now)
        .bind(by.map(str::to_string))
        .bind(gate),
        None => Statement::new(
            "DELETE FROM setting WHERE key = ? AND locale = '' \
             AND EXISTS (SELECT 1 FROM audit_log WHERE id = ?)",
        )
        .bind(key)
        .bind(gate),
    }
}

/// The staff id of `actor`, for `setting.updated_by`.
pub(crate) fn by(actor: &Actor) -> Option<&str> {
    match actor {
        Actor::Staff(id) => Some(id.as_str()),
        _ => None,
    }
}

fn commit(
    d: &Deployment,
    saved: &Saved,
    next: &Saved,
    texts: &Texts,
    unlocked_changed: bool,
    actor: &Actor,
    ctx: &RequestCtx,
) -> Result<Vec<Statement>> {
    let s = d.services();
    let rev = |r: u32| (r > 0).then(|| r.to_string());
    let condition = still(&[
        (LOCKED_KEY, saved.locked.clone()),
        (UNLOCKED_KEY, saved.unlocked.clone()),
        (LOCKED_REVISION_KEY, rev(saved.locked_revision)),
    ]);
    let shown = |v: &Saved| json!({ "locked": v.locked, "unlocked": v.unlocked, "locked_revision": v.locked_revision });
    let action = if next.locked.is_none() && next.unlocked.is_none() {
        "branding.menu_images.reset"
    } else {
        "branding.menu_images"
    };
    let (row, gate) = audit::entry_if(
        s,
        actor,
        action,
        Some(&Target::new("setting", "kernel.branding.menu")),
        Some(shown(saved)),
        Some(shown(next)),
        ctx,
        condition,
    )?;
    let now = s.now_ms();
    let by = by(actor);
    let mut stmts = vec![
        row,
        put(LOCKED_KEY, next.locked.as_deref(), by, now, &gate),
        put(UNLOCKED_KEY, next.unlocked.as_deref(), by, now, &gate),
        put(
            LOCKED_REVISION_KEY,
            rev(next.locked_revision).as_deref(),
            by,
            now,
            &gate,
        ),
    ];
    let salt = next.salt.as_deref().unwrap_or_default();
    if unlocked_changed {
        stmts.extend(menu::on_images(d, texts, salt, &gate)?);
    } else {
        stmts.push(menu::salt_once(salt, &gate));
    }
    if next.locked_revision != saved.locked_revision {
        let audited =
            Statement::new("EXISTS (SELECT 1 FROM audit_log WHERE id = ?)").bind(gate.as_str());
        let key = format!("{RELOCK}:{gate}");
        let p = payload(next.locked_revision);
        let (_, enqueue) = d.jobs().insert_unique_if(&key, RELOCK, &p, None, audited)?;
        stmts.push(enqueue);
    }
    Ok(stmts)
}

//! Branding (#114): the Deployment's display name, primary colour, logo
//! and Menu images, edited on `/admin/branding` (`kernel.setting.edit`)
//! with a preview, no deploy.
//!
//! Kernel settings (`setting`, Locale `''`), each missing until a save,
//! when `deployment.toml` (`name`, `[branding]`) or the Channel's
//! configured images apply:
//!
//! | Key | Value |
//! |---|---|
//! | [`NAME_KEY`] | the institution's display name |
//! | [`COLOR_KEY`] | `#rrggbb` |
//! | [`LOGO_KEY`] | FileStore key `branding/<sha256>.png` or `.jpg` |
//! | [`LOCKED_KEY`], [`UNLOCKED_KEY`] | FileStore key `branding/menu/<sha256>.png` |
//! | [`LOCKED_REVISION_KEY`] | the locked Menu's revision, bumped on each change of its image |
//!
//! Files are content-addressed, so a save never overwrites what an older
//! backup or a running request reads; a key no setting names any more is
//! deleted after the save. Backups carry the settings with the `setting`
//! table and the files because their keys end in `_file_key` (see
//! [`crate::backup`]).
//!
//! The page chrome ([`crate::ui`]) reads [`Cache`], refreshed by a layer on
//! every request at most every [`REFRESH_MS`] and at once by a save in the
//! same process; a Worker isolate that did not save sees a change within
//! that time. The Menu images are read fresh for every Menu set
//! ([`store::drawn`]).

pub mod api;
mod apply;
mod check;
mod logo;
mod menu_page;
mod page;
mod relock;
mod save;
mod store;
mod texts;
mod view;

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use axum::extract::{Request, State};
use axum::middleware::Next;
use axum::response::Response;
use axum::routing::{get, post};

use crate::config::DeploymentConfig;
use crate::module::{ModuleRegistrar, UploadSpec};
use crate::port_types::{Statement, Value};
use crate::{AppState, Result, Services};

pub(crate) use relock::RELOCK;
pub(crate) use store::{drawn, drawn_all};

/// The display name.
pub(crate) const NAME_KEY: &str = "kernel.branding.name";
/// The primary colour, `#rrggbb`.
pub(crate) const COLOR_KEY: &str = "kernel.branding.primary_color";
/// The logo's FileStore key.
pub(crate) const LOGO_KEY: &str = "kernel.branding.logo_file_key";
/// The locked Menu image's FileStore key.
pub(crate) const LOCKED_KEY: &str = "kernel.branding.menu_locked_file_key";
/// The unlocked Menu image's FileStore key.
pub(crate) const UNLOCKED_KEY: &str = "kernel.branding.menu_unlocked_file_key";
/// The locked Menu's revision.
pub(crate) const LOCKED_REVISION_KEY: &str = "kernel.branding.menu_locked_revision";
/// Every key above starts with this.
const PREFIX: &str = "kernel.branding.";

/// The page.
pub(crate) const PATH: &str = "/branding";
/// The name, colour and logo form.
pub(crate) const SAVE_PATH: &str = "/branding/save";
/// The Menu images form.
pub(crate) const MENU_PATH: &str = "/branding/menu";
/// The logo, public (sign-in and Mini App pages show it before any session).
pub(crate) const LOGO_PATH: &str = "/branding/logo";
/// The Permission of the page: Administrator holds it.
pub(crate) const EDIT: &str = "kernel.setting.edit";
/// Largest body of the page's forms: two Menu images of 1 MB, each sent
/// again as base64 after a preview, and the fields.
const BODY_LIMIT: usize = 6 * 1024 * 1024;
/// How old the chrome may be before a request reads the settings again.
pub(crate) const REFRESH_MS: i64 = 10_000;

/// Registers the page, the logo route and the re-lock Job on the Kernel.
pub(crate) fn register(r: &mut ModuleRegistrar) {
    let spec = |path| UploadSpec {
        path,
        max_body: BODY_LIMIT,
        too_large: None,
    };
    r.messages(texts::TEXTS)
        .back_office_page(EDIT, PATH, "kernel.branding.title", 85, get(page::show))
        .back_office_upload(EDIT, spec(SAVE_PATH), post(page::submit))
        .back_office_upload(EDIT, spec(MENU_PATH), post(menu_page::submit))
        .back_office(EDIT, "/branding/menu/{slot}", get(menu_page::image))
        .public(LOGO_PATH, get(logo::serve));
    relock::register(r);
}

/// The `kernel.branding.*` settings, by key.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Stored(HashMap<String, String>);

impl Stored {
    /// The value of `key`, if saved.
    pub(crate) fn get(&self, key: &str) -> Option<&str> {
        self.0.get(key).map(String::as_str)
    }

    /// Reads them.
    pub(crate) async fn load(s: &Services) -> Result<Self> {
        let rows =
            s.db.query(
                Statement::new(
                    "SELECT key, value FROM setting WHERE locale = '' AND substr(key, 1, ?) = ?",
                )
                .bind(PREFIX.len() as i64)
                .bind(PREFIX),
            )
            .await?;
        let text = |i: usize, c: &str| rows.get(i, c).and_then(Value::as_text).map(str::to_string);
        Ok(Self(
            (0..rows.len())
                .filter_map(|i| Some((text(i, "key")?, text(i, "value")?)))
                .collect(),
        ))
    }
}

/// What the page chrome shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Chrome {
    pub(crate) name: String,
    pub(crate) color: String,
    /// URL of the logo.
    pub(crate) logo: Option<String>,
    pub(crate) font_url: Option<String>,
}

impl Chrome {
    /// The saved values over the TOML ones.
    pub(crate) fn of(config: &DeploymentConfig, stored: &Stored) -> Self {
        let b = &config.branding;
        let logo = match stored.get(LOGO_KEY) {
            Some(key) => Some(logo::url(key)),
            None => b.logo.clone(),
        };
        Chrome {
            name: stored
                .get(NAME_KEY)
                .map(str::to_string)
                .or_else(|| b.name.clone())
                .unwrap_or_else(|| config.name.clone()),
            color: stored
                .get(COLOR_KEY)
                .map_or_else(|| b.primary_color.clone(), str::to_string),
            logo,
            font_url: b.font_url.clone(),
        }
    }
}

/// The chrome's settings for this process, and since when (ms).
#[derive(Default)]
pub(crate) struct Cache {
    stored: Mutex<Option<(Arc<Stored>, i64)>>,
    pub(crate) images: store::ImageCache,
}

impl core::fmt::Debug for Cache {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("branding::Cache { .. }")
    }
}

impl Cache {
    fn lock(&self) -> std::sync::MutexGuard<'_, Option<(Arc<Stored>, i64)>> {
        self.stored.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// The settings last read (none: the TOML values).
    pub(crate) fn stored(&self) -> Arc<Stored> {
        self.lock()
            .as_ref()
            .map(|(s, _)| s.clone())
            .unwrap_or_default()
    }

    /// Replaces them, read at `now`.
    pub(crate) fn put(&self, stored: Stored, now: i64) {
        *self.lock() = Some((Arc::new(stored), now));
    }

    /// Reads them again when older than [`REFRESH_MS`]. A failed read
    /// keeps the old ones (the page still renders).
    pub(crate) async fn refresh(&self, s: &Services) {
        let now = s.now_ms();
        let fresh = self
            .lock()
            .as_ref()
            .is_some_and(|(_, at)| (0..REFRESH_MS).contains(&(now - at)));
        if !fresh && let Ok(stored) = Stored::load(s).await {
            self.put(stored, now);
        }
    }
}

/// The layer that keeps [`Cache`] fresh for the pages of this request.
pub(crate) async fn refresh(State(state): State<AppState>, req: Request, next: Next) -> Response {
    state.deployment.branding().refresh(&state.services).await;
    next.run(req).await
}

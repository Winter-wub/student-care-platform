//! Menu revisions and the Menu re-link (#66).
//!
//! A back-office save (or reset) of a Menu label changes the unlocked Menu
//! of every effective Locale whose labels it changes. In the same batch as
//! the `setting` write and its audit row, [`on_save`] records the next
//! **Menu revision** of each such Locale in `menu_revision` (and, the
//! first time, revision 0 for the Menu shown until then, and the
//! Deployment's revision salt in `menu_salt`), and enqueues one Job
//! [`RELINK`] per Locale. Every Menu the Kernel sets carries the newest
//! revision and the salt ([`unlocked`]), so an adapter can name each
//! version apart, and apart from another Deployment's on the same
//! platform account ([`crate::Menu::revision`], [`crate::Menu::salt`]).
//!
//! The Job ([`job`]) re-links every linked account of that Locale to the
//! newest revision through [`crate::Channel::relink_menu`], a chunk per
//! run, passing over accounts the platform will never reach (their Link
//! marked unreachable), and only once every Channel is done retires the
//! older revisions through [`crate::Channel::retire_menus`], finishing
//! first any retire still pending from an earlier run (`menu_retire`). A
//! failed chunk is retried by the Job's retry from the same cursor;
//! nothing is retired before the last chunk. A save during a run makes a
//! newer revision: the older run stops at its next step and the newer Job
//! re-links everyone and retires every older revision.

mod job;

use std::collections::HashMap;

use serde_json::json;

use crate::kernel::text::Texts;
use crate::manifest::JobSpec;
use crate::message::{ActionId, Locale, Menu, MenuEntry};
use crate::module::ModuleRegistrar;
use crate::port_types::{Statement, Value};
use crate::{Deployment, Result, Services};

/// The re-link Job kind; payload `{"locale": <effective Locale>}`.
pub(crate) const RELINK: &str = "kernel.menu.relink";

/// Registers the Job on the Kernel.
pub(crate) fn register(r: &mut ModuleRegistrar) {
    r.job(JobSpec::new(RELINK), job::run);
}

/// Length of the revision salt, in bytes (8 hex digits).
const SALT_BYTES: usize = 4;

/// A new revision salt from the Random port.
pub(crate) fn new_salt(s: &Services) -> Result<String> {
    s.random_hex(SALT_BYTES)
}

/// Replaces the stored revision salt, if any, with a new one: a restored
/// backup must not share its salt with the Deployment it came from (they
/// may run on one platform account). Rich menus named with the old salt are
/// then never retired by this Deployment: they leak, which is safe.
pub(crate) fn redraw_salt(s: &Services) -> Result<Statement> {
    Ok(Statement::new("UPDATE menu_salt SET salt = ?").bind(new_salt(s)?))
}

/// What a restore must not carry over from the Deployment it came from:
/// retires still pending there (their tokens name that Deployment's
/// platform requests, e.g. a LINE batch id) and its queued or running
/// re-link Jobs (they would link its accounts, on its platform account, to
/// this Deployment's Menus). The Jobs are marked failed with a reason; the
/// revisions they worked for stay `linking` until the next label save.
pub(crate) fn forget_runs(s: &Services) -> Vec<Statement> {
    vec![
        Statement::new("DELETE FROM menu_retire"),
        Statement::new(
            "UPDATE job SET state = 'failed', last_error = ?, finished_at = ?, \
             lock_token = NULL, locked_until = NULL \
             WHERE kind IN (?, ?) AND state IN ('queued', 'running')",
        )
        .bind("cancelled by the restore of a backup")
        .bind(s.now_ms())
        .bind(RELINK)
        .bind(crate::kernel::branding::RELOCK),
    ]
}

/// The newest Menu revision of the effective Locale `locale` (0 when no
/// Menu label was ever saved there) and the Deployment's revision salt.
pub(crate) async fn revision(s: &Services, locale: &Locale) -> Result<(u32, Option<String>)> {
    let rows =
        s.db.query(
            Statement::new(
                "SELECT (SELECT MAX(revision) FROM menu_revision WHERE locale = ?) AS r, \
                 (SELECT salt FROM menu_salt LIMIT 1) AS salt",
            )
            .bind(locale.as_str()),
        )
        .await?;
    let revision = rows
        .get(0, "r")
        .and_then(Value::as_i64)
        .and_then(|r| u32::try_from(r).ok())
        .unwrap_or(0);
    let salt = rows
        .get(0, "salt")
        .and_then(Value::as_text)
        .map(str::to_string);
    Ok((revision, salt))
}

/// `menu` at `revision`, with `salt` when there is one.
pub(crate) fn revised(menu: Menu, revision: u32, salt: Option<&str>) -> Menu {
    let menu = menu.with_revision(revision);
    match salt {
        Some(salt) => menu.with_salt(salt),
        None => menu,
    }
}

/// The unlocked Menu a linked Student in `locale` gets now: the effective
/// Locale's entries with `texts` (and their `setting` overrides), at its
/// newest revision, with the Deployment's salt.
pub(crate) async fn unlocked(d: &Deployment, texts: &Texts, locale: &Locale) -> Result<Menu> {
    let locale = d.catalog().effective(locale);
    let texts = texts.in_locale(&locale);
    let (revision, salt) = revision(d.services(), &locale).await?;
    let menu = d.menu_with(&locale, |k| texts.t(k));
    Ok(revised(menu, revision, salt.as_deref()))
}

/// The statements a Texts save of `key` in `locale` adds to its batch,
/// gated like the write on its audit row `gate`: for every effective
/// Locale whose Menu labels change from `stored` (the `setting` rows
/// before the write, `(key, locale) -> value`) to the same rows with
/// `value` (`None`: the row is reset), the next revision and one Job
/// [`RELINK`]. Nothing for a key that is no Menu label.
pub(crate) fn on_save(
    d: &Deployment,
    stored: &HashMap<(String, String), String>,
    key: &str,
    locale: &str,
    value: Option<&str>,
    gate: &str,
) -> Result<Vec<Statement>> {
    if !d.menu_label_keys().contains(&key) {
        return Ok(Vec::new());
    }
    let before: HashMap<(String, String), String> = stored
        .iter()
        .map(|((k, l), v)| ((l.clone(), k.clone()), v.clone()))
        .collect();
    let mut after = before.clone();
    match value {
        Some(v) => after.insert((locale.to_string(), key.to_string()), v.to_string()),
        None => after.remove(&(locale.to_string(), key.to_string())),
    };
    let catalog = d.catalog_arc();
    let default = catalog.default_locale();
    let old = Texts::with_overrides(catalog.clone(), before, default.clone());
    let new = Texts::with_overrides(catalog.clone(), after, default);
    let mut stmts = Vec::new();
    for l in catalog.locales() {
        let (o, n) = (old.in_locale(&l), new.in_locale(&l));
        let was = entries_json(&d.menu_with(&l, |k| o.t(k)));
        let is = entries_json(&d.menu_with(&l, |k| n.t(k)));
        if was == is {
            continue;
        }
        if stmts.is_empty() {
            // The Deployment's revision salt, once.
            stmts.push(salt_once(&new_salt(d.services())?, gate));
        }
        stmts.extend(next_revision(d, &l, &was, &is, gate)?);
    }
    Ok(stmts)
}

/// The statements a Menu image save (#114) adds to its batch, gated on
/// its audit row `gate`: the next revision of every Locale's unlocked
/// Menu (the same entries, `texts` with their `setting` overrides: the
/// image is in the platform object, not in the entries) and one Job
/// [`RELINK`] each, so every account is re-linked to the new image and
/// the older revisions retired as after a label save. `salt` is the one
/// the new rich menus were staged with, stored if there is none yet.
pub(crate) fn on_images(
    d: &Deployment,
    texts: &Texts,
    salt: &str,
    gate: &str,
) -> Result<Vec<Statement>> {
    let mut stmts = vec![salt_once(salt, gate)];
    for l in d.catalog().locales() {
        let t = texts.in_locale(&l);
        let entries = entries_json(&d.menu_with(&l, |k| t.t(k)));
        stmts.extend(next_revision(d, &l, &entries, &entries, gate)?);
    }
    Ok(stmts)
}

/// Stores `salt` as the Deployment's revision salt unless it has one.
pub(crate) fn salt_once(salt: &str, gate: &str) -> Statement {
    Statement::new(
        "INSERT INTO menu_salt (salt) SELECT ? \
         WHERE EXISTS (SELECT 1 FROM audit_log WHERE id = ?) \
         AND NOT EXISTS (SELECT 1 FROM menu_salt)",
    )
    .bind(salt)
    .bind(gate)
}

/// Revision 0 (`was`, the Menu shown until now) the first time, the next
/// revision (`is`), and the Job [`RELINK`] of Locale `l`.
fn next_revision(
    d: &Deployment,
    l: &Locale,
    was: &str,
    is: &str,
    gate: &str,
) -> Result<Vec<Statement>> {
    let now = d.services().now_ms();
    let tag = l.as_str();
    let audited = || Statement::new("EXISTS (SELECT 1 FROM audit_log WHERE id = ?)").bind(gate);
    let mut stmts = vec![
        Statement::new(
            "INSERT INTO menu_revision (locale, revision, entries, state, created_at, \
             updated_at) SELECT ?, 0, ?, 'live', ?, ? \
             WHERE EXISTS (SELECT 1 FROM audit_log WHERE id = ?) \
             AND NOT EXISTS (SELECT 1 FROM menu_revision WHERE locale = ?)",
        )
        .bind(tag)
        .bind(was)
        .bind(now)
        .bind(now)
        .bind(gate)
        .bind(tag),
        Statement::new(
            "INSERT INTO menu_revision (locale, revision, entries, state, created_at, \
             updated_at) SELECT ?, MAX(revision) + 1, ?, 'linking', ?, ? \
             FROM menu_revision WHERE locale = ? \
             AND EXISTS (SELECT 1 FROM audit_log WHERE id = ?) HAVING COUNT(*) > 0",
        )
        .bind(tag)
        .bind(is)
        .bind(now)
        .bind(now)
        .bind(tag)
        .bind(gate),
    ];
    let job_key = format!("{RELINK}:{tag}:{gate}");
    let payload = json!({ "locale": tag });
    let (_, enqueue) = d
        .jobs()
        .insert_unique_if(&job_key, RELINK, &payload, None, audited())?;
    stmts.push(enqueue);
    Ok(stmts)
}

/// A Menu's entries as stored in `menu_revision.entries`.
pub(crate) fn entries_json(menu: &Menu) -> String {
    let v: Vec<serde_json::Value> = menu
        .entries
        .iter()
        .map(|e| json!({ "label": e.label, "action": e.action.as_str() }))
        .collect();
    serde_json::Value::Array(v).to_string()
}

/// The entries stored by [`entries_json`]; entries that no longer parse
/// are dropped (the Menu then names no platform object of ours).
pub(crate) fn entries_from(json: &str) -> Vec<MenuEntry> {
    let v: serde_json::Value = serde_json::from_str(json).unwrap_or_default();
    v.as_array()
        .into_iter()
        .flatten()
        .filter_map(|e| {
            Some(MenuEntry {
                label: e["label"].as_str()?.to_string(),
                action: ActionId::new(e["action"].as_str()?).ok()?,
            })
        })
        .collect()
}

//! Menu revisions and the Menu re-link (#66).
//!
//! A back-office save (or reset) of a Menu label changes the unlocked Menu
//! of every effective Locale whose labels it changes. In the same batch as
//! the `setting` write and its audit row, [`on_save`] records the next
//! **Menu revision** of each such Locale in `menu_revision` (and, the
//! first time, revision 0 for the Menu shown until then) and enqueues one
//! Job [`RELINK`] per Locale. Every Menu the Kernel sets carries the
//! newest revision ([`unlocked`]), so an adapter can name each version
//! apart ([`crate::Menu::revision`]).
//!
//! The Job ([`job`]) re-links every linked account of that Locale to the
//! newest revision through [`crate::Channel::relink_menu`], a chunk per
//! run, and only once every Channel is done retires the older revisions
//! through [`crate::Channel::retire_menus`]. A failed chunk is retried by
//! the Job's retry from the same cursor; nothing is retired before the
//! last chunk. A save during a run makes a newer revision: the older run
//! stops at its next step and the newer Job re-links everyone and retires
//! both older revisions.

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

/// The newest Menu revision of the effective Locale `locale` (0 when no
/// Menu label was ever saved there).
pub(crate) async fn revision(s: &Services, locale: &Locale) -> Result<u32> {
    let rows =
        s.db.query(
            Statement::new("SELECT MAX(revision) AS r FROM menu_revision WHERE locale = ?")
                .bind(locale.as_str()),
        )
        .await?;
    Ok(rows
        .get(0, "r")
        .and_then(Value::as_i64)
        .and_then(|r| u32::try_from(r).ok())
        .unwrap_or(0))
}

/// The unlocked Menu a linked Student in `locale` gets now: the effective
/// Locale's entries with `texts` (and their `setting` overrides), at its
/// newest revision.
pub(crate) async fn unlocked(d: &Deployment, texts: &Texts, locale: &Locale) -> Result<Menu> {
    let locale = d.catalog().effective(locale);
    let texts = texts.in_locale(&locale);
    let revision = revision(d.services(), &locale).await?;
    Ok(d.menu_with(&locale, |k| texts.t(k)).with_revision(revision))
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
    let now = d.services().now_ms();
    let audited = || Statement::new("EXISTS (SELECT 1 FROM audit_log WHERE id = ?)").bind(gate);
    let mut stmts = Vec::new();
    for l in catalog.locales() {
        let (o, n) = (old.in_locale(&l), new.in_locale(&l));
        let was = d.menu_with(&l, |k| o.t(k));
        let is = d.menu_with(&l, |k| n.t(k));
        let (was, is) = (entries_json(&was), entries_json(&is));
        if was == is {
            continue;
        }
        let tag = l.as_str();
        stmts.push(
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
        );
        stmts.push(
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
        );
        let job_key = format!("{RELINK}:{tag}:{gate}");
        let payload = json!({ "locale": tag });
        let (_, enqueue) =
            d.jobs()
                .insert_unique_if(&job_key, RELINK, &payload, None, audited())?;
        stmts.push(enqueue);
    }
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

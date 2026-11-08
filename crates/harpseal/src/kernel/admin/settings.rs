//! The `setting` table as the Texts page edits it: catalog texts per
//! Locale with their source, `{name}` placeholder checks, the UPSERT and
//! the reset, and the Consent notice version (key
//! `kernel.consent.version`, Locale `''`). No cache: the chat flow's
//! `Texts::load` reads `setting` on every Inbound, so an edit is live on
//! the next message.

use std::collections::{BTreeSet, HashMap};

use serde_json::json;

use crate::audit::{self, Actor, RequestCtx, Target};
use crate::config::DeploymentConfig;
use crate::kernel::text::CONSENT_VERSION_KEY;
use crate::manifest::Catalog;
use crate::port_types::{Statement, Value};
use crate::{Result, Services};

/// Longest text accepted, in characters.
pub(super) const MAX_TEXT_CHARS: usize = 5000;
/// Longest Consent version, in characters.
pub(super) const MAX_VERSION_CHARS: usize = 32;

/// Where the effective text comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Source {
    /// The Module's default.
    Default,
    /// `[texts]` in `deployment.toml`.
    Toml,
    /// A `setting` row written from this page.
    Setting,
}

impl Source {
    pub(super) fn key(self) -> &'static str {
        match self {
            Source::Default => "kernel.admin.texts.source_default",
            Source::Toml => "kernel.admin.texts.source_toml",
            Source::Setting => "kernel.admin.texts.source_setting",
        }
    }
}

/// One key in one Locale.
#[derive(Debug, Clone)]
pub(super) struct TextRow {
    pub(super) locale: String,
    /// What a reset falls back to: the catalog (Module default, or the
    /// TOML text when `[texts]` sets it) in this Locale or, failing
    /// that, in the default Locale.
    pub(super) base: String,
    pub(super) source: Source,
    /// The `setting` row's value, if any.
    pub(super) setting: Option<String>,
}

impl TextRow {
    pub(super) fn effective(&self) -> &str {
        self.setting.as_deref().unwrap_or(&self.base)
    }
}

/// `(key, locale)` -> value of every text `setting` row (not the version).
pub(super) async fn settings(s: &Services) -> Result<HashMap<(String, String), String>> {
    let rows =
        s.db.query(Statement::new(
            "SELECT key, locale, value FROM setting WHERE locale <> ''",
        ))
        .await?;
    let text = |i: usize, c: &str| {
        rows.get(i, c)
            .and_then(Value::as_text)
            .unwrap_or_default()
            .to_string()
    };
    Ok((0..rows.len())
        .map(|i| ((text(i, "key"), text(i, "locale")), text(i, "value")))
        .collect())
}

/// The Locales the page offers: the default first, then `th`, `en` and
/// every other Locale of the catalog, sorted.
pub(super) fn locales(catalog: &Catalog, config: &DeploymentConfig) -> Vec<String> {
    let mut rest: BTreeSet<String> = ["th", "en"].iter().map(|l| l.to_string()).collect();
    rest.extend(catalog.entries().map(|(l, _, _)| l.to_string()));
    rest.extend(config.texts.keys().cloned());
    let default = config.default_locale.0.clone();
    rest.remove(&default);
    // `''` holds the Consent version and Module settings, never texts.
    std::iter::once(default)
        .chain(rest)
        .filter(|l| !l.trim().is_empty())
        .collect()
}

/// Prefixes of texts the back office reads from the build-time catalog
/// (its own pages), never through `setting`: an edit would not show, so
/// the Texts page leaves them out.
const BUILD_TIME: [&str; 5] = [
    "kernel.admin.",
    "kernel.auth.",
    "kernel.backoffice.",
    "kernel.students.",
    "kernel.roster.",
];

/// Whether `key` is a catalog key the Texts page edits: read through
/// `Texts::load` (chat flow, Module Actions, `/notice`), not a
/// back-office page text.
pub(super) fn editable(catalog: &Catalog, key: &str) -> bool {
    catalog.knows(key) && !BUILD_TIME.iter().any(|p| key.starts_with(p))
}

/// The compare-and-set token of a form: SHA-256 of the `setting` value
/// it was rendered from (`-` for no row). A token instead of the text, so
/// browsers' CRLF in hidden fields never matters.
pub(super) fn version(setting: Option<&str>) -> String {
    match setting {
        Some(v) => crate::auth::sha256_hex(v.as_bytes()),
        None => "-".into(),
    }
}

/// Every editable catalog key, sorted.
pub(super) fn keys(catalog: &Catalog) -> Vec<String> {
    let set: BTreeSet<&str> = catalog
        .entries()
        .map(|(_, k, _)| k)
        .filter(|k| editable(catalog, k))
        .collect();
    set.into_iter().map(str::to_string).collect()
}

/// `key` in `locale`.
pub(super) fn row(
    catalog: &Catalog,
    config: &DeploymentConfig,
    settings: &HashMap<(String, String), String>,
    key: &str,
    locale: &str,
) -> TextRow {
    let setting = settings
        .get(&(key.to_string(), locale.to_string()))
        .cloned();
    let toml = config
        .texts
        .get(locale)
        .is_some_and(|t| t.contains_key(key));
    let source = match (&setting, toml) {
        (Some(_), _) => Source::Setting,
        (None, true) => Source::Toml,
        (None, false) => Source::Default,
    };
    let base = catalog
        .exact(locale, key)
        .map(str::to_string)
        .unwrap_or_else(|| catalog.t(&crate::message::Locale(locale.into()), key));
    TextRow {
        locale: locale.to_string(),
        base,
        source,
        setting,
    }
}

/// The `{name}` placeholders in `text` (`name` = ASCII letters, digits, `_`).
pub(super) fn placeholders(text: &str) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    let mut rest = text;
    while let Some(open) = rest.find('{') {
        let after = &rest[open + 1..];
        match after.find('}') {
            Some(close) => {
                let name = &after[..close];
                let ok = !name.is_empty()
                    && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_');
                if ok {
                    out.insert(name.to_string());
                }
                rest = &after[close + 1..];
            }
            None => break,
        }
    }
    out
}

/// Why a value was refused: catalog key of the message, plus the
/// placeholder names concerned.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Refused {
    pub(super) key: &'static str,
    pub(super) names: Vec<String>,
}

/// Checks `value` against the text it replaces: not empty, not too long,
/// the same `{name}` placeholders as `base` (none dropped, none added).
pub(super) fn check(base: &str, value: &str) -> Result<(), Refused> {
    let refused = |key, names| Err(Refused { key, names });
    if value.trim().is_empty() {
        return refused("kernel.admin.texts.err_empty", Vec::new());
    }
    if value.chars().count() > MAX_TEXT_CHARS {
        return refused("kernel.admin.texts.err_long", Vec::new());
    }
    let (want, got) = (placeholders(base), placeholders(value));
    let missing: Vec<String> = want.difference(&got).cloned().collect();
    if !missing.is_empty() {
        return refused("kernel.admin.texts.err_missing", missing);
    }
    let unknown: Vec<String> = got.difference(&want).cloned().collect();
    if !unknown.is_empty() {
        return refused("kernel.admin.texts.err_unknown", unknown);
    }
    Ok(())
}

/// `EXISTS`-style condition: the `setting` row `(key, locale)` still
/// holds `expected` (`None` = no row).
fn still(key: &str, locale: &str, expected: Option<&str>) -> Statement {
    Statement::new("(SELECT value FROM setting WHERE key = ? AND locale = ?) IS ?")
        .bind(key)
        .bind(locale)
        .bind(expected.map(str::to_string))
}

/// Builds more statements for a write's batch from its audit row id.
pub(super) type Then<'a> = &'a (dyn Fn(&str) -> Result<Vec<Statement>> + Send + Sync);

fn target(key: &str, locale: &str) -> Target {
    Target::new("setting", format!("{key}:{locale}"))
}

/// Writes `value` for `(key, locale)` if the row still holds `expected`,
/// with the audit row `setting.update` (before = the old effective text)
/// in the same batch, and the statements `then` builds from the audit
/// row's id (gated on it: the Menu re-link of a Menu label, #66). `false`
/// when the row changed meanwhile.
#[allow(clippy::too_many_arguments)]
pub(crate) async fn save(
    s: &Services,
    actor: &Actor,
    key: &str,
    locale: &str,
    value: &str,
    expected: Option<&str>,
    before: Option<&str>,
    ctx: &RequestCtx,
    then: Then<'_>,
) -> Result<bool> {
    let (row, gate) = audit::entry_if(
        s,
        actor,
        "setting.update",
        Some(&target(key, locale)),
        Some(json!({ "key": key, "locale": locale, "value": before })),
        Some(json!({ "key": key, "locale": locale, "value": value })),
        ctx,
        still(key, locale, expected),
    )?;
    let by = match actor {
        Actor::Staff(id) => Some(id.clone()),
        _ => None,
    };
    let upsert = Statement::new(
        "INSERT INTO setting (key, locale, value, updated_at, updated_by) \
         SELECT ?, ?, ?, ?, ? WHERE EXISTS (SELECT 1 FROM audit_log WHERE id = ?) \
         ON CONFLICT (key, locale) DO UPDATE SET value = excluded.value, \
         updated_at = excluded.updated_at, updated_by = excluded.updated_by",
    )
    .bind(key)
    .bind(locale)
    .bind(value)
    .bind(s.now_ms())
    .bind(by)
    .bind(gate.as_str());
    let mut stmts = vec![row, upsert];
    stmts.extend(then(&gate)?);
    let done = s.db.batch(stmts).await?;
    Ok(done.first().is_some_and(|r| r.rows_affected == 1))
}

/// Deletes the `setting` row `(key, locale)` if it still holds
/// `expected`, audited as `setting.reset` (after = the text it falls back
/// to, `null` when nothing does), with the statements `then` builds as in [`save`]. `false` when it
/// changed or was already gone.
#[allow(clippy::too_many_arguments)]
pub(crate) async fn reset(
    s: &Services,
    actor: &Actor,
    key: &str,
    locale: &str,
    expected: &str,
    fallback: Option<&str>,
    ctx: &RequestCtx,
    then: Then<'_>,
) -> Result<bool> {
    let (row, gate) = audit::entry_if(
        s,
        actor,
        "setting.reset",
        Some(&target(key, locale)),
        Some(json!({ "key": key, "locale": locale, "value": expected })),
        Some(json!({ "key": key, "locale": locale, "value": fallback })),
        ctx,
        still(key, locale, Some(expected)),
    )?;
    let delete = Statement::new(
        "DELETE FROM setting WHERE key = ? AND locale = ? \
         AND EXISTS (SELECT 1 FROM audit_log WHERE id = ?)",
    )
    .bind(key)
    .bind(locale)
    .bind(gate.as_str());
    let mut stmts = vec![row, delete];
    stmts.extend(then(&gate)?);
    let done = s.db.batch(stmts).await?;
    Ok(done.first().is_some_and(|r| r.rows_affected == 1))
}

/// The stored Consent version row, if any.
pub(super) async fn stored_version(s: &Services) -> Result<Option<String>> {
    let rows =
        s.db.query(
            Statement::new("SELECT value FROM setting WHERE key = ? AND locale = ''")
                .bind(CONSENT_VERSION_KEY),
        )
        .await?;
    Ok(rows
        .get(0, "value")
        .and_then(Value::as_text)
        .map(str::to_string))
}

/// Whether `v` is a usable version: 1..=32 of ASCII letters, digits, `.`,
/// `_`, `-`.
pub(super) fn valid_version(v: &str) -> bool {
    !v.is_empty()
        && v.len() <= MAX_VERSION_CHARS
        && v.bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
}

/// Sets the Consent version to `value` if the row still holds `expected`
/// (`None` = no row, the TOML version applies). Audited as
/// `setting.update` like a text. Every linked Student is asked again on
/// their next Inbound.
pub(super) async fn set_version(
    s: &Services,
    actor: &Actor,
    value: &str,
    expected: Option<&str>,
    before: &str,
    ctx: &RequestCtx,
) -> Result<bool> {
    save(
        s,
        actor,
        CONSENT_VERSION_KEY,
        "",
        value,
        expected,
        Some(before),
        ctx,
        &|_| Ok(Vec::new()),
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn placeholders_are_names_in_braces() {
        let p = placeholders("ไฟล์ใหญ่เกิน {mb} MB {a_1} {} {no space} {x");
        assert_eq!(p, ["a_1", "mb"].iter().map(|s| s.to_string()).collect());
    }

    #[test]
    fn values_keep_the_placeholders_of_the_default() {
        let base = "เกิน {mb} MB";
        assert!(check(base, "ใหญ่กว่า {mb} เมกะไบต์").is_ok());
        assert_eq!(check(base, "ใหญ่เกินไป").unwrap_err().names, vec!["mb"]);
        let e = check(base, "{mb} {gb}").unwrap_err();
        assert_eq!(
            (e.key, e.names),
            ("kernel.admin.texts.err_unknown", vec!["gb".into()])
        );
        assert_eq!(
            check(base, "  ").unwrap_err().key,
            "kernel.admin.texts.err_empty"
        );
        assert!(valid_version("2026-11.b_2") && !valid_version("2 0") && !valid_version(""));
    }
}

//! The checks [`super::load`] runs before it writes anything: migrations,
//! columns, and each table part's name, hash, count and values.

use std::collections::BTreeMap;

use serde_json::Value as Json;

use super::schema::Table;
use super::value::decode;
use super::{BackupSource, ManifestMigration, ManifestTable, part_entry, sha256_hex, table_entry};
use crate::port_types::Value;
use crate::{Error, Migration, Result};

/// The Deployment's migrations the backup had applied, in declared order.
///
/// Checked per Module, since the applied order interleaves Modules (a
/// Kernel migration added after a Module's is applied after it): every
/// Module in the backup must exist here, and its migrations in the backup
/// must be the first of its migrations here, in order, with the same names
/// and checksums.
pub(crate) fn backed_migrations(
    backup: &[ManifestMigration],
    ours: &[Migration],
) -> Result<Vec<Migration>> {
    let mut by_module: BTreeMap<&str, Vec<&ManifestMigration>> = BTreeMap::new();
    for m in backup {
        by_module.entry(m.module.as_str()).or_default().push(m);
    }
    let mut keep: Vec<(&str, &str)> = Vec::new();
    for (module, theirs) in &by_module {
        let mine: Vec<&Migration> = ours.iter().filter(|m| m.module == *module).collect();
        if mine.is_empty() {
            return Err(Error::Invalid(format!(
                "the backup has migrations of Module {module}, which this Deployment does not \
                 register: restore it with the build that took it"
            )));
        }
        for (i, b) in theirs.iter().enumerate() {
            let Some(m) = mine.get(i) else {
                return Err(Error::Invalid(format!(
                    "the backup has migration {module}/{} that this Deployment does not have \
                     ({module} has {} migrations here, {} in the backup): restore it with the \
                     build that took it",
                    b.name,
                    mine.len(),
                    theirs.len()
                )));
            };
            let sum = crate::migrate::checksum(m.sql);
            if b.name != m.name || b.checksum != sum {
                return Err(Error::Invalid(format!(
                    "migration #{} of {module} differs: the backup has {} (checksum {}), this \
                     Deployment {} (checksum {sum}): the backup comes from another Deployment \
                     or build",
                    i + 1,
                    b.name,
                    b.checksum,
                    m.name
                )));
            }
            keep.push((m.module, m.name));
        }
    }
    Ok(ours
        .iter()
        .filter(|m| keep.contains(&(m.module, m.name)))
        .cloned()
        .collect())
}

/// Every backup column must exist; a column the backup lacks must not need
/// a value.
pub(crate) fn check_columns(mt: &ManifestTable, current: &[Table]) -> Result<()> {
    let Some(t) = current.iter().find(|t| t.name == mt.name) else {
        return Err(Error::Invalid(format!(
            "table {} is in the backup but not in this Deployment's schema",
            mt.name
        )));
    };
    if let Some(gone) = mt
        .columns
        .iter()
        .find(|c| !t.columns.iter().any(|tc| &tc.name == *c))
    {
        return Err(Error::Invalid(format!(
            "column {}.{gone} is in the backup but not in the schema",
            mt.name
        )));
    }
    if let Some(new) = t
        .columns
        .iter()
        .find(|c| c.not_null && !c.has_default && c.pk == 0 && !mt.columns.contains(&c.name))
    {
        return Err(Error::Invalid(format!(
            "column {}.{} is NOT NULL without a default and not in the backup",
            mt.name, new.name
        )));
    }
    Ok(())
}

/// The rows of `mt`, in manifest column order, read from `source` part by
/// part in order (format 1: its one entry). Refuses, before anything is
/// written, a part out of place (each must be [`part_entry`] of its
/// index, so parts listed in another order or under another table's name
/// are refused), parts whose rows do not add up to the table's, and any
/// entry missing, not matching its SHA-256 or its row count, or holding a
/// row that does not decode.
pub(crate) async fn read_table(
    source: &dyn BackupSource,
    mt: &ManifestTable,
) -> Result<Vec<Vec<Value>>> {
    if mt.parts.is_empty() {
        let entry = table_entry(&mt.name);
        let bytes = source.read(&entry).await?;
        return decode_entry(mt, &entry, &mt.sha256, mt.rows, &bytes);
    }
    let sum: u64 = mt.parts.iter().map(|p| p.rows).sum();
    if sum != mt.rows {
        return Err(Error::Invalid(format!(
            "backup table {}: its parts hold {sum} rows, the manifest says {}",
            mt.name, mt.rows
        )));
    }
    let mut rows = Vec::with_capacity(usize::try_from(mt.rows).unwrap_or(0));
    for (i, part) in mt.parts.iter().enumerate() {
        let expected = part_entry(&mt.name, i);
        if part.entry != expected {
            return Err(Error::Invalid(format!(
                "backup table {}: part {} is {}, expected {expected} (parts out of order?)",
                mt.name,
                i + 1,
                part.entry
            )));
        }
        let bytes = source.read(&part.entry).await?;
        rows.extend(decode_entry(
            mt,
            &part.entry,
            &part.sha256,
            part.rows,
            &bytes,
        )?);
    }
    Ok(rows)
}

/// The rows of `entry` (of table `mt`) from its `bytes`, in manifest
/// column order, after checking the bytes' SHA-256 and the row count.
fn decode_entry(
    mt: &ManifestTable,
    entry: &str,
    sha256: &str,
    expect: u64,
    bytes: &[u8],
) -> Result<Vec<Vec<Value>>> {
    if sha256_hex(bytes) != sha256 {
        return Err(Error::Invalid(format!(
            "backup {entry} does not match its SHA-256 in the manifest"
        )));
    }
    let bad = |line: usize, m: String| Error::Invalid(format!("backup {entry} line {line}: {m}"));
    let mut rows = Vec::with_capacity(usize::try_from(expect).unwrap_or(0));
    for (i, line) in bytes.split(|b| *b == b'\n').enumerate() {
        if line.is_empty() {
            continue;
        }
        let obj: Json = serde_json::from_slice(line).map_err(|e| bad(i + 1, e.to_string()))?;
        let Json::Object(obj) = obj else {
            return Err(bad(i + 1, "not a JSON object".into()));
        };
        let row = mt
            .columns
            .iter()
            .map(|c| match obj.get(c) {
                Some(v) => decode(v).map_err(|e| bad(i + 1, e.to_string())),
                None => Err(bad(i + 1, format!("column {c} is missing"))),
            })
            .collect::<Result<Vec<Value>>>()?;
        rows.push(row);
    }
    if rows.len() as u64 != expect {
        return Err(Error::Invalid(format!(
            "backup {entry} has {} rows, the manifest says {expect}",
            rows.len()
        )));
    }
    Ok(rows)
}

#[cfg(test)]
mod tests {
    use super::super::schema::Column;
    use super::*;

    fn col(name: &str, not_null: bool, has_default: bool) -> Column {
        Column {
            name: name.into(),
            not_null,
            has_default,
            pk: 0,
        }
    }

    fn schema(cols: Vec<Column>) -> Vec<Table> {
        vec![Table {
            name: "permit".into(),
            sql: String::new(),
            columns: cols,
            parents: Default::default(),
        }]
    }

    fn backed(cols: &[&str]) -> ManifestTable {
        ManifestTable {
            name: "permit".into(),
            rows: 0,
            sha256: String::new(),
            parts: Vec::new(),
            columns: cols.iter().map(|c| c.to_string()).collect(),
            sql: String::new(),
        }
    }

    #[test]
    fn a_dropped_column_is_refused() {
        let now = schema(vec![col("id", true, false)]);
        let err = check_columns(&backed(&["id", "plate"]), &now).unwrap_err();
        assert!(err.to_string().contains("permit.plate"), "{err}");
        let gone = check_columns(&backed(&["id"]), &[]).unwrap_err();
        assert!(gone.to_string().contains("table permit"), "{gone}");
    }

    #[test]
    fn a_new_not_null_column_without_default_is_refused() {
        let now = schema(vec![col("id", true, false), col("zone", true, false)]);
        let err = check_columns(&backed(&["id"]), &now).unwrap_err();
        assert!(err.to_string().contains("permit.zone"), "{err}");
    }

    #[test]
    fn new_nullable_or_defaulted_columns_are_accepted() {
        let now = schema(vec![
            col("id", true, false),
            col("note", false, false),
            col("zone", true, true),
        ]);
        check_columns(&backed(&["id"]), &now).unwrap();
    }

    fn mm(module: &str, name: &str, sql: &str) -> ManifestMigration {
        ManifestMigration {
            module: module.into(),
            name: name.into(),
            checksum: crate::migrate::checksum(sql),
        }
    }

    fn ours() -> Vec<Migration> {
        let m = |module, name, sql| Migration { module, name, sql };
        vec![
            m("kernel", "0001", "A;"),
            m("kernel", "0002", "B;"),
            m("news", "0001", "N;"),
        ]
    }

    #[test]
    fn modules_are_checked_apart_from_their_applied_interleaving() {
        // Applied: kernel/0001, news/0001, kernel/0002 (Kernel added later).
        let backup = [
            mm("kernel", "0001", "A;"),
            mm("news", "0001", "N;"),
            mm("kernel", "0002", "B;"),
        ];
        let got = backed_migrations(&backup, &ours()).unwrap();
        let names: Vec<_> = got.iter().map(|m| (m.module, m.name)).collect();
        assert_eq!(
            names,
            [("kernel", "0001"), ("kernel", "0002"), ("news", "0001")]
        );
        let older = [mm("kernel", "0001", "A;")];
        assert_eq!(backed_migrations(&older, &ours()).unwrap().len(), 1);
    }

    #[test]
    fn foreign_skipped_or_edited_migrations_are_refused() {
        for backup in [
            vec![mm("parking", "0001", "P;")],
            vec![mm("kernel", "0002", "B;")],
            vec![mm("kernel", "0001", "A changed;")],
            vec![
                mm("kernel", "0001", "A;"),
                mm("kernel", "0002", "B;"),
                mm("kernel", "0003", "C;"),
            ],
        ] {
            let err = backed_migrations(&backup, &ours()).unwrap_err();
            assert!(err.to_string().contains("migration"), "{err}");
        }
    }
}

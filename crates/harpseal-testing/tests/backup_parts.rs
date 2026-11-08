//! Table parts (#88, backup format 2): a table is split into ordered
//! parts, each pinned by its SHA-256 and row count; the parts round-trip
//! to the same rows, a part corrupted, missing or out of order refuses the
//! restore before anything is written, and a format-1 backup (one entry
//! per table) still restores.

mod backup_support;

use backup_support::{config, counts, deployment, populated_as, query};
use harpseal::backup::{self, LoadOptions, MANIFEST, MemoryBackup, part_entry};
use harpseal::{Deployment, Error};
use harpseal_testing::{Harness, TestApp};
use serde_json::Value as Json;

/// Parts of at most two rows.
const TINY: &str = "[backup]\npart_rows = 2\n";

/// A populated Deployment whose backups split tables every two rows.
async fn split() -> (Harness, TestApp) {
    let h = Harness::new();
    let d = h
        .builder()
        .config(config(TINY))
        .module(harpseal::modules::announcements::Announcements)
        .build()
        .unwrap();
    populated_as(h, d).await
}

/// A fresh Deployment with the same configuration, not migrated.
fn fresh(extra: &str) -> (Harness, Deployment) {
    let h = Harness::new();
    let d = h
        .builder()
        .config(config(extra))
        .module(harpseal::modules::announcements::Announcements)
        .build()
        .unwrap();
    (h, d)
}

fn manifest(b: &MemoryBackup) -> Json {
    serde_json::from_slice(&b.entries[MANIFEST]).unwrap()
}

fn set_manifest(b: &mut MemoryBackup, m: &Json) {
    b.entries
        .insert(MANIFEST.into(), serde_json::to_vec_pretty(m).unwrap());
}

/// A table's rows as its parts hold them, concatenated, in order.
fn rows_of(b: &MemoryBackup, table: &str) -> Vec<u8> {
    let m = manifest(b);
    let t = m["tables"]
        .as_array()
        .unwrap()
        .iter()
        .find(|t| t["name"] == table)
        .unwrap();
    let mut out = Vec::new();
    for p in t["parts"].as_array().unwrap() {
        out.extend_from_slice(&b.entries[p["entry"].as_str().unwrap()]);
    }
    out
}

#[tokio::test]
async fn a_table_split_into_parts_round_trips_the_same_rows() {
    let (_h, app) = split().await;
    let mut first = MemoryBackup::new();
    let report = backup::dump(app.deployment(), &mut first).await.unwrap();
    assert_eq!(report.manifest.format, 2);
    let audit = report
        .manifest
        .tables
        .iter()
        .find(|t| t.name == "audit_log")
        .unwrap();
    assert!(audit.parts.len() >= 3, "{audit:?}");
    for t in &report.manifest.tables {
        assert!(!t.parts.is_empty(), "{}: at least one part", t.name);
        assert!(t.sha256.is_empty(), "{}: pinned by its parts", t.name);
        for (i, p) in t.parts.iter().enumerate() {
            assert_eq!(p.entry, part_entry(&t.name, i));
            assert!(p.rows <= 2, "{}: a part is at most part_rows", p.entry);
            assert!(
                i == 0 || p.rows > 0,
                "{}: only a lone part is empty",
                p.entry
            );
        }
        assert_eq!(t.parts.iter().map(|p| p.rows).sum::<u64>(), t.rows);
    }
    assert!(first.entries.contains_key("tables/audit_log/00002.ndjson"));

    let (h2, d2) = fresh(TINY);
    let r = backup::load(&d2, &first, &LoadOptions::new())
        .await
        .unwrap();
    assert_eq!(
        r.rows,
        report.manifest.tables.iter().map(|t| t.rows).sum::<u64>()
    );
    let mut second = MemoryBackup::new();
    backup::dump(&d2, &mut second).await.unwrap();
    for t in &report.manifest.tables {
        let (before, after) = (rows_of(&first, &t.name), rows_of(&second, &t.name));
        match t.name.as_str() {
            // Redrawn by every restore.
            "menu_salt" => {}
            // The same rows plus the restore's own.
            "audit_log" => {
                let after = String::from_utf8(after).unwrap();
                for line in String::from_utf8(before).unwrap().lines() {
                    assert!(after.lines().any(|l| l == line), "{line}");
                }
                assert_eq!(after.lines().count() as u64, t.rows + 1);
            }
            _ => assert_eq!(before, after, "{}: the same bytes", t.name),
        }
    }
    let students = query(h2.db().as_ref(), "SELECT count(*) FROM student").await;
    assert_eq!(students.rows[0][0].as_i64(), Some(3));
}

/// Restores `b` over the populated Deployment, forced, expecting a refusal
/// whose message has `needle`, and that nothing changed.
async fn refused(app: &TestApp, h: &Harness, b: &MemoryBackup, needle: &str) {
    let before = counts(h.db().as_ref()).await;
    let keys = h.files().keys();
    let r = backup::load(app.deployment(), b, &LoadOptions::new().force(true)).await;
    assert!(
        matches!(&r, Err(Error::Invalid(m)) if m.contains(needle)),
        "{needle}: {r:?}"
    );
    assert_eq!(counts(h.db().as_ref()).await, before, "nothing wiped");
    assert_eq!(h.files().keys(), keys, "no object put");
}

#[tokio::test]
async fn a_corrupted_missing_or_reordered_part_is_refused_before_the_wipe() {
    let (h, app) = split().await;
    let mut good = MemoryBackup::new();
    backup::dump(app.deployment(), &mut good).await.unwrap();
    let part = "tables/audit_log/00002.ndjson";

    // One byte changed: same length, still JSON.
    let mut b = good.clone();
    let entry = b.entries.get_mut(part).unwrap();
    let at = entry.iter().position(|c| c.is_ascii_digit()).unwrap();
    entry[at] = if entry[at] == b'9' {
        b'8'
    } else {
        entry[at] + 1
    };
    refused(&app, &h, &b, &format!("{part} does not match its SHA-256")).await;

    let mut b = good.clone();
    b.entries.remove(part);
    refused(&app, &h, &b, &format!("backup entry {part} is missing")).await;

    // Parts listed out of order (each still matching its own bytes).
    let mut b = good.clone();
    let mut m = manifest(&b);
    let t = m["tables"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|t| t["name"] == "audit_log")
        .unwrap();
    t["parts"].as_array_mut().unwrap().swap(1, 2);
    set_manifest(&mut b, &m);
    refused(&app, &h, &b, "parts out of order").await;

    // A part's row count changed with the table's: the bytes disagree.
    let mut b = good.clone();
    let mut m = manifest(&b);
    let t = m["tables"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|t| t["name"] == "audit_log")
        .unwrap();
    t["rows"] = Json::from(t["rows"].as_u64().unwrap() - 1);
    let p = &mut t["parts"][1]["rows"];
    *p = Json::from(p.as_u64().unwrap() - 1);
    set_manifest(&mut b, &m);
    refused(&app, &h, &b, "tables/audit_log/00001.ndjson has 2 rows").await;

    // The table's rows no longer add up.
    let mut b = good.clone();
    let mut m = manifest(&b);
    m["tables"][0]["rows"] = Json::from(99);
    set_manifest(&mut b, &m);
    refused(&app, &h, &b, "its parts hold").await;

    // The good one goes in.
    backup::load(app.deployment(), &good, &LoadOptions::new().force(true))
        .await
        .unwrap();
}

/// `b` (one part per table) rewritten as a format-1 backup: the same
/// entries, each table pinned by `sha256` and without parts.
fn as_format_1(b: &mut MemoryBackup) {
    let mut m = manifest(b);
    m["format"] = Json::from(1);
    for t in m["tables"].as_array_mut().unwrap() {
        let parts = t["parts"].as_array().unwrap();
        assert_eq!(parts.len(), 1, "{}", t["name"]);
        assert_eq!(
            parts[0]["entry"],
            format!("tables/{}.ndjson", t["name"].as_str().unwrap())
        );
        t["sha256"] = parts[0]["sha256"].clone();
        t.as_object_mut().unwrap().remove("parts");
    }
    set_manifest(b, &m);
}

#[tokio::test]
async fn a_format_1_backup_still_restores() {
    let h = Harness::new();
    let d = deployment(&h);
    let (h, app) = populated_as(h, d).await;
    let mut b = MemoryBackup::new();
    backup::dump(app.deployment(), &mut b).await.unwrap();
    as_format_1(&mut b);

    let (fresh_h, d) = fresh("");
    let r = backup::load(&d, &b, &LoadOptions::new()).await.unwrap();
    assert!(r.rows > 0);
    let mut want = counts(h.db().as_ref()).await;
    let mut got = counts(fresh_h.db().as_ref()).await;
    for c in [&mut want, &mut got] {
        c.remove("audit_log");
    }
    assert_eq!(got, want);

    // Format 1 has no parts; another format is not read at all.
    let mut m = manifest(&b);
    m["tables"][0]["parts"] = serde_json::json!([{"entry": "x", "rows": 0, "sha256": ""}]);
    let mut bad = b.clone();
    set_manifest(&mut bad, &m);
    let (_, d) = fresh("");
    let r = backup::load(&d, &bad, &LoadOptions::new()).await;
    assert!(
        matches!(&r, Err(Error::Invalid(m)) if m.contains("format 1 does not")),
        "{r:?}"
    );
    let mut m = manifest(&b);
    m["format"] = Json::from(3);
    set_manifest(&mut bad, &m);
    let r = backup::load(&d, &bad, &LoadOptions::new()).await;
    assert!(
        matches!(&r, Err(Error::Invalid(m)) if m.contains("format 3")),
        "{r:?}"
    );
}

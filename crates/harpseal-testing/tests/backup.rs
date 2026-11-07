//! Backup and restore at the Deployment seam (#53): dump a Deployment with
//! real data, restore it into a fresh database and FileStore, and the
//! refusals (a non-empty database, a foreign migration).

mod backup_support;

use backup_support::{
    OFFICER, PASSWORD, count, counts, deployment, populated, query, row, text, triggers,
};
use harpseal::backup::{self, LoadOptions, Manifest, MemoryBackup};
use harpseal::{Error, FileStore, Statement};
use harpseal_testing::Harness;

#[tokio::test]
async fn a_backup_restores_into_a_fresh_database_and_filestore() {
    let (h, app) = populated().await;
    let before = counts(h.db().as_ref()).await;
    let student = "SELECT * FROM student WHERE student_code = '68010002'";
    let student_before = row(h.db().as_ref(), student).await;
    let file_key = text(&h, "SELECT file_key FROM roster_import").await;
    let file_before = h.files().get(&file_key).await.unwrap().unwrap();

    let mut b = MemoryBackup::new();
    let dumped = backup::dump(app.deployment(), &mut b).await.unwrap();
    assert!(dumped.missing_files.is_empty());
    assert_eq!(dumped.manifest.files.len(), 1);
    assert_eq!(
        dumped.manifest.migrations.len(),
        app.deployment().migrations().len()
    );
    assert!(b.entries.contains_key("tables/student.ndjson"));
    assert!(
        !b.entries.contains_key("tables/migration.ndjson"),
        "migrations are re-run, not dumped"
    );

    // A fresh database and FileStore, never migrated.
    let fresh = Harness::new();
    let d = deployment(&fresh);
    let r = backup::load(&d, &b, &LoadOptions::new()).await.unwrap();
    assert!(!r.forced && !r.wiped);
    assert_eq!(r.files, 1);
    assert_eq!(r.migrations_applied, d.migrations().len());
    let db = fresh.db();
    let mut after = counts(db.as_ref()).await;
    // The restore adds its own audit row.
    *after.get_mut("audit_log").unwrap() -= 1;
    assert_eq!(after, before);
    for t in ["student", "link", "announcement", "audit_log", "staff"] {
        assert!(before[t] > 0, "{t} has rows to compare");
    }

    assert_eq!(row(db.as_ref(), student).await, student_before);
    let file_after = fresh.files().get(&file_key).await.unwrap();
    assert_eq!(file_after.as_deref(), Some(file_before.as_slice()));

    let audit = query(
        db.as_ref(),
        "SELECT actor_kind, after, request_id FROM audit_log WHERE action = 'backup.restore'",
    )
    .await;
    assert_eq!(audit.len(), 1);
    assert_eq!(audit.rows[0][0].as_text(), Some("system"));
    assert_eq!(audit.rows[0][2].as_text(), Some("cli"));
    let after: serde_json::Value =
        serde_json::from_str(audit.rows[0][1].as_text().unwrap()).unwrap();
    assert_eq!(after["manifest_sha256"], dumped.manifest_sha256.as_str());
    assert_eq!(after["forced"], false);
    assert_eq!(after["files"], 1);
    assert_eq!(after["rows"], r.rows);

    // The password hash came along: the Officer still signs in.
    // Test-only: the fresh harness replays the same seeded Random from the
    // same START_MS, so its next UUIDv7s would repeat the restored ids. The
    // production Random is OS-seeded and cannot collide like this.
    fresh.clock().advance_minutes(24 * 60);
    let app = fresh.boot(d).await;
    let r = app.browser().login(OFFICER, PASSWORD).await;
    assert_eq!(r.status, 303, "{}", r.body);
}

#[tokio::test]
async fn a_non_empty_database_is_refused_unless_forced() {
    let (h, app) = populated().await;
    let mut b = MemoryBackup::new();
    let dumped = backup::dump(app.deployment(), &mut b).await.unwrap();
    let before = counts(h.db().as_ref()).await;
    let newest = query(
        h.db().as_ref(),
        "SELECT id, at FROM audit_log ORDER BY at DESC, id DESC LIMIT 1",
    )
    .await;
    let triggers_before = triggers(h.db().as_ref()).await;
    assert!(
        triggers_before
            .iter()
            .any(|(n, _)| n == "audit_log_no_delete"),
        "{triggers_before:?}"
    );

    let refused = backup::load(app.deployment(), &b, &LoadOptions::new()).await;
    assert!(
        matches!(&refused, Err(Error::Conflict(m)) if m.contains("not empty")),
        "{refused:?}"
    );
    assert_eq!(counts(h.db().as_ref()).await, before, "nothing changed");

    let r = backup::load(app.deployment(), &b, &LoadOptions::new().force(true))
        .await
        .unwrap();
    assert!(r.forced && r.wiped);
    let mut after = counts(h.db().as_ref()).await;
    *after.get_mut("audit_log").unwrap() -= 1;
    assert_eq!(after, before);

    // The append-only guards are back, unchanged, and still fire.
    let db = h.db();
    assert_eq!(triggers(db.as_ref()).await, triggers_before);
    let delete = db.execute(Statement::new("DELETE FROM audit_log")).await;
    assert!(delete.is_err(), "audit_log stays append-only");
    let audit = query(
        db.as_ref(),
        "SELECT json_extract(after, '$.forced'), json_extract(after, '$.manifest_sha256') \
         FROM audit_log WHERE action = 'backup.restore'",
    )
    .await;
    assert_eq!(audit.rows[0][0].as_i64(), Some(1));
    assert_eq!(
        audit.rows[0][1].as_text(),
        Some(dumped.manifest_sha256.as_str())
    );

    // What the wipe deleted is on record.
    let after: serde_json::Value = serde_json::from_str(
        &text(
            &h,
            "SELECT after FROM audit_log WHERE action = 'backup.restore'",
        )
        .await,
    )
    .unwrap();
    for (table, n) in &before {
        if *n > 0 {
            assert_eq!(after["deleted"][table], *n, "{table}");
            assert_eq!(r.deleted[table], *n as u64, "{table}");
        }
    }
    assert_eq!(
        after["newest_deleted_audit"]["id"].as_str(),
        newest.rows[0][0].as_text()
    );
    assert_eq!(
        after["newest_deleted_audit"]["at"].as_i64(),
        newest.rows[0][1].as_i64()
    );
    assert_eq!(after["dropped_sessions"], false);
}

#[tokio::test]
async fn a_backup_with_a_foreign_migration_is_refused() {
    let (_h, app) = populated().await;
    let mut b = MemoryBackup::new();
    backup::dump(app.deployment(), &mut b).await.unwrap();
    let edit = |b: &MemoryBackup, f: &dyn Fn(&mut serde_json::Value)| {
        let mut m: serde_json::Value = serde_json::from_slice(&b.entries["manifest.json"]).unwrap();
        f(&mut m);
        let mut b = b.clone();
        b.entries
            .insert("manifest.json".into(), serde_json::to_vec(&m).unwrap());
        b
    };
    let extra = edit(&b, &|m| {
        m["migrations"]
            .as_array_mut()
            .unwrap()
            .push(serde_json::json!({
                "module": "parking", "name": "0001_permits", "checksum": "00"
            }))
    });
    let changed = edit(&b, &|m| {
        m["migrations"][0]["checksum"] = "0".repeat(64).into();
    });
    for (what, b) in [("extra", extra), ("changed", changed)] {
        let fresh = Harness::new();
        let r = backup::load(&deployment(&fresh), &b, &LoadOptions::new()).await;
        assert!(
            matches!(&r, Err(Error::Invalid(m)) if m.contains("migration")),
            "{what}: {r:?}"
        );
        let tables = query(
            fresh.db().as_ref(),
            "SELECT count(*) FROM sqlite_master WHERE type = 'table'",
        )
        .await;
        assert_eq!(
            tables.rows[0][0].as_i64(),
            Some(0),
            "{what}: nothing written"
        );
    }
    assert!(Manifest::read(&b).await.is_ok());
}

#[tokio::test]
async fn a_missing_filestore_object_is_listed_and_the_restore_still_succeeds() {
    let (h, app) = populated().await;
    let key = text(&h, "SELECT file_key FROM roster_import").await;
    h.files().delete(&key).await.unwrap();

    let mut b = MemoryBackup::new();
    let dumped = backup::dump(app.deployment(), &mut b).await.unwrap();
    assert_eq!(dumped.missing_files, std::slice::from_ref(&key));
    let listed = &dumped.manifest.files[0];
    assert!(listed.missing && listed.sha256.is_none());
    assert!(!b.entries.contains_key(&listed.entry));

    let fresh = Harness::new();
    let r = backup::load(&deployment(&fresh), &b, &LoadOptions::new())
        .await
        .unwrap();
    assert_eq!((r.files, r.missing_files.clone()), (0, vec![key]));
    assert!(fresh.files().keys().is_empty());
    assert_eq!(count(fresh.db().as_ref(), "roster_import").await, 1);
}

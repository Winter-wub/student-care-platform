//! Backup integrity (#53 review): every entry is pinned by the manifest, a
//! failed wipe changes nothing, a restored Deployment dumps the same, an
//! INSERT trigger is refused, and `drop_sessions` ends restored sessions.

mod backup_support;

use backup_support::{OFFICER, PASSWORD, count, counts, deployment, populated, query, triggers};
use harpseal::backup::{self, LoadOptions, MemoryBackup};
use harpseal::{Error, Statement};
use harpseal_testing::Harness;

/// Every table of a migrated, empty database has no rows.
async fn all_empty(h: &Harness) -> bool {
    counts(h.db().as_ref()).await.values().all(|n| *n == 0)
}

#[tokio::test]
async fn a_flipped_byte_in_a_table_entry_is_refused_before_any_write() {
    let (_h, app) = populated().await;
    let mut b = MemoryBackup::new();
    backup::dump(app.deployment(), &mut b).await.unwrap();
    let entry = b.entries.get_mut("tables/student.ndjson").unwrap();
    // Same length, valid JSON still: only the hash can tell.
    let at = entry.iter().position(|c| *c == b'6').unwrap();
    entry[at] = b'7';

    let fresh = Harness::new();
    let d = deployment(&fresh);
    let r = backup::load(&d, &b, &LoadOptions::new()).await;
    assert!(
        matches!(&r, Err(Error::Invalid(m)) if m.contains("tables/student.ndjson") && m.contains("SHA-256")),
        "{r:?}"
    );
    assert!(all_empty(&fresh).await);
    assert!(fresh.files().keys().is_empty(), "no object put either");
}

#[tokio::test]
async fn a_failed_wipe_leaves_rows_and_triggers_as_they_were() {
    let (h, app) = populated().await;
    let mut b = MemoryBackup::new();
    backup::dump(app.deployment(), &mut b).await.unwrap();

    // A table outside the backup holds a row that points at a Student.
    let db = h.db();
    db.execute(Statement::new(
        "CREATE TABLE parking_permit (id TEXT PRIMARY KEY, \
         student_id TEXT NOT NULL REFERENCES student (id))",
    ))
    .await
    .unwrap();
    db.execute(Statement::new(
        "INSERT INTO parking_permit (id, student_id) SELECT 'p1', id FROM student LIMIT 1",
    ))
    .await
    .unwrap();
    let before = counts(db.as_ref()).await;
    let triggers_before = triggers(db.as_ref()).await;

    let r = backup::load(app.deployment(), &b, &LoadOptions::new().force(true)).await;
    assert!(
        matches!(&r, Err(Error::Db(m)) if m.contains("could not delete")),
        "{r:?}"
    );
    assert_eq!(counts(db.as_ref()).await, before, "the wipe rolled back");
    assert_eq!(triggers(db.as_ref()).await, triggers_before);
    assert!(
        db.execute(Statement::new("DELETE FROM audit_log"))
            .await
            .is_err()
    );
}

#[tokio::test]
async fn a_restored_deployment_dumps_the_same_entries() {
    let (_h, app) = populated().await;
    let mut first = MemoryBackup::new();
    let one = backup::dump(app.deployment(), &mut first).await.unwrap();

    let fresh = Harness::new();
    let d = deployment(&fresh);
    backup::load(&d, &first, &LoadOptions::new()).await.unwrap();
    let mut second = MemoryBackup::new();
    let two = backup::dump(&d, &mut second).await.unwrap();

    let names = |b: &MemoryBackup| b.entries.keys().cloned().collect::<Vec<_>>();
    assert_eq!(names(&first), names(&second));
    for (name, bytes) in &first.entries {
        match name.as_str() {
            "manifest.json" => {}
            "tables/audit_log.ndjson" => {
                // The same rows plus the restore's own (in key order, so
                // not necessarily last).
                let lines = |b: &[u8]| -> Vec<String> {
                    String::from_utf8_lossy(b)
                        .lines()
                        .map(String::from)
                        .collect()
                };
                let (before, mut again) = (lines(bytes), lines(&second.entries[name]));
                let extra: Vec<String> = again
                    .iter()
                    .filter(|l| !before.contains(l))
                    .cloned()
                    .collect();
                assert_eq!(extra.len(), 1, "{extra:?}");
                assert!(extra[0].contains("backup.restore"), "{extra:?}");
                again.retain(|l| *l != extra[0]);
                assert_eq!(again, before, "{name}");
            }
            _ => assert_eq!(&second.entries[name], bytes, "{name}"),
        }
    }
    assert_eq!(one.manifest.migrations, two.manifest.migrations);
    assert_eq!(one.manifest.files, two.manifest.files);
    for (a, b) in one.manifest.tables.iter().zip(&two.manifest.tables) {
        assert_eq!((&a.name, &a.columns, &a.sql), (&b.name, &b.columns, &b.sql));
        if a.name != "audit_log" {
            assert_eq!((a.rows, &a.sha256), (b.rows, &b.sha256), "{}", a.name);
        }
    }
}

#[tokio::test]
async fn an_insert_trigger_on_a_backed_up_table_is_refused() {
    let (_h, app) = populated().await;
    let mut b = MemoryBackup::new();
    backup::dump(app.deployment(), &mut b).await.unwrap();

    let fresh = Harness::new();
    let d = deployment(&fresh);
    let db = fresh.db();
    harpseal::migrate(db.as_ref(), d.migrations(), 1)
        .await
        .unwrap();
    db.execute(Statement::new(
        "CREATE TRIGGER tag_stamp AFTER INSERT ON tag BEGIN SELECT 1; END",
    ))
    .await
    .unwrap();
    let r = backup::load(&d, &b, &LoadOptions::new()).await;
    assert!(
        matches!(&r, Err(Error::Invalid(m)) if m.contains("tag_stamp")),
        "{r:?}"
    );
    assert!(all_empty(&fresh).await);
}

#[tokio::test]
async fn drop_sessions_ends_every_restored_session() {
    let (h, app) = populated().await;
    let b = app.browser();
    assert_eq!(b.login(OFFICER, PASSWORD).await.status, 303);
    assert_eq!(count(h.db().as_ref(), "staff_session").await, 1);
    let mut kept = MemoryBackup::new();
    backup::dump(app.deployment(), &mut kept).await.unwrap();

    let fresh = Harness::new();
    let d = deployment(&fresh);
    let r = backup::load(&d, &kept, &LoadOptions::new().drop_sessions(true))
        .await
        .unwrap();
    assert!(r.dropped_sessions);
    let db = fresh.db();
    assert_eq!(count(db.as_ref(), "staff_session").await, 0);
    assert_eq!(count(db.as_ref(), "app_session").await, 0);
    assert_eq!(count(db.as_ref(), "staff").await, 1, "Staff stay");
    let audit = query(
        db.as_ref(),
        "SELECT json_extract(after, '$.dropped_sessions') FROM audit_log \
         WHERE action = 'backup.restore'",
    )
    .await;
    assert_eq!(audit.rows[0][0].as_i64(), Some(1));
}

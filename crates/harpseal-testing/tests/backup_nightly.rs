//! The nightly backup Job (#78): it runs on `[backup] schedule`, writes
//! backup format 1 to the FileStore over several Job runs, restores with
//! `backup::load`, and deletes backups past `[backup] retention_days`
//! (with their objects) only after a successful backup.

mod backup_support;

use std::sync::Arc;

use backup_support::nightly::{DAY, NIGHT, night, runs, stamp_of};
use backup_support::{OFFICER, PASSWORD, count, counts, deployment, populated, query, text};
use harpseal::backup::{self, FileStoreBackup, LoadOptions};
use harpseal::{FileStore, Statement};
use harpseal_testing::{Harness, START_MS};

/// A `backup_run` row as an older Job would have left it.
async fn old_run(h: &Harness, id: &str, state: &str, prefix: &str, started: i64) {
    h.db()
        .execute(
            Statement::new(
                "INSERT INTO backup_run (id, prefix, state, origin, started_at, updated_at) \
                 VALUES (?, ?, ?, 'schedule', ?, ?)",
            )
            .bind(id)
            .bind(prefix)
            .bind(state)
            .bind(started)
            .bind(started),
        )
        .await
        .unwrap();
}

async fn audit(h: &Harness, action: &str) -> i64 {
    let sql = format!("SELECT count(*) FROM audit_log WHERE action = '{action}'");
    query(h.db().as_ref(), &sql).await.rows[0][0]
        .as_i64()
        .unwrap()
}

#[tokio::test]
async fn the_schedule_backs_up_and_retention_prunes_only_what_expired() {
    let (h, app) = populated().await;
    let d = app.deployment();
    let files: Arc<dyn FileStore> = Arc::new(h.files().clone());

    // A finished backup 40 days old, with its real objects.
    let mut old = FileStoreBackup::new(files.clone(), "backups/old-done/");
    backup::dump(d, &mut old).await.unwrap();
    old_run(
        &h,
        "r-old-done",
        "done",
        "backups/old-done/",
        START_MS - 40 * DAY,
    )
    .await;
    // A failed one, 35 days old: one table written, then its progress.
    files
        .put(
            "backups/old-failed/tables/student.ndjson",
            b"{}\n".to_vec(),
            "x",
        )
        .await
        .unwrap();
    let progress = r#"{"taken_at":0,"migrations":[],"order":["student","tag"],"tables":[],
        "keys":[],"files":[],"bytes":3}"#;
    files
        .put("backups/old-failed.progress.json", progress.into(), "x")
        .await
        .unwrap();
    old_run(
        &h,
        "r-old-failed",
        "failed",
        "backups/old-failed/",
        START_MS - 35 * DAY,
    )
    .await;
    // A finished one, 20 days old: kept.
    let mut recent = FileStoreBackup::new(files.clone(), "backups/recent/");
    backup::dump(d, &mut recent).await.unwrap();
    old_run(
        &h,
        "r-recent",
        "done",
        "backups/recent/",
        START_MS - 20 * DAY,
    )
    .await;

    // Nothing runs before the scheduled minute.
    h.clock().set_ms(NIGHT - 30 * 60_000);
    backup_support::nightly::ticks(&h, &app, 20).await;
    assert_eq!(count(h.db().as_ref(), "backup_run").await, 3);

    let before = counts(h.db().as_ref()).await;
    night(&h, &app, 0).await;
    let prefix = format!("backups/{}/", stamp_of(NIGHT));
    assert_eq!(
        runs(&h).await,
        [
            ("done".to_string(), "backups/recent/".to_string()),
            ("done".to_string(), prefix.clone())
        ],
        "the expired done and failed runs are gone"
    );
    let keys = h.files().keys();
    assert!(
        !keys.iter().any(|k| k.starts_with("backups/old-")),
        "{keys:?}"
    );
    assert!(keys.iter().any(|k| k.starts_with("backups/recent/")));
    assert!(keys.contains(&format!("{prefix}manifest.json")));
    let progress_key = format!("backups/{}.progress.json", stamp_of(NIGHT));
    assert!(!keys.contains(&progress_key), "the resume point is removed");

    let row = query(
        h.db().as_ref(),
        "SELECT progress, bytes, table_count, row_count, file_count, origin \
         FROM backup_run WHERE state = 'done' ORDER BY started_at DESC LIMIT 1",
    )
    .await;
    let int = |i: usize| row.rows[0][i].as_i64().unwrap();
    assert!(int(0) >= 2, "tables, then objects: at least two Job runs");
    assert!(int(1) > 0);
    assert_eq!(int(4), 1, "the roster file");
    assert_eq!(row.rows[0][5].as_text(), Some("schedule"));
    assert_eq!(audit(&h, "backup.create").await, 1);
    assert_eq!(audit(&h, "backup.prune").await, 2);
    let job_actor = text(
        &h,
        "SELECT actor_kind FROM audit_log WHERE action = 'backup.create'",
    )
    .await;
    assert_eq!(job_actor, "job");

    // The nightly backup restores into a fresh database and FileStore.
    let source = FileStoreBackup::new(files.clone(), prefix.clone());
    let fresh = Harness::new();
    // Test-only: the fresh harness replays the same seeded Random; a later
    // clock keeps its new UUIDv7s apart from the restored ones.
    fresh.clock().advance_minutes(3 * 24 * 60);
    let d2 = deployment(&fresh);
    let r = backup::load(&d2, &source, &LoadOptions::new())
        .await
        .unwrap();
    assert_eq!(r.files, 1);
    let after = counts(fresh.db().as_ref()).await;
    for t in ["student", "link", "announcement", "staff", "roster_import"] {
        assert_eq!(after[t], before[t], "{t}");
    }
    // The run that was writing this very backup is in it as `running`:
    // the restore marks it failed, so the copy never resumes it.
    let sql = format!("SELECT state, error FROM backup_run WHERE prefix = '{prefix}'");
    let restored = query(fresh.db().as_ref(), &sql).await;
    assert_eq!(restored.rows[0][0].as_text(), Some("failed"));
    assert!(restored.rows[0][1].as_text().unwrap().contains("restored"));
    let app2 = fresh.boot(d2).await;
    let login = app2.browser().login(OFFICER, PASSWORD).await;
    assert_eq!(login.status, 303, "{}", login.body);

    // The next night: nothing new expired (30 days not reached).
    night(&h, &app, 1).await;
    assert_eq!(runs(&h).await.len(), 3);
    assert_eq!(audit(&h, "backup.prune").await, 2);
}

#[tokio::test]
async fn a_disabled_schedule_runs_nothing() {
    let h = Harness::new();
    let toml = "name = \"Seal Bay University\"\ndefault_locale = \"th\"\n\
                utc_offset = \"+07:00\"\n[backup]\nenabled = false\n";
    let config = harpseal::DeploymentConfig::from_toml(toml).unwrap();
    let app = h.boot(h.builder().config(config).build().unwrap()).await;
    night(&h, &app, 0).await;
    assert_eq!(count(h.db().as_ref(), "backup_run").await, 0);
    assert!(h.files().keys().is_empty());
}

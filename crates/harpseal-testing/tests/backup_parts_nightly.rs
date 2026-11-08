//! The nightly backup Job with table parts (#88): a table far over the old
//! per-table limit backs up over several steps, and a step that dies after
//! writing some parts resumes into one complete backup, with no stray part
//! left in the FileStore.

mod backup_support;

use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use backup_support::nightly::{DAY, NIGHT, stamp_of, ticks};
use backup_support::{count, counts, deployment, deployment_extra, populated_as, query};
use harpseal::backup::{self, FileStoreBackup, LoadOptions, Manifest};
use harpseal::{Error, FileStore, Result, Statement};
use harpseal_testing::Harness;

/// The FileStore of `h`, failing the next `fails` `put`s of a key ending
/// in `when`.
struct Failing {
    files: Arc<dyn FileStore>,
    when: String,
    fails: Mutex<u32>,
}

impl Failing {
    fn new(h: &Harness, when: &str, fails: u32) -> Arc<Failing> {
        Arc::new(Failing {
            files: Arc::new(h.files().clone()),
            when: when.into(),
            fails: Mutex::new(fails),
        })
    }
    fn left(&self) -> u32 {
        *self.fails.lock().unwrap()
    }
    fn stop(&self) {
        *self.fails.lock().unwrap() = 0;
    }
}

#[async_trait]
impl FileStore for Failing {
    async fn put(&self, key: &str, bytes: Vec<u8>, content_type: &str) -> Result<()> {
        let fail = key.ends_with(&self.when) && {
            let mut left = self.fails.lock().unwrap();
            let fail = *left > 0;
            *left = left.saturating_sub(1);
            fail
        };
        if fail {
            return Err(Error::Platform(format!("the Worker died writing {key}")));
        }
        self.files.put(key, bytes, content_type).await
    }
    async fn get(&self, key: &str) -> Result<Option<Vec<u8>>> {
        self.files.get(key).await
    }
    async fn delete(&self, key: &str) -> Result<()> {
        self.files.delete(key).await
    }
}

/// The finished backup of the run at [`NIGHT`]: its manifest, and checks
/// that the FileStore holds exactly the entries it names.
async fn finished(h: &Harness) -> (FileStoreBackup, Manifest) {
    let row = query(h.db().as_ref(), "SELECT state, error FROM backup_run").await;
    assert_eq!(row.rows[0][0].as_text(), Some("done"), "{:?}", row.rows[0]);
    let prefix = format!("backups/{}/", stamp_of(NIGHT));
    let files: Arc<dyn FileStore> = Arc::new(h.files().clone());
    let b = FileStoreBackup::new(files, prefix.clone());
    let (m, _) = Manifest::read(&b).await.unwrap();
    let mut named: Vec<String> = m.entries().iter().map(|e| b.key(e)).collect();
    named.sort();
    let stored: Vec<String> = h
        .files()
        .keys()
        .into_iter()
        .filter(|k| k.starts_with(&prefix))
        .collect();
    assert_eq!(stored, named, "every object is an entry of the manifest");
    assert!(
        !h.files()
            .keys()
            .iter()
            .any(|k| k.ends_with(".progress.json")),
        "the progress is gone"
    );
    (b, m)
}

#[tokio::test]
async fn a_step_that_dies_mid_table_resumes_into_one_complete_backup() {
    let h = Harness::new();
    let extra = "[backup]\npart_rows = 2\n";
    let fail = Failing::new(&h, "tables/audit_log/00001.ndjson", 1);
    let mut services = h.services();
    services.files = fail.clone();
    let d = deployment_extra(&h, services, extra);
    let (h, app) = populated_as(h, d).await;

    h.clock().set_ms(NIGHT);
    for _ in 0..10 {
        // The failed step is an error of its tick: retried a minute later.
        app.deployment()
            .tick(harpseal::Timestamp(h.clock().now_ms()))
            .await
            .unwrap();
        h.clock().advance_minutes(1);
    }
    assert_eq!(fail.left(), 0, "the step did die");
    let (b, m) = finished(&h).await;
    let audit = m.tables.iter().find(|t| t.name == "audit_log").unwrap();
    assert!(audit.parts.len() >= 3, "{audit:?}");

    let fresh = Harness::new();
    let d = deployment_extra(&fresh, fresh.services(), extra);
    let r = backup::load(&d, &b, &LoadOptions::new()).await.unwrap();
    // Every row but the run's own `backup_run` row (never restored).
    let rows = m.tables.iter().filter(|t| t.name != "backup_run");
    assert_eq!(r.rows, rows.map(|t| t.rows).sum::<u64>());
    assert_eq!(count(fresh.db().as_ref(), "student").await, 3);
}

#[tokio::test]
async fn the_nightly_backup_takes_a_table_over_the_old_limit_in_parts() {
    let h = Harness::new();
    let d = deployment(&h);
    let (h, app) = populated_as(h, d).await;
    // 160,000 Tags: over the pre-#88 limit of 150,000 rows, in 20,000-row
    // parts, over steps of 50,000 rows.
    h.db()
        .execute(Statement::new(
            "WITH RECURSIVE n(i) AS (SELECT 1 UNION ALL SELECT i + 1 FROM n WHERE i < 160000) \
             INSERT INTO tag (id, name, created_at) \
             SELECT printf('tag-%06d', i), printf('ป้ายกำกับ %06d', i), 1 FROM n",
        ))
        .await
        .unwrap();
    h.clock().set_ms(NIGHT);
    ticks(&h, &app, 10).await;
    let (b, m) = finished(&h).await;
    let tag = m.tables.iter().find(|t| t.name == "tag").unwrap();
    assert_eq!(tag.rows, 160_000);
    assert!(tag.parts.len() >= 8, "{} parts", tag.parts.len());
    assert!(tag.parts.iter().all(|p| p.rows <= 20_000));
    let steps = query(h.db().as_ref(), "SELECT progress FROM backup_run").await;
    assert!(steps.rows[0][0].as_i64().unwrap() >= 4, "several steps");

    let fresh = Harness::new();
    let d = deployment(&fresh);
    backup::load(&d, &b, &LoadOptions::new()).await.unwrap();
    let mut want = counts(h.db().as_ref()).await;
    let mut got = counts(fresh.db().as_ref()).await;
    for c in [&mut want, &mut got] {
        // The restore's own audit row, the target's own backups, and the
        // backup's own Jobs, written while it ran.
        c.remove("audit_log");
        c.remove("backup_run");
        c.remove("job");
    }
    assert_eq!(got, want);
}

#[tokio::test]
async fn a_run_abandoned_mid_table_is_pruned_without_a_stray_part() {
    let h = Harness::new();
    let fail = Failing::new(&h, "tables/audit_log/00002.ndjson", u32::MAX);
    let mut services = h.services();
    services.files = fail.clone();
    let d = deployment_extra(&h, services, "[backup]\npart_rows = 2\n");
    let (h, app) = populated_as(h, d).await;
    h.clock().set_ms(NIGHT);
    for _ in 0..45 {
        let _ = app
            .deployment()
            .tick(harpseal::Timestamp(h.clock().now_ms()))
            .await;
        h.clock().advance_minutes(1);
    }
    let old = format!("backups/{}/", stamp_of(NIGHT));
    let state = query(h.db().as_ref(), "SELECT state FROM backup_run").await;
    assert_eq!(state.rows[0][0].as_text(), Some("failed"));
    let written = |h: &Harness| -> Vec<String> {
        let keys = h.files().keys();
        keys.into_iter().filter(|k| k.starts_with(&old)).collect()
    };
    assert!(
        written(&h).contains(&format!("{old}tables/audit_log/00001.ndjson")),
        "{:?}",
        written(&h)
    );

    // A month later a good backup prunes it: every part it wrote goes.
    fail.stop();
    h.clock().set_ms(NIGHT + 31 * DAY);
    ticks(&h, &app, 10).await;
    assert_eq!(written(&h), Vec::<String>::new());
    let gone = format!("backups/{}.progress.json", stamp_of(NIGHT));
    assert!(!h.files().keys().contains(&gone));
}

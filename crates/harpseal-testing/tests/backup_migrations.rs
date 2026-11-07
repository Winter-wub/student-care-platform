//! Backups across migration histories (#53 review): the applied order
//! interleaves Modules, and migrations newer than the backup run over the
//! restored rows.

mod backup_support;

use backup_support::{count, counts, deployment, populated_on, query};
use harpseal::backup::{self, LoadOptions, MemoryBackup};
use harpseal::{Deployment, Module, ModuleRegistrar, Statement};
use harpseal_testing::Harness;

#[tokio::test]
async fn a_kernel_migration_applied_after_a_modules_still_restores() {
    // Migrated when the Kernel stopped at 0006 (with Announcements), then
    // upgraded: kernel/0007 is applied after announcements/0001.
    let h = Harness::new();
    let d = deployment(&h);
    let all = d.migrations();
    let last_kernel = all.iter().rposition(|m| m.module == "kernel").unwrap();
    let older: Vec<_> = all
        .iter()
        .enumerate()
        .filter(|(i, _)| *i != last_kernel)
        .map(|(_, m)| *m)
        .collect();
    assert!(older.iter().any(|m| m.module == "announcements"));
    harpseal::migrate(h.db().as_ref(), &older, 1).await.unwrap();
    let (h, app) = populated_on(h).await;
    let applied = query(
        h.db().as_ref(),
        "SELECT module, name FROM migration ORDER BY rowid",
    )
    .await;
    let last = applied.rows.last().unwrap();
    assert_eq!(last[0].as_text(), Some("kernel"), "kernel applied last");
    assert_eq!(last[1].as_text(), Some(all[last_kernel].name));

    let mut b = MemoryBackup::new();
    backup::dump(app.deployment(), &mut b).await.unwrap();
    let fresh = Harness::new();
    let r = backup::load(&deployment(&fresh), &b, &LoadOptions::new())
        .await
        .unwrap();
    assert_eq!(r.migrations_applied, all.len());
    let mut after = counts(fresh.db().as_ref()).await;
    *after.get_mut("audit_log").unwrap() -= 1;
    assert_eq!(after, counts(h.db().as_ref()).await);
}

const PERMITS_0001: &str =
    "CREATE TABLE permits_permit (id TEXT PRIMARY KEY, plate TEXT NOT NULL);";
const PERMITS_0002: &str = "ALTER TABLE permits_permit ADD COLUMN zone TEXT;\n\
     UPDATE permits_permit SET zone = 'A-' || plate;";

/// A Module whose second migration changes data.
struct Permits {
    v2: bool,
}

impl Module for Permits {
    fn name(&self) -> &'static str {
        "permits"
    }
    fn register(&self, r: &mut ModuleRegistrar) {
        r.migration("0001_permit", PERMITS_0001);
        if self.v2 {
            r.migration("0002_zone", PERMITS_0002);
        }
    }
}

fn with_permits(h: &Harness, v2: bool) -> Deployment {
    h.builder().module(Permits { v2 }).build().unwrap()
}

#[tokio::test]
async fn newer_migrations_run_over_the_restored_rows() {
    let h = Harness::new();
    let old = with_permits(&h, false);
    old.migrate().await.unwrap();
    h.db()
        .execute(Statement::new(
            "INSERT INTO permits_permit (id, plate) VALUES ('p1', 'กข-1234')",
        ))
        .await
        .unwrap();
    let mut b = MemoryBackup::new();
    let dumped = backup::dump(&old, &mut b).await.unwrap();
    let permit = dumped
        .manifest
        .tables
        .iter()
        .find(|t| t.name == "permits_permit")
        .unwrap();
    assert_eq!(permit.columns, ["id", "plate"]);

    let fresh = Harness::new();
    let new = with_permits(&fresh, true);
    let r = backup::load(&new, &b, &LoadOptions::new()).await.unwrap();
    assert_eq!(r.migrations_applied, new.migrations().len());
    let rows = query(fresh.db().as_ref(), "SELECT zone FROM permits_permit").await;
    assert_eq!(
        rows.rows[0][0].as_text(),
        Some("A-กข-1234"),
        "0002 ran after the rows went in"
    );
    assert_eq!(count(fresh.db().as_ref(), "permits_permit").await, 1);
}

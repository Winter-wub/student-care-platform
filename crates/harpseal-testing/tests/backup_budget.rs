//! A nightly backup step's limits (#87 review): it stays inside its
//! subrequest budget, and a table with a NULL primary key refuses the dump
//! (keyset paging would stop at it) instead of cutting the table short.

mod backup_support;

use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use backup_support::nightly::NIGHT;
use backup_support::{deployment_with, populated, query};
use harpseal::backup::{self, MemoryBackup};
use harpseal::{Db, ExecResult, FileStore, Result, Rows, Statement, Timestamp};

/// Counts the database queries and FileStore calls made through it.
struct Counting {
    db: Arc<dyn Db>,
    files: Arc<dyn FileStore>,
    calls: Arc<Mutex<u64>>,
}

impl Counting {
    fn tick(&self) {
        *self.calls.lock().unwrap() += 1;
    }
}

#[async_trait]
impl Db for Counting {
    async fn query(&self, stmt: Statement) -> Result<Rows> {
        self.tick();
        self.db.query(stmt).await
    }
    async fn execute(&self, stmt: Statement) -> Result<ExecResult> {
        self.tick();
        self.db.execute(stmt).await
    }
    async fn batch(&self, stmts: Vec<Statement>) -> Result<Vec<ExecResult>> {
        self.tick();
        self.db.batch(stmts).await
    }
}

#[async_trait]
impl FileStore for Counting {
    async fn put(&self, key: &str, bytes: Vec<u8>, content_type: &str) -> Result<()> {
        self.tick();
        self.files.put(key, bytes, content_type).await
    }
    async fn get(&self, key: &str) -> Result<Option<Vec<u8>>> {
        self.tick();
        self.files.get(key).await
    }
    async fn delete(&self, key: &str) -> Result<()> {
        self.tick();
        self.files.delete(key).await
    }
}

#[tokio::test]
async fn a_step_stays_inside_its_subrequest_budget() {
    let (h, _app) = populated().await;
    // 190 roster files: copying them takes 380 calls, more than one step's
    // 350, though fewer than its 200-object limit.
    let staff = backup_support::text(&h, "SELECT id FROM staff LIMIT 1").await;
    let mut rows = Vec::new();
    for i in 0..190 {
        let key = format!("roster/many-{i:03}.csv");
        h.files()
            .put(&key, b"a,b\n".to_vec(), "text/csv")
            .await
            .unwrap();
        rows.push(
            Statement::new(
                "INSERT INTO roster_import (id, staff_id, mode, state, file_key, file_name, \
                 created_at) VALUES (?, ?, 'upsert', 'done', ?, 'many.csv', 1)",
            )
            .bind(format!("ri-many-{i:03}"))
            .bind(staff.as_str())
            .bind(key),
        );
    }
    h.db().batch(rows).await.unwrap();

    let calls = Arc::new(Mutex::new(0u64));
    let counting = Arc::new(Counting {
        db: h.db(),
        files: Arc::new(h.files().clone()),
        calls: calls.clone(),
    });
    let mut services = h.services();
    services.db = counting.clone();
    services.files = counting;
    let d = deployment_with(&h, services);
    h.clock().set_ms(NIGHT);
    let mut per_tick = Vec::new();
    for _ in 0..10 {
        *calls.lock().unwrap() = 0;
        d.tick(Timestamp(h.clock().now_ms())).await.unwrap();
        per_tick.push(*calls.lock().unwrap());
        h.clock().advance_minutes(1);
    }
    let row = query(
        h.db().as_ref(),
        "SELECT state, progress, file_count FROM backup_run",
    )
    .await;
    assert_eq!(row.rows[0][0].as_text(), Some("done"), "{per_tick:?}");
    assert!(
        row.rows[0][1].as_i64().unwrap() >= 3,
        "tables, then two steps of files"
    );
    assert!(row.rows[0][2].as_i64().unwrap() >= 191);
    // Each tick: one step plus the tick's own work, well inside 1,000.
    assert!(per_tick.iter().all(|&n| n <= 450), "{per_tick:?}");
}

#[tokio::test]
async fn a_null_primary_key_refuses_the_dump_naming_the_table() {
    let (h, app) = populated().await;
    h.db()
        .execute(Statement::new(
            "INSERT INTO tag (id, name, created_at) VALUES (NULL, 'no key', 1)",
        ))
        .await
        .unwrap();
    let err = backup::dump(app.deployment(), &mut MemoryBackup::new())
        .await
        .unwrap_err()
        .to_string();
    assert!(
        err.contains("table tag") && err.contains("NULL primary key"),
        "{err}"
    );
}

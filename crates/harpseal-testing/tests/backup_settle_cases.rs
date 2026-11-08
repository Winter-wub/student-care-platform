//! The cross-step key check (`settle.rs`, #88 review) at its edges: a key
//! range that grew past twice a part fails early naming the table; a key
//! that keeps repeating fails the run after three rounds, naming the
//! index; saved hashes that no longer match their part are not used (the
//! part is read again); an expression and partial index on a Module's own
//! table is checked like the Kernel's.

mod backup_support;

use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use backup_support::nightly::{NIGHT, stamp_of, ticks};
use backup_support::{deployment, deployment_with, populated, query};
use harpseal::backup::{self, FileStoreBackup, LoadOptions, MemoryBackup};
use harpseal::{Db, ExecResult, FileStore, Result, Rows, Statement, Timestamp, Value};
use harpseal_testing::Harness;
use serde_json::Value as Json;

async fn exec(db: &dyn Db, sql: &str) {
    db.execute(Statement::new(sql.to_string())).await.unwrap();
}

/// `n` Tags `tag-000001`…, named `ป้าย 000001`….
async fn tags(db: &dyn Db, n: i64) {
    exec(db, &format!(
        "WITH RECURSIVE n(i) AS (SELECT 1 UNION ALL SELECT i + 1 FROM n WHERE i < {n}) \
         INSERT INTO tag (id, name, created_at) SELECT printf('tag-%06d', i), printf('ป้าย %06d', i), 1 FROM n"
    ))
    .await;
}

/// The saved progress of the run at [`NIGHT`], if any.
async fn progress(h: &Harness) -> Option<Json> {
    let key = format!("backups/{}.progress.json", stamp_of(NIGHT));
    let raw = h.files().get(&key).await.unwrap()?;
    Some(serde_json::from_slice(&raw).unwrap())
}

/// Ticks a minute at a time until the run has stopped inside `table`
/// after saving hashes; `true` once it has.
async fn until_inside(h: &Harness, d: &harpseal::Deployment, table: &str) -> bool {
    for _ in 0..20 {
        d.tick(Timestamp(h.clock().now_ms())).await.unwrap();
        h.clock().advance_minutes(1);
        let Some(p) = progress(h).await else { continue };
        let done = p["tables"].as_array().unwrap().len();
        if p["order"][done] == table && p["cursor"]["segments"].as_u64().unwrap_or(0) > 0 {
            return true;
        }
    }
    false
}

/// `(state, error)` of the only backup run.
async fn run_state(h: &Harness) -> (String, String) {
    let row = query(h.db().as_ref(), "SELECT state, error FROM backup_run").await;
    let t = |i: usize| row.rows[0][i].as_text().unwrap_or_default().to_string();
    (t(0), t(1))
}

#[tokio::test]
async fn a_key_range_that_grew_past_twice_a_part_fails_naming_the_table() {
    let (h, app) = populated().await;
    tags(h.db().as_ref(), 60_000).await;
    h.clock().set_ms(NIGHT);
    assert!(until_inside(&h, app.deployment(), "tag").await);
    // Between two steps: tag-000002 (in part 1, read) gives its name to a
    // Tag read later, and 50,000 Tags land inside part 1's key range.
    let db = h.db();
    exec(
        db.as_ref(),
        "UPDATE tag SET name = 'ย้ายแล้ว' WHERE id = 'tag-000002'",
    )
    .await;
    exec(
        db.as_ref(),
        "INSERT INTO tag (id, name, created_at) VALUES ('tag-zzzzzz', 'ป้าย 000002', 2)",
    )
    .await;
    exec(db.as_ref(),
        "WITH RECURSIVE n(i) AS (SELECT 1 UNION ALL SELECT i + 1 FROM n WHERE i < 50000) \
         INSERT INTO tag (id, name, created_at) SELECT printf('tag-000001-%05d', i), printf('ใหม่ %05d', i), 2 FROM n",
    )
    .await;
    ticks(&h, &app, 45).await;
    let (state, error) = run_state(&h).await;
    assert_eq!(state, "failed", "{error}");
    assert!(
        error.contains("table tag: part 1 must be read again"),
        "{error}"
    );
    assert!(error.contains("more than twice a part"), "{error}");
}

/// The real database, except that Tag `tag-b` reads with `tag-a`'s name
/// (and key), every time: a repeat that never goes away.
struct Lying(Arc<dyn Db>);

#[async_trait]
impl Db for Lying {
    async fn query(&self, stmt: Statement) -> Result<Rows> {
        let tag = stmt.sql.contains("FROM \"tag\"");
        let mut rows = self.0.query(stmt).await?;
        if tag {
            let at = |c: &str| rows.columns.iter().position(|n| n == c);
            if let (Some(id), Some(name)) = (at("id"), at("name")) {
                let key = at("__hs_0");
                for r in &mut rows.rows {
                    if r[id].as_text() == Some("tag-b") {
                        r[name] = Value::Text("ซ้ำ".into());
                        if let Some(k) = key {
                            r[k] = Value::Text("ซ้ำ".into());
                        }
                    }
                }
            }
        }
        Ok(rows)
    }
    async fn execute(&self, stmt: Statement) -> Result<ExecResult> {
        self.0.execute(stmt).await
    }
    async fn batch(&self, stmts: Vec<Statement>) -> Result<Vec<ExecResult>> {
        self.0.batch(stmts).await
    }
}

#[tokio::test]
async fn a_key_that_keeps_repeating_fails_the_run_naming_the_index() {
    let (h, _app) = populated().await;
    exec(
        h.db().as_ref(),
        "INSERT INTO tag (id, name, created_at) VALUES \
        ('tag-a', 'ซ้ำ', 1), ('tag-b', 'ไม่ซ้ำ', 1)",
    )
    .await;
    let mut services = h.services();
    services.db = Arc::new(Lying(h.db()));
    let d = deployment_with(&h, services);
    h.clock().set_ms(NIGHT);
    for _ in 0..45 {
        let _ = d.tick(Timestamp(h.clock().now_ms())).await;
        h.clock().advance_minutes(1);
    }
    let (state, error) = run_state(&h).await;
    assert_eq!(state, "failed", "{error}");
    assert!(error.contains("table tag kept changing"), "{error}");
    assert!(error.contains("sqlite_autoindex_tag_2"), "{error}");
    assert!(
        error.contains("next scheduled backup starts over"),
        "{error}"
    );
}

/// The real database, counting re-reads of a part of `tag` (`<=` bound).
struct Counting(Arc<dyn Db>, Arc<Mutex<u32>>);

#[async_trait]
impl Db for Counting {
    async fn query(&self, stmt: Statement) -> Result<Rows> {
        if stmt.sql.contains("FROM \"tag\"") && stmt.sql.contains(") <= (") {
            *self.1.lock().unwrap() += 1;
        }
        self.0.query(stmt).await
    }
    async fn execute(&self, stmt: Statement) -> Result<ExecResult> {
        self.0.execute(stmt).await
    }
    async fn batch(&self, stmts: Vec<Statement>) -> Result<Vec<ExecResult>> {
        self.0.batch(stmts).await
    }
}

#[tokio::test]
async fn saved_hashes_that_do_not_match_their_part_make_it_read_again() {
    let (h, _app) = populated().await;
    tags(h.db().as_ref(), 60_000).await;
    let rereads = Arc::new(Mutex::new(0));
    let mut services = h.services();
    services.db = Arc::new(Counting(h.db(), rereads.clone()));
    let d = deployment_with(&h, services);
    h.clock().set_ms(NIGHT);
    assert!(until_inside(&h, &d, "tag").await);
    // The first saved part's SHA-256, one byte off: as if its part had
    // been rewritten after the hashes were saved.
    let key = format!("backups/{}.check/tag/00000.keys", stamp_of(NIGHT));
    let mut raw = h.files().get(&key).await.unwrap().unwrap();
    raw[64 + 4] ^= 1;
    h.files()
        .put(&key, raw, "application/octet-stream")
        .await
        .unwrap();
    assert_eq!(*rereads.lock().unwrap(), 0);
    for _ in 0..10 {
        d.tick(Timestamp(h.clock().now_ms())).await.unwrap();
        h.clock().advance_minutes(1);
    }
    assert_eq!(run_state(&h).await.0, "done");
    assert!(*rereads.lock().unwrap() > 0, "the part was read again");
    let files: Arc<dyn FileStore> = Arc::new(h.files().clone());
    let b = FileStoreBackup::new(files, format!("backups/{}/", stamp_of(NIGHT)));
    let fresh = Harness::new();
    backup::load(&deployment(&fresh), &b, &LoadOptions::new())
        .await
        .unwrap();
    let n = query(fresh.db().as_ref(), "SELECT count(*) FROM tag").await;
    assert_eq!(n.rows[0][0].as_i64(), Some(60_000));
}

const PERMITS: &str = "CREATE TABLE parking_permit (id TEXT PRIMARY KEY, plate TEXT NOT NULL, \
    status TEXT NOT NULL)";
const LIVE_PLATE: &str = "CREATE UNIQUE INDEX permit_live_plate ON parking_permit (lower(plate)) \
    WHERE status <> 'void'";

/// The real database; after the first page of `parking_permit` is read,
/// permit `p-0001` (`PL-0001`) is voided and a permit read later gets
/// `pl-0001`: the same key under `lower()`.
struct Replating(Arc<dyn Db>, Mutex<bool>);

#[async_trait]
impl Db for Replating {
    async fn query(&self, stmt: Statement) -> Result<Rows> {
        let page = stmt.sql.contains("FROM \"parking_permit\"") && stmt.sql.contains("LIMIT");
        let rows = self.0.query(stmt).await?;
        if page && !std::mem::replace(&mut *self.1.lock().unwrap(), true) {
            self.0
                .batch(vec![
                    Statement::new("UPDATE parking_permit SET status = 'void' WHERE id = 'p-0001'"),
                    Statement::new(
                        "INSERT INTO parking_permit (id, plate, status) \
                         VALUES ('p-9999', 'pl-0001', 'live')",
                    ),
                ])
                .await?;
        }
        Ok(rows)
    }
    async fn execute(&self, stmt: Statement) -> Result<ExecResult> {
        self.0.execute(stmt).await
    }
    async fn batch(&self, stmts: Vec<Statement>) -> Result<Vec<ExecResult>> {
        self.0.batch(stmts).await
    }
}

#[tokio::test]
async fn an_expression_and_partial_index_of_a_module_table_is_checked_too() {
    let (h, _app) = populated().await;
    let db = h.db();
    exec(db.as_ref(), PERMITS).await;
    exec(db.as_ref(), LIVE_PLATE).await;
    exec(
        db.as_ref(),
        "WITH RECURSIVE n(i) AS (SELECT 1 UNION ALL SELECT i + 1 FROM n WHERE i < 600) \
         INSERT INTO parking_permit (id, plate, status) \
         SELECT printf('p-%04d', i), printf('PL-%04d', i), 'live' FROM n",
    )
    .await;
    let mut services = h.services();
    services.db = Arc::new(Replating(h.db(), Mutex::new(false)));
    let d = deployment_with(&h, services);
    let mut b = MemoryBackup::new();
    backup::dump(&d, &mut b).await.unwrap();

    // Into a fresh database with the Module's table: it restores, with
    // p-0001 as it was when read again.
    let fresh = Harness::new();
    let d2 = deployment(&fresh);
    let fdb = fresh.db();
    harpseal::migrate(fdb.as_ref(), d2.migrations(), 1)
        .await
        .unwrap();
    exec(fdb.as_ref(), PERMITS).await;
    exec(fdb.as_ref(), LIVE_PLATE).await;
    backup::load(&d2, &b, &LoadOptions::new()).await.unwrap();
    let got = query(
        fdb.as_ref(),
        "SELECT id, status FROM parking_permit WHERE lower(plate) = 'pl-0001' ORDER BY id",
    )
    .await;
    let got: Vec<_> = got
        .rows
        .iter()
        .map(|r| (r[0].as_text().unwrap(), r[1].as_text().unwrap()))
        .collect();
    assert_eq!(got, [("p-0001", "void"), ("p-9999", "live")]);
}

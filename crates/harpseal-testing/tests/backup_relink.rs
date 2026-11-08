//! A Student who unlinks and links again while the backup reads `link`
//! (#78 review, #88): the dump reads the stale Link again, so the backup
//! restores; a backup that still holds two live Links (spliced from two
//! genuine ones) is refused by the partial unique index before any write.

mod backup_support;

use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use backup_support::nightly::{NIGHT, stamp_of};
use backup_support::{deployment, deployment_with, populated, query};
use harpseal::backup::{self, FileStoreBackup, LoadOptions, MemoryBackup};
use harpseal::{Db, ExecResult, FileStore, Result, Rows, Statement, Timestamp};
use harpseal_testing::Harness;

/// The real database; a Student unlinks and links again (same account)
/// right after the dump read the first page of `link`.
struct Relinking {
    inner: Arc<dyn Db>,
    done: Mutex<bool>,
}

#[async_trait]
impl Db for Relinking {
    async fn query(&self, stmt: Statement) -> Result<Rows> {
        let first_page = stmt.sql.contains("FROM \"link\"") && stmt.sql.contains("LIMIT");
        let page = self.inner.query(stmt).await?;
        if first_page && !std::mem::replace(&mut *self.done.lock().unwrap(), true) {
            self.inner
                .batch(vec![
                    Statement::new("UPDATE link SET status = 'revoked' WHERE id = 'l-0001'"),
                    Statement::new(
                        "INSERT INTO link (id, student_id, channel_account_id, channel, status, \
                         method, linked_at, updated_at) \
                         VALUES ('l-9999', 'st-0001', 'ca-0001', 'fake', 'active', 'manual', 2, 2)",
                    ),
                ])
                .await?;
        }
        Ok(page)
    }
    async fn execute(&self, stmt: Statement) -> Result<ExecResult> {
        self.inner.execute(stmt).await
    }
    async fn batch(&self, stmts: Vec<Statement>) -> Result<Vec<ExecResult>> {
        self.inner.batch(stmts).await
    }
}

/// Students `st-0000`… with live Links `l-0000`… (600, more than a page).
async fn many_links(db: &dyn Db) {
    let mut setup = Vec::new();
    for i in 0..600 {
        setup.push(Statement::new(format!(
            "INSERT INTO student (id, student_code, display_name, locale, created_at, updated_at) \
             VALUES ('st-{i:04}', '6902{i:04}', 'นักศึกษา {i}', 'th', 1, 1)"
        )));
        setup.push(Statement::new(format!(
            "INSERT INTO channel_account (id, channel, external_id, first_seen_at, updated_at) \
             VALUES ('ca-{i:04}', 'fake', 'u-relink-{i:04}', 1, 1)"
        )));
        setup.push(Statement::new(format!(
            "INSERT INTO link (id, student_id, channel_account_id, channel, status, method, \
             linked_at, updated_at) VALUES ('l-{i:04}', 'st-{i:04}', 'ca-{i:04}', 'fake', \
             'active', 'manual', 1, 1)"
        )));
    }
    db.batch(setup).await.unwrap();
}

#[tokio::test]
async fn a_relink_during_the_backup_is_read_again_so_the_backup_restores() {
    let (h, _app) = populated().await;
    let db = h.db();
    many_links(db.as_ref()).await;
    let mut services = h.services();
    services.db = Arc::new(Relinking {
        inner: db.clone(),
        done: Mutex::new(false),
    });
    let d = deployment_with(&h, services);
    h.clock().set_ms(NIGHT);
    for _ in 0..10 {
        d.tick(Timestamp(h.clock().now_ms())).await.unwrap();
        h.clock().advance_minutes(1);
    }
    let state = query(db.as_ref(), "SELECT state FROM backup_run").await;
    assert_eq!(state.rows[0][0].as_text(), Some("done"));
    let files: Arc<dyn FileStore> = Arc::new(h.files().clone());
    let source = FileStoreBackup::new(files, format!("backups/{}/", stamp_of(NIGHT)));

    // l-0001 was read live, then revoked and replaced by l-9999 (read
    // later): the dump read l-0001 again, so it is revoked in the backup.
    let fresh = Harness::new();
    let d2 = deployment(&fresh);
    backup::load(&d2, &source, &LoadOptions::new())
        .await
        .unwrap();
    let links = query(
        fresh.db().as_ref(),
        "SELECT id, status FROM link WHERE student_id = 'st-0001' ORDER BY id",
    )
    .await;
    let got: Vec<_> = links
        .rows
        .iter()
        .map(|r| (r[0].as_text().unwrap(), r[1].as_text().unwrap()))
        .collect();
    assert_eq!(got, [("l-0001", "revoked"), ("l-9999", "active")]);
}

#[tokio::test]
async fn a_backup_with_two_live_links_is_refused_by_the_partial_unique_index() {
    let h = Harness::new();
    let d = backup_support::deployment_extra(&h, h.services(), "[backup]\npart_rows = 100\n");
    let (h, app) = backup_support::populated_as(h, d).await;
    let db = h.db();
    many_links(db.as_ref()).await;
    // Two genuine backups, before and after a relink; the first part of
    // `link` from the one before, the rest from the one after: every part
    // matches its SHA-256, and l-0001 and l-9999 are both live.
    let mut before = MemoryBackup::new();
    backup::dump(app.deployment(), &mut before).await.unwrap();
    db.batch(vec![
        Statement::new("UPDATE link SET status = 'revoked' WHERE id = 'l-0001'"),
        Statement::new(
            "INSERT INTO link (id, student_id, channel_account_id, channel, status, method, \
             linked_at, updated_at) \
             VALUES ('l-9999', 'st-0001', 'ca-0001', 'fake', 'active', 'manual', 2, 2)",
        ),
    ])
    .await
    .unwrap();
    let mut spliced = MemoryBackup::new();
    let after = backup::dump(app.deployment(), &mut spliced).await.unwrap();
    let old = before.entries["tables/link.ndjson"].clone();
    assert!(String::from_utf8_lossy(&old).contains("\"l-0001\""));
    spliced.entries.insert("tables/link.ndjson".into(), old);
    let mut m: serde_json::Value =
        serde_json::from_slice(&spliced.entries["manifest.json"]).unwrap();
    let first: serde_json::Value =
        serde_json::from_slice(&before.entries["manifest.json"]).unwrap();
    let find = |m: &serde_json::Value| {
        m["tables"]
            .as_array()
            .unwrap()
            .iter()
            .position(|t| t["name"] == "link")
            .unwrap()
    };
    let (i, j) = (find(&m), find(&first));
    assert!(after.manifest.tables[i].parts.len() > 1);
    let p0 = first["tables"][j]["parts"][0].clone();
    let rows = m["tables"][i]["rows"].as_u64().unwrap()
        - m["tables"][i]["parts"][0]["rows"].as_u64().unwrap()
        + p0["rows"].as_u64().unwrap();
    m["tables"][i]["parts"][0] = p0;
    m["tables"][i]["rows"] = rows.into();
    spliced
        .entries
        .insert("manifest.json".into(), serde_json::to_vec(&m).unwrap());

    // Into a fresh database: refused before any write, naming the index.
    let fresh = Harness::new();
    let d2 = deployment(&fresh);
    let err = backup::load(&d2, &spliced, &LoadOptions::new())
        .await
        .unwrap_err();
    let text = err.to_string();
    assert!(text.contains("nothing was restored"), "{text}");
    assert!(text.contains("link_live_student_channel"), "{text}");
    let students = query(fresh.db().as_ref(), "SELECT count(*) FROM student").await;
    assert_eq!(students.rows[0][0].as_i64(), Some(0));
    // Over the source itself, forced: the wipe never runs.
    let count = query(db.as_ref(), "SELECT count(*) FROM link").await;
    let forced = backup::load(app.deployment(), &spliced, &LoadOptions::new().force(true)).await;
    assert!(forced.is_err());
    assert_eq!(
        query(db.as_ref(), "SELECT count(*) FROM link").await.rows,
        count.rows
    );
}

/// The FileStore of `h`, failing the second `put` of a key ending in
/// `when` (the rewrite of a part read again).
struct FailRewrite {
    files: Arc<dyn FileStore>,
    when: &'static str,
    puts: Mutex<u32>,
}

#[async_trait]
impl FileStore for FailRewrite {
    async fn put(&self, key: &str, bytes: Vec<u8>, content_type: &str) -> Result<()> {
        if key.ends_with(self.when) {
            let n = {
                let mut puts = self.puts.lock().unwrap();
                *puts += 1;
                *puts
            };
            if n == 2 {
                return Err(harpseal::Error::Platform(format!(
                    "the Worker died writing {key}"
                )));
            }
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

#[tokio::test]
async fn a_step_that_dies_between_marking_a_part_and_rewriting_it_reads_it_again() {
    let (h, _app) = populated().await;
    let db = h.db();
    many_links(db.as_ref()).await;
    let files = Arc::new(FailRewrite {
        files: Arc::new(h.files().clone()),
        when: "/tables/link.ndjson",
        puts: Mutex::new(0),
    });
    let mut services = h.services();
    services.db = Arc::new(Relinking {
        inner: db.clone(),
        done: Mutex::new(false),
    });
    services.files = files.clone();
    let d = deployment_with(&h, services);
    h.clock().set_ms(NIGHT);
    let key = format!("backups/{}.progress.json", stamp_of(NIGHT));
    let mut marked = None;
    for _ in 0..10 {
        let _ = d.tick(Timestamp(h.clock().now_ms())).await;
        h.clock().advance_minutes(1);
        if *files.puts.lock().unwrap() == 2 && marked.is_none() {
            // The rewrite died: the saved progress names the part to read.
            let raw = h.files().get(&key).await.unwrap().unwrap();
            let p: serde_json::Value = serde_json::from_slice(&raw).unwrap();
            marked = Some(p["cursor"].clone());
        }
    }
    let cursor = marked.expect("the rewrite was attempted and died");
    assert_eq!(cursor["dirty"], serde_json::json!([0]), "{cursor}");
    assert_eq!(cursor["written"], true);
    let state = query(db.as_ref(), "SELECT state FROM backup_run").await;
    assert_eq!(state.rows[0][0].as_text(), Some("done"));
    assert!(*files.puts.lock().unwrap() >= 3, "rewritten on the retry");

    let files: Arc<dyn FileStore> = Arc::new(h.files().clone());
    let source = FileStoreBackup::new(files, format!("backups/{}/", stamp_of(NIGHT)));
    let fresh = Harness::new();
    backup::load(&deployment(&fresh), &source, &LoadOptions::new())
        .await
        .unwrap();
    let live = query(
        fresh.db().as_ref(),
        "SELECT id FROM link WHERE student_id = 'st-0001' AND status <> 'revoked'",
    )
    .await;
    assert_eq!(live.rows.len(), 1);
    assert_eq!(live.rows[0][0].as_text(), Some("l-9999"));
}

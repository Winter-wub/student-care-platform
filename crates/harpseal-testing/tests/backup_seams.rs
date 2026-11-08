//! A nightly backup taken while the Deployment writes (#78 review): a
//! roster-style Tag replacement in the middle of a table's pages, an
//! announcement text edited as it is read, and a Role deleted between its
//! grants' table and its own. The backup restores cleanly: no duplicate
//! key (keyset paging), the deleted Role's grants dropped (they would have
//! cascaded). Live Verification challenges are never in it. A relink
//! during the backup: `backup_relink.rs`.

mod backup_support;

use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use backup_support::nightly::{NIGHT, stamp_of};
use backup_support::{OFFICER, deployment, deployment_with, populated, query};
use harpseal::backup::{self, FileStoreBackup, LoadOptions};
use harpseal::{Actor, Db, ExecResult, FileStore, RequestCtx, Result, Rows, Statement, Timestamp};
use harpseal_testing::Harness;

/// The real database, with writes slipped in as certain reads happen.
struct Meddling {
    inner: Arc<dyn Db>,
    done: Mutex<Vec<&'static str>>,
}

impl Meddling {
    /// `true` the first time `what` is asked for.
    fn first(&self, what: &'static str) -> bool {
        let mut done = self.done.lock().unwrap();
        if done.contains(&what) {
            return false;
        }
        done.push(what);
        true
    }
}

#[async_trait]
impl Db for Meddling {
    async fn query(&self, stmt: Statement) -> Result<Rows> {
        let sql = stmt.sql.to_string();
        let page = self.inner.query(stmt).await?;
        // A page of a table read by the dump: write to it before the next.
        if sql.contains("FROM \"student_tag\"") && sql.contains("LIMIT") && self.first("tags") {
            // As a roster import replacing a Student's Tags: same key, new rowid.
            self.inner
                .batch(vec![
                    Statement::new(
                        "DELETE FROM student_tag WHERE student_id = 's-many' AND tag_id = 't-0010'",
                    ),
                    Statement::new(
                        "INSERT INTO student_tag (student_id, tag_id, source, added_at) \
                         VALUES ('s-many', 't-0010', 'roster', 1)",
                    ),
                ])
                .await?;
        }
        if sql.contains("FROM \"announcement_text\"") && sql.contains("LIMIT") && self.first("text")
        {
            // As an edit: the text is deleted and inserted again.
            self.inner
                .execute(Statement::new(
                    "UPDATE announcement_text SET title = title || ' (แก้ไข)'",
                ))
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

/// The real FileStore; deletes the Role `Temp` once its grants are written.
struct DeletingRole {
    inner: Arc<dyn FileStore>,
    db: Arc<dyn Db>,
}

#[async_trait]
impl FileStore for DeletingRole {
    async fn put(&self, key: &str, bytes: Vec<u8>, content_type: &str) -> Result<()> {
        self.inner.put(key, bytes, content_type).await?;
        if key.ends_with("tables/role_permission.ndjson") {
            self.db
                .execute(Statement::new("DELETE FROM role WHERE name = 'Temp'"))
                .await?;
        }
        Ok(())
    }
    async fn get(&self, key: &str) -> Result<Option<Vec<u8>>> {
        self.inner.get(key).await
    }
    async fn delete(&self, key: &str) -> Result<()> {
        self.inner.delete(key).await
    }
}

#[tokio::test]
async fn a_backup_taken_during_writes_restores_cleanly() {
    let (h, _app) = populated().await;
    let db = h.db();
    // 600 Tags on one Student: more than one page of `student_tag`, in
    // key order and rowid order alike.
    let mut setup = vec![Statement::new(
        "INSERT INTO student (id, student_code, display_name, locale, created_at, updated_at) \
         VALUES ('s-many', '68019999', 'แมวน้ำ หลายป้าย', 'th', 1, 1)",
    )];
    for i in 0..600 {
        setup.push(Statement::new(format!(
            "INSERT INTO tag (id, name, created_at) VALUES ('t-{i:04}', 'tag {i:04}', 1)"
        )));
        setup.push(Statement::new(format!(
            "INSERT INTO student_tag (student_id, tag_id, source, added_at) \
             VALUES ('s-many', 't-{i:04}', 'roster', 1)"
        )));
    }
    db.batch(setup).await.unwrap();
    harpseal::ensure_role(
        &h.services(),
        &Actor::System,
        "Temp",
        &["kernel.student.view"],
        &RequestCtx::cli(),
    )
    .await
    .unwrap();
    // A live challenge (left out) and a spent one (kept).
    for (id, state, expires) in [
        ("vc-live", "pending", NIGHT + 600_000),
        ("vc-spent", "passed", NIGHT),
    ] {
        db.execute(
            Statement::new(
                "INSERT INTO verification_challenge (id, channel_account_id, student_id, \
                 method, secret_hash, expires_at, state, created_at) \
                 SELECT ?, a.id, s.id, 'email_otp', 'x', ?, ?, 1 \
                 FROM channel_account a, student s LIMIT 1",
            )
            .bind(id)
            .bind(expires)
            .bind(state),
        )
        .await
        .unwrap();
    }

    let mut services = h.services();
    let meddling: Arc<dyn Db> = Arc::new(Meddling {
        inner: db.clone(),
        done: Mutex::new(Vec::new()),
    });
    services.db = meddling;
    services.files = Arc::new(DeletingRole {
        inner: Arc::new(h.files().clone()),
        db: db.clone(),
    });
    let d = deployment_with(&h, services);
    d.migrate().await.unwrap();
    h.clock().set_ms(NIGHT);
    for _ in 0..10 {
        d.tick(Timestamp(h.clock().now_ms())).await.unwrap();
        h.clock().advance_minutes(1);
    }
    let state = query(db.as_ref(), "SELECT state, error FROM backup_run").await;
    assert_eq!(state.rows[0][0].as_text(), Some("done"), "{:?}", state.rows);

    let files: Arc<dyn FileStore> = Arc::new(h.files().clone());
    let source = FileStoreBackup::new(files, format!("backups/{}/", stamp_of(NIGHT)));
    let tags = String::from_utf8(
        harpseal::backup::BackupSource::read(&source, "tables/student_tag.ndjson")
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(tags.matches("\"t-0010\"").count(), 1, "read once");
    let challenges = String::from_utf8(
        harpseal::backup::BackupSource::read(&source, "tables/verification_challenge.ndjson")
            .await
            .unwrap(),
    )
    .unwrap();
    assert!(challenges.contains("vc-spent") && !challenges.contains("vc-live"));

    let fresh = Harness::new();
    fresh.clock().advance_minutes(3 * 24 * 60);
    let d2 = deployment(&fresh);
    let r = backup::load(&d2, &source, &LoadOptions::new())
        .await
        .unwrap();
    assert!(
        r.orphans_dropped
            .get("role_permission")
            .copied()
            .unwrap_or(0)
            >= 1,
        "{r:?}"
    );
    let fdb = fresh.db();
    let n = |sql: &'static str| {
        let fdb = fdb.clone();
        async move { query(fdb.as_ref(), sql).await.rows[0][0].as_i64().unwrap() }
    };
    assert_eq!(
        n("SELECT count(*) FROM student_tag WHERE student_id = 's-many'").await,
        600
    );
    assert_eq!(n("SELECT count(*) FROM role WHERE name = 'Temp'").await, 0);
    assert_eq!(
        n("SELECT count(*) FROM role_permission WHERE role_id NOT IN (SELECT id FROM role)").await,
        0
    );
    let audit = query(
        fdb.as_ref(),
        "SELECT after FROM audit_log WHERE action = 'backup.restore'",
    )
    .await;
    assert!(
        audit.rows[0][0]
            .as_text()
            .unwrap()
            .contains("orphans_dropped")
    );
    let _ = OFFICER;
}

#[tokio::test]
async fn a_backup_that_would_not_insert_is_refused_before_any_write() {
    let (h, app) = populated().await;
    // A Tag grant naming a Tag that does not exist (a key without ON
    // DELETE CASCADE), slipped in with foreign keys off.
    let db = h.db();
    db.execute(Statement::new("PRAGMA foreign_keys = OFF"))
        .await
        .unwrap();
    db.execute(Statement::new(
        "INSERT INTO student_tag (student_id, tag_id, source, added_at) \
         SELECT id, 'ghost', 'manual', 1 FROM student LIMIT 1",
    ))
    .await
    .unwrap();
    db.execute(Statement::new("PRAGMA foreign_keys = ON"))
        .await
        .unwrap();
    let mut b = backup::MemoryBackup::new();
    backup::dump(app.deployment(), &mut b).await.unwrap();

    let fresh = Harness::new();
    let d2 = deployment(&fresh);
    let err = backup::load(&d2, &b, &LoadOptions::new())
        .await
        .unwrap_err();
    let text = err.to_string();
    assert!(
        text.contains("nothing was restored") && text.contains("student_tag"),
        "{text}"
    );
    // Nothing was written: no Students, no FileStore objects.
    let students = query(fresh.db().as_ref(), "SELECT count(*) FROM student").await;
    assert_eq!(students.rows[0][0].as_i64(), Some(0));
    assert!(fresh.files().keys().is_empty());
}

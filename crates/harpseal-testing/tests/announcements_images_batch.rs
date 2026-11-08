//! A save whose batch commits but reports an error (a lost answer from
//! D1, say) keeps the objects its rows list and deletes only those no row
//! lists (#109 review).

mod announcements_support;
mod images_support;

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use announcements_support::{CONFIG, PASSWORD};
use async_trait::async_trait;
use harpseal::modules::announcements::Announcements;
use harpseal::{Db, DeploymentConfig, Error, ExecResult, Result, Rows, Statement};
use harpseal_testing::Harness;
use images_support::{PUBLIC, Part, f, jpeg, png, post_parts, t};

/// Commits every batch; armed, it then answers an error once.
struct Lying {
    inner: Arc<dyn Db>,
    armed: AtomicBool,
}

#[async_trait]
impl Db for Lying {
    async fn query(&self, stmt: Statement) -> Result<Rows> {
        self.inner.query(stmt).await
    }
    async fn execute(&self, stmt: Statement) -> Result<ExecResult> {
        self.inner.execute(stmt).await
    }
    async fn batch(&self, stmts: Vec<Statement>) -> Result<Vec<ExecResult>> {
        let done = self.inner.batch(stmts).await?;
        if self.armed.swap(false, Ordering::SeqCst) {
            return Err(Error::Db("the answer was lost".into()));
        }
        Ok(done)
    }
}

async fn keys_listed(h: &Harness) -> Vec<String> {
    let rows = h
        .db()
        .query(Statement::new(
            "SELECT file_key FROM announcement_file ORDER BY file_key",
        ))
        .await
        .unwrap();
    rows.rows
        .iter()
        .map(|r| r[0].as_text().unwrap().to_string())
        .collect()
}

#[tokio::test]
async fn a_batch_that_commits_but_errs_keeps_listed_objects() {
    let h = Harness::new();
    let db = Arc::new(Lying {
        inner: h.db(),
        armed: AtomicBool::new(false),
    });
    let mut services = h.services();
    services.db = db.clone();
    let src = format!("public_url = \"{PUBLIC}\"\n{CONFIG}");
    let d = h
        .builder()
        .config(DeploymentConfig::from_toml(&src).unwrap())
        .runtime(services)
        .module(Announcements)
        .build()
        .unwrap();
    let app = h.boot(d).await;
    h.create_staff("officer@sealbay.example", PASSWORD, &["Officer"])
        .await;
    let b = app.browser();
    assert_eq!(
        b.login("officer@sealbay.example", PASSWORD).await.status,
        303
    );
    b.get("/admin/announcements/new").await;

    // Create: committed, reported as failed. The object stays: its row
    // lists it.
    db.armed.store(true, Ordering::SeqCst);
    let parts: Vec<Part> = vec![
        t("title_th", "ย้ายห้องสอบ"),
        t("body_th", "ห้อง 3105"),
        f("image1", "a.png", png(20, 13)),
    ];
    let r = post_parts(&b, "/admin/announcements/new", &parts).await;
    assert_eq!(r.status, 500);
    let listed = keys_listed(&h).await;
    assert_eq!(listed.len(), 1);
    assert_eq!(
        h.files().keys(),
        listed,
        "a listed object is never discarded"
    );

    // A draft edit replacing it: committed, reported as failed. The new
    // object stays (listed), the old one goes (no longer listed).
    let id = h
        .db()
        .query(Statement::new("SELECT id FROM announcement"))
        .await
        .unwrap()
        .rows[0][0]
        .as_text()
        .unwrap()
        .to_string();
    let path = format!("/admin/announcements/{id}/edit");
    b.get(&path).await;
    db.armed.store(true, Ordering::SeqCst);
    let parts = vec![
        t("title_th", "ย้ายห้องสอบ"),
        t("body_th", "ห้อง 3105"),
        t("version", "0"),
        f("image1", "b.jpg", jpeg(20, 13)),
    ];
    let r = post_parts(&b, &path, &parts).await;
    assert_eq!(r.status, 500);
    let now_listed = keys_listed(&h).await;
    assert_eq!(now_listed.len(), 1);
    assert_ne!(now_listed, listed);
    assert_eq!(h.files().keys(), now_listed);

    // A batch that really fails (a lost race): the new object goes.
    let r = post_parts(
        &b,
        &path,
        &[
            t("title_th", "ย้ายห้องสอบ"),
            t("body_th", "ห้อง 3105"),
            t("version", "0"),
            f("image1", "c.png", png(20, 13)),
        ],
    )
    .await;
    assert_eq!(r.status, 303, "a stale version: back to the page");
    assert_eq!(h.files().keys(), now_listed);
}

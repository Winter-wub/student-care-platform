//! A restore draws a new Menu revision salt (#66): a copy of production
//! restored into staging on the same LINE token must not name (and then
//! retire) rich menus like production.

mod backup_support;

use backup_support::{deployment, populated, query};
use harpseal::Statement;
use harpseal::backup::{self, LoadOptions, MemoryBackup};
use harpseal_testing::Harness;

#[tokio::test]
async fn a_restore_replaces_the_menu_revision_salt() {
    let (h, app) = populated().await;
    let sql = "INSERT INTO menu_salt (salt) VALUES ('0a1b2c3d')";
    h.db().execute(Statement::new(sql)).await.unwrap();
    let mut b = MemoryBackup::new();
    backup::dump(app.deployment(), &mut b).await.unwrap();
    assert!(
        String::from_utf8_lossy(&b.entries["tables/menu_salt.ndjson"]).contains("0a1b2c3d"),
        "the salt is backed up"
    );

    let fresh = Harness::new();
    let d = deployment(&fresh);
    backup::load(&d, &b, &LoadOptions::new()).await.unwrap();
    let rows = query(fresh.db().as_ref(), "SELECT salt FROM menu_salt").await;
    assert_eq!(rows.len(), 1, "still one salt");
    let salt = rows.get(0, "salt").and_then(|v| v.as_text()).unwrap();
    assert_ne!(salt, "0a1b2c3d", "a new salt");
    assert!(salt.len() == 8 && salt.bytes().all(|c| c.is_ascii_hexdigit()));
    let audit = query(
        fresh.db().as_ref(),
        "SELECT json_extract(after, '$.menu_salt_redrawn') AS r FROM audit_log \
         WHERE action = 'backup.restore'",
    )
    .await;
    assert_eq!(audit.get(0, "r").and_then(|v| v.as_i64()), Some(1));
}

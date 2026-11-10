//! `[roles]` is seeded once (#110): an Administrator's edits on
//! `/admin/roles` survive `migrate`, a pair new to the TOML still arrives
//! (once), a database from before the rule is upgraded without a grant or a
//! removal of its own, a deleted TOML Role stays deleted (and a Role Admin
//! creates under its name inherits nothing), a restored backup keeps what
//! was seeded, and a forced restore of a backup from before `role_seed`
//! takes the upgrade path. Racing writers: `roles_seed_races.rs`.

mod admin_support;
mod support;

use admin_support::{CONFIG, audits, boot, flash, hidden, signed_in, text};
use harpseal::backup::{self, LoadOptions, MemoryBackup};
use harpseal::{Deployment, DeploymentConfig, Statement};
use harpseal_testing::{Browser, Harness, urlencode};

/// The admin Deployment with `[roles]` changed by `edit` on the TOML text.
fn with_roles(h: &Harness, edit: impl Fn(&str) -> String) -> Deployment {
    let config = DeploymentConfig::from_toml(&edit(CONFIG)).unwrap();
    h.builder().config(config).build().unwrap()
}

/// `CONFIG` with Reader also listing `kernel.usage.view`.
fn reader_with_usage(c: &str) -> String {
    c.replace(
        r#"Reader = ["kernel.student.view"]"#,
        r#"Reader = ["kernel.student.view", "kernel.usage.view"]"#,
    )
}

async fn perms(h: &Harness, role: &str) -> Vec<String> {
    harpseal::role_permissions(&h.services(), role)
        .await
        .unwrap()
}

async fn role_id(h: &Harness, name: &str) -> Option<String> {
    text(h, "SELECT id FROM role WHERE name = ?", name, "id").await
}

/// Sets the Permissions of Role `name` on its edit page.
async fn set_perms(h: &Harness, admin: &Browser<'_>, name: &str, perms: &[&str]) {
    let id = role_id(h, name).await.unwrap();
    let version = hidden(
        &admin.get(&format!("/admin/roles/{id}")).await.body,
        "version",
    );
    let mut body = format!("version={}", urlencode(&version));
    for p in perms {
        body.push_str(&format!("&permission={p}"));
    }
    admin.post(&format!("/admin/roles/{id}"), &body).await;
    assert_eq!(flash(admin), "ok:kernel.admin.saved");
}

/// Every (Role name, Permission) grant, sorted.
async fn grants(h: &Harness) -> Vec<String> {
    let rows = h
        .db()
        .query(Statement::new(
            "SELECT r.name || ' ' || rp.permission_id AS g FROM role_permission rp \
             JOIN role r ON r.id = rp.role_id ORDER BY g",
        ))
        .await
        .unwrap();
    rows.rows
        .iter()
        .filter_map(|r| r[0].as_text().map(str::to_string))
        .collect()
}

/// Audit rows by the system for Roles (the seed's own).
async fn seed_audits(h: &Harness) -> i64 {
    let rows = h
        .db()
        .query(Statement::new(
            "SELECT count(*) AS n FROM audit_log \
             WHERE action LIKE 'role.%' AND actor_kind = 'system'",
        ))
        .await
        .unwrap();
    rows.get(0, "n").and_then(|v| v.as_i64()).unwrap()
}

#[tokio::test]
async fn a_toml_permission_an_administrator_removed_stays_removed() {
    let (h, app) = boot().await;
    let (admin, _) = signed_in(&h, &app, "Administrator").await;
    // Officer lists kernel.link.manage alone, so it was seeded with
    // kernel.link.revoke too (the legacy rule); both go.
    assert_eq!(
        perms(&h, "Officer").await,
        [
            "kernel.link.manage",
            "kernel.link.revoke",
            "kernel.student.edit",
            "kernel.student.view"
        ]
    );
    set_perms(&h, &admin, "Officer", &["kernel.student.view"]).await;
    let audited = seed_audits(&h).await;

    app.deployment().migrate().await.unwrap();
    app.deployment().migrate().await.unwrap();
    assert_eq!(perms(&h, "Officer").await, ["kernel.student.view"]);
    assert_eq!(
        seed_audits(&h).await,
        audited,
        "nothing granted, nothing audited"
    );
}

#[tokio::test]
async fn a_pair_new_to_the_toml_is_granted_once() {
    let (h, app) = boot().await;
    let (admin, _) = signed_in(&h, &app, "Administrator").await;
    let audited = seed_audits(&h).await;
    assert_eq!(
        audited, 0,
        "the first seed of an empty database is the install"
    );
    let d = with_roles(&h, reader_with_usage);
    d.migrate().await.unwrap();
    assert_eq!(
        perms(&h, "Reader").await,
        ["kernel.student.view", "kernel.usage.view"]
    );
    assert_eq!(
        seed_audits(&h).await,
        audited + 1,
        "one role.update by system"
    );
    let after = text(
        &h,
        "SELECT after FROM audit_log WHERE action = 'role.update' AND actor_kind = ?",
        "system",
        "after",
    )
    .await
    .unwrap();
    assert!(after.contains("kernel.usage.view"), "{after}");

    // Removed by Admin, it is not granted a second time.
    set_perms(&h, &admin, "Reader", &["kernel.student.view"]).await;
    d.migrate().await.unwrap();
    assert_eq!(perms(&h, "Reader").await, ["kernel.student.view"]);
    assert_eq!(seed_audits(&h).await, audited + 1);
}

#[tokio::test]
async fn an_existing_database_is_upgraded_without_a_grant_or_a_removal() {
    let (h, app) = boot().await;
    let (admin, _) = signed_in(&h, &app, "Administrator").await;
    // Admin added a Permission the TOML does not list: kept, as ever.
    set_perms(
        &h,
        &admin,
        "Reader",
        &["kernel.student.view", "kernel.audit.view"],
    )
    .await;
    // The database as it was before migration 0017.
    for sql in [
        "DROP TABLE role_seed",
        "DELETE FROM migration WHERE module = 'kernel' AND name = '0017_role_seed'",
    ] {
        h.db().execute(Statement::new(sql)).await.unwrap();
    }
    let before = grants(&h).await;
    let audited = seed_audits(&h).await;

    app.deployment().migrate().await.unwrap();
    assert_eq!(grants(&h).await, before);
    assert_eq!(seed_audits(&h).await, audited, "no grant to audit");
    let rows = h
        .db()
        .query(Statement::new(
            "SELECT permission_id FROM role_seed WHERE role_name = 'Reader' ORDER BY 1",
        ))
        .await
        .unwrap();
    let got: Vec<&str> = rows.rows.iter().filter_map(|r| r[0].as_text()).collect();
    assert_eq!(got, ["", "kernel.student.view"], "the TOML's pairs only");

    // From now on the rule holds: a removal sticks.
    set_perms(&h, &admin, "Reader", &["kernel.audit.view"]).await;
    app.deployment().migrate().await.unwrap();
    assert_eq!(perms(&h, "Reader").await, ["kernel.audit.view"]);
}

#[tokio::test]
async fn a_deleted_toml_role_stays_deleted_and_a_new_name_is_created() {
    let (h, app) = boot().await;
    let (admin, _) = signed_in(&h, &app, "Administrator").await;
    let id = role_id(&h, "Reader").await.unwrap();
    admin.post(&format!("/admin/roles/{id}/delete"), "").await;
    assert_eq!(flash(&admin), "ok:kernel.admin.roles.deleted");

    // Even with a Permission new to it in the TOML.
    let d = with_roles(&h, with_auditor);
    let created = audits(&h, "role.create").await;
    d.migrate().await.unwrap();
    d.migrate().await.unwrap();
    assert_eq!(role_id(&h, "Reader").await, None);
    assert_eq!(perms(&h, "Auditor").await, ["kernel.audit.view"]);
    assert_eq!(audits(&h, "role.create").await, created + 1);

    // A Role Admin creates under the deleted name gets none of the TOML's
    // Permissions: they were recorded while the name had no Role.
    admin
        .post("/admin/roles", "name=Reader&permission=kernel.audit.view")
        .await;
    assert_eq!(flash(&admin), "ok:kernel.admin.roles.created");
    d.migrate().await.unwrap();
    assert_eq!(perms(&h, "Reader").await, ["kernel.audit.view"]);
}

/// `CONFIG` with Reader also listing `kernel.usage.view` and a new Role.
fn with_auditor(c: &str) -> String {
    reader_with_usage(c).replace(
        "[texts.th]",
        "Auditor = [\"kernel.audit.view\"]\n\n[texts.th]",
    )
}

#[tokio::test]
async fn a_forced_restore_of_a_backup_from_before_role_seed_takes_the_upgrade_path() {
    // A Deployment still at Kernel migration 0016, on the first TOML
    // (so without 0018's indexes either).
    let old = Harness::new();
    let d_old = with_roles(&old, str::to_string);
    d_old.migrate().await.unwrap();
    for sql in [
        "DROP TABLE role_seed",
        "DROP INDEX inbound_event_sender",
        "DROP INDEX channel_account_first_seen",
        "DELETE FROM migration WHERE module = 'kernel' AND name IN \
         ('0017_role_seed', '0018_link_picker_idx')",
    ] {
        old.db().execute(Statement::new(sql)).await.unwrap();
    }
    let mut b = MemoryBackup::new();
    backup::dump(&d_old, &mut b).await.unwrap();
    assert!(!b.entries.contains_key("tables/role_seed.ndjson"));

    // The target is seeded with a TOML that grew since: Reader got a
    // Permission, Auditor is new.
    let h = Harness::new();
    h.clock().advance_minutes(24 * 60);
    let d = with_roles(&h, with_auditor);
    d.migrate().await.unwrap();
    backup::load(&d, &b, &LoadOptions::new().force(true))
        .await
        .unwrap();
    let left = h
        .db()
        .query(Statement::new("SELECT 1 FROM role_seed"))
        .await
        .unwrap();
    assert!(
        left.is_empty(),
        "the target's seed record is gone with its Roles"
    );

    h.clock().advance_minutes(24 * 60);
    d.migrate().await.unwrap();
    assert_eq!(
        perms(&h, "Reader").await,
        ["kernel.student.view", "kernel.usage.view"]
    );
    assert_eq!(perms(&h, "Auditor").await, ["kernel.audit.view"]);
}

#[tokio::test]
async fn a_restored_backup_does_not_grant_a_removed_pair_again() {
    let (h, app) = boot().await;
    let (admin, _) = signed_in(&h, &app, "Administrator").await;
    set_perms(&h, &admin, "Officer", &["kernel.student.view"]).await;
    let mut b = MemoryBackup::new();
    backup::dump(app.deployment(), &mut b).await.unwrap();
    assert!(b.entries.contains_key("tables/role_seed.ndjson"));

    let fresh = Harness::new();
    let d = with_roles(&fresh, str::to_string);
    backup::load(&d, &b, &LoadOptions::new()).await.unwrap();
    // The fresh harness replays the same ids from the same time (see
    // backup.rs); move on before the next write.
    fresh.clock().advance_minutes(24 * 60);
    d.migrate().await.unwrap();
    assert_eq!(perms(&fresh, "Officer").await, ["kernel.student.view"]);
}

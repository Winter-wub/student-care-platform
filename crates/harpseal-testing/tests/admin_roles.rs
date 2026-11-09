//! Roles pages (#27) over HTTP: Permissions grouped by Module, create
//! (unique name, declared Permissions only), edit, delete only when unused.

mod admin_support;
mod support;

use admin_support::{audits, boot, flash, hidden, signed_in, staff_version, text};
use harpseal_testing::urlencode;

#[tokio::test]
async fn roles_are_created_edited_and_deleted_only_when_unused() {
    let (h, app) = boot().await;
    let (admin, _) = signed_in(&h, &app, "Administrator").await;
    let list = admin.get("/admin/roles").await;
    assert_eq!(list.status, 200);
    assert!(
        list.body.contains("<legend>ระบบหลัก</legend>"),
        "grouped by Module, in Thai"
    );
    assert!(
        list.body.contains("ดูเหตุการณ์ความปลอดภัย"),
        "Permissions read as Thai sentences"
    );
    assert!(list.body.contains("มาจากไฟล์ตั้งค่า"), "TOML Roles marked");
    assert!(
        list.body.contains("<a href=\"/admin/roles/") && list.body.contains(">ผู้ดูแลระบบ</a>"),
        "the Administrator Role reads in Thai"
    );

    let r = admin
        .post(
            "/admin/roles",
            "name=+Auditor+&permission=kernel.audit.view&permission=kernel.security.view",
        )
        .await;
    assert_eq!(r.status, 303, "{}", r.body);
    assert_eq!(flash(&admin), "ok:kernel.admin.roles.created");
    let id = text(&h, "SELECT id FROM role WHERE name = ?", "Auditor", "id")
        .await
        .expect("trimmed name");
    let perms = harpseal::role_permissions(app.deployment().services(), "Auditor")
        .await
        .unwrap();
    assert_eq!(perms, ["kernel.audit.view", "kernel.security.view"]);
    assert_eq!(audits(&h, "role.create").await, 1);

    // Same name, unknown Permission, empty name: re-rendered, nothing written.
    let dup = admin.post("/admin/roles", "name=Auditor").await;
    assert_eq!(dup.status, 422);
    assert!(dup.body.contains("มีบทบาทชื่อนี้อยู่แล้ว"));
    let unknown = admin
        .post("/admin/roles", "name=X&permission=kernel.nope")
        .await;
    assert_eq!(unknown.status, 422);
    assert_eq!(admin.post("/admin/roles", "name=+").await.status, 422);
    assert_eq!(
        app.count("SELECT count(*) FROM role WHERE name = 'X'")
            .await,
        0
    );

    // Edit with the version token; a stale token is refused.
    let edit = admin.get(&format!("/admin/roles/{id}")).await;
    let version = hidden(&edit.body, "version");
    let body = format!(
        "permission=kernel.audit.view&version={}",
        urlencode(&version)
    );
    let r = admin.post(&format!("/admin/roles/{id}"), &body).await;
    assert_eq!(r.status, 303);
    assert_eq!(flash(&admin), "ok:kernel.admin.saved");
    let perms = harpseal::role_permissions(app.deployment().services(), "Auditor")
        .await
        .unwrap();
    assert_eq!(perms, ["kernel.audit.view"]);
    admin.post(&format!("/admin/roles/{id}"), &body).await;
    assert_eq!(flash(&admin), "err:kernel.admin.conflict");
    let (before, after) = (
        text(
            &h,
            "SELECT before FROM audit_log WHERE action = ?",
            "role.update",
            "before",
        )
        .await,
        text(
            &h,
            "SELECT after FROM audit_log WHERE action = ?",
            "role.update",
            "after",
        )
        .await,
    );
    assert!(before.unwrap().contains("kernel.security.view"));
    assert!(!after.unwrap().contains("kernel.security.view"));

    // Held by someone: no delete. Free again: deleted.
    let (_, viewer_id) = signed_in(&h, &app, "Viewer").await;
    let v = staff_version(&admin, &viewer_id).await;
    admin
        .post(
            &format!("/admin/staff/{viewer_id}/roles"),
            &format!("role=Auditor&version={v}"),
        )
        .await;
    assert_eq!(flash(&admin), "ok:kernel.admin.saved");
    let held = admin.get(&format!("/admin/roles/{id}")).await;
    assert!(!held.body.contains("/delete\""), "no delete button");
    admin.post(&format!("/admin/roles/{id}/delete"), "").await;
    assert_eq!(flash(&admin), "err:kernel.admin.roles.in_use");
    assert_eq!(
        app.count("SELECT count(*) FROM role WHERE name = 'Auditor'")
            .await,
        1
    );

    let v = staff_version(&admin, &viewer_id).await;
    admin
        .post(
            &format!("/admin/staff/{viewer_id}/roles"),
            &format!("role=Viewer&version={v}"),
        )
        .await;
    admin.post(&format!("/admin/roles/{id}/delete"), "").await;
    assert_eq!(flash(&admin), "ok:kernel.admin.roles.deleted");
    assert_eq!(
        app.count("SELECT count(*) FROM role WHERE name = 'Auditor'")
            .await,
        0
    );
    assert_eq!(
        app.count(
            "SELECT count(*) FROM role_permission WHERE role_id NOT IN (SELECT id FROM role)"
        )
        .await,
        0
    );
    assert_eq!(audits(&h, "role.delete").await, 1);
    assert_eq!(admin.get(&format!("/admin/roles/{id}")).await.status, 404);
}

#[tokio::test]
async fn a_toml_role_is_editable_and_its_edit_survives_migrate() {
    let (h, app) = boot().await;
    let (admin, _) = signed_in(&h, &app, "Administrator").await;
    let id = text(&h, "SELECT id FROM role WHERE name = ?", "Reader", "id")
        .await
        .unwrap();
    let edit = admin.get(&format!("/admin/roles/{id}")).await;
    assert!(edit.body.contains("migrate"), "the TOML note");
    let version = hidden(&edit.body, "version");
    let body = format!(
        "permission=kernel.student.view&permission=kernel.usage.view&version={}",
        urlencode(&version)
    );
    admin.post(&format!("/admin/roles/{id}"), &body).await;
    assert_eq!(flash(&admin), "ok:kernel.admin.saved");
    // A migrate keeps the addition.
    app.deployment().migrate().await.unwrap();
    let perms = harpseal::role_permissions(app.deployment().services(), "Reader")
        .await
        .unwrap();
    assert_eq!(perms, ["kernel.student.view", "kernel.usage.view"]);
}

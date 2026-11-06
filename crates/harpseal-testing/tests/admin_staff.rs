//! Staff pages (#27) over HTTP: create with a temporary password shown
//! once and a forced change, disable (sessions end), self-disable and
//! last-manager refusals, Roles, password reset, clear lockout, CSRF.

mod admin_support;
mod support;

use admin_support::{
    PASSWORD, audits, boot, email_of, flash, signed_in, staff_version, temp_password, text,
};
use harpseal_testing::urlencode;

const NEW: &str = "pim@sealbay.example";

#[tokio::test]
async fn a_new_account_sees_its_password_once_and_must_change_it_first() {
    let (h, app) = boot().await;
    let (admin, _) = signed_in(&h, &app, "Administrator").await;
    assert_eq!(admin.get("/admin/staff/new").await.status, 200);
    let form = format!(
        "email={}&display_name=Pim+Officer&locale=en&role=Officer",
        urlencode("Pim@SealBay.example")
    );
    let created = admin.post("/admin/staff/new", &form).await;
    assert_eq!(created.status, 200, "{}", created.body);
    let password = temp_password(&created.body);
    assert!(password.chars().count() >= 12, "{password}");
    let id = text(&h, "SELECT id FROM staff WHERE email = ?", NEW, "id")
        .await
        .expect("stored lowercased");
    // Shown once: the account page and the audit row never hold it.
    let page = admin.get(&format!("/admin/staff/{id}")).await;
    assert!(!page.body.contains(&password));
    assert!(page.body.contains("Officer"));
    let after = text(
        &h,
        "SELECT after FROM audit_log WHERE action = ?",
        "staff.create",
        "after",
    )
    .await;
    let rows = h
        .db()
        .query(harpseal::Statement::new(
            "SELECT before, after FROM audit_log",
        ))
        .await
        .unwrap();
    for r in &rows.rows {
        assert!(
            !format!("{r:?}").contains(&password),
            "password in the audit log"
        );
    }
    assert!(after.is_some());

    let b = app.browser();
    let r = b.login(NEW, &password).await;
    assert_eq!(r.status, 303, "{}", r.body);
    let students = b.get("/admin/students").await;
    assert_eq!(students.status, 303);
    assert_eq!(students.header("location"), Some("/admin/password"));
    let change = format!(
        "current={}&new=a+brand+new+secret&confirm=a+brand+new+secret",
        urlencode(&password)
    );
    assert_eq!(b.get("/admin/password").await.status, 200);
    assert_eq!(b.post("/admin/password", &change).await.status, 303);
    assert_eq!(b.get("/admin/students").await.status, 200);

    // The same email again is refused on the form, nothing written.
    let again = admin.post("/admin/staff/new", &form).await;
    assert_eq!(again.status, 422);
    assert!(again.body.contains("อีเมลนี้มีบัญชีอยู่แล้ว"));
    assert_eq!(audits(&h, "staff.create").await, 2, "Administrator + Pim");
    let bad = admin
        .post("/admin/staff/new", "email=nope&display_name=X")
        .await;
    assert_eq!(bad.status, 422);
}

#[tokio::test]
async fn disabling_ends_every_session_and_enabling_lets_them_back() {
    let (h, app) = boot().await;
    let (admin, _) = signed_in(&h, &app, "Administrator").await;
    let (officer, officer_id) = signed_in(&h, &app, "Officer").await;
    let v = staff_version(&admin, &officer_id).await;
    let r = admin
        .post(
            &format!("/admin/staff/{officer_id}/status"),
            &format!("active=0&version={v}"),
        )
        .await;
    assert_eq!(r.status, 303);
    assert_eq!(flash(&admin), "ok:kernel.admin.saved");
    assert_eq!(app.count("SELECT count(*) FROM staff_session s JOIN staff st ON st.id = s.staff_id WHERE st.status = 'disabled'").await, 0);
    let next = officer.get("/admin/students").await;
    assert_eq!(next.status, 303);
    assert_eq!(next.header("location"), Some("/admin/login"));
    assert_eq!(
        app.browser()
            .login(&email_of("Officer"), PASSWORD)
            .await
            .status,
        401
    );
    assert_eq!(audits(&h, "staff.disable").await, 1);

    // The old version is stale now.
    let stale = admin
        .post(
            &format!("/admin/staff/{officer_id}/status"),
            &format!("active=1&version={v}"),
        )
        .await;
    assert_eq!(stale.status, 303);
    assert_eq!(flash(&admin), "err:kernel.admin.conflict");
    let v = staff_version(&admin, &officer_id).await;
    admin
        .post(
            &format!("/admin/staff/{officer_id}/status"),
            &format!("active=1&version={v}"),
        )
        .await;
    assert_eq!(
        app.browser()
            .login(&email_of("Officer"), PASSWORD)
            .await
            .status,
        303
    );
}

#[tokio::test]
async fn an_administrator_cannot_disable_themself_nor_drop_the_last_manager() {
    let (h, app) = boot().await;
    let (admin, admin_id) = signed_in(&h, &app, "Administrator").await;
    let v = staff_version(&admin, &admin_id).await;
    let own = format!("/admin/staff/{admin_id}");
    admin
        .post(&format!("{own}/status"), &format!("active=0&version={v}"))
        .await;
    assert_eq!(flash(&admin), "err:kernel.admin.staff.own_refused");
    admin
        .post(&format!("{own}/roles"), &format!("role=Viewer&version={v}"))
        .await;
    assert_eq!(flash(&admin), "err:kernel.admin.staff.last_manager");
    let roles = text(
        &h,
        "SELECT r.name FROM staff_role sr JOIN role r ON r.id = sr.role_id WHERE sr.staff_id = ?",
        &admin_id,
        "name",
    )
    .await;
    assert_eq!(roles.as_deref(), Some("Administrator"));
    assert_eq!(audits(&h, "staff.roles").await, 0);
    assert_eq!(
        admin.get("/admin/staff").await.status,
        200,
        "still a manager"
    );

    // Taking the Permission off the Role is refused the same way.
    let role_id = text(
        &h,
        "SELECT id FROM role WHERE name = ?",
        "Administrator",
        "id",
    )
    .await
    .unwrap();
    let edit = admin.get(&format!("/admin/roles/{role_id}")).await.body;
    let version = admin_support::hidden(&edit, "version");
    admin
        .post(
            &format!("/admin/roles/{role_id}"),
            &format!(
                "permission=kernel.audit.view&version={}",
                urlencode(&version)
            ),
        )
        .await;
    assert_eq!(flash(&admin), "err:kernel.admin.staff.last_manager");

    // With a second manager, the first may step down.
    let other = h
        .create_staff("chief@sealbay.example", PASSWORD, &["Administrator"])
        .await;
    assert!(!other.is_empty());
    admin
        .post(&format!("{own}/roles"), &format!("role=Viewer&version={v}"))
        .await;
    assert_eq!(flash(&admin), "ok:kernel.admin.saved");
    assert_eq!(admin.get("/admin/staff").await.status, 403);
    let (before, after) = (
        text(
            &h,
            "SELECT before FROM audit_log WHERE action = ?",
            "staff.roles",
            "before",
        )
        .await,
        text(
            &h,
            "SELECT after FROM audit_log WHERE action = ?",
            "staff.roles",
            "after",
        )
        .await,
    );
    assert_eq!(before.as_deref(), Some(r#"{"roles":["Administrator"]}"#));
    assert_eq!(after.as_deref(), Some(r#"{"roles":["Viewer"]}"#));
}

#[tokio::test]
async fn a_temporary_password_ends_sessions_and_a_lockout_can_be_cleared() {
    let (h, app) = boot().await;
    let (admin, _) = signed_in(&h, &app, "Administrator").await;
    let (officer, officer_id) = signed_in(&h, &app, "Officer").await;
    // Five wrong passwords lock the account.
    let guesser = app.browser();
    for _ in 0..5 {
        guesser
            .login(&email_of("Officer"), "wrong wrong wrong")
            .await;
    }
    let page = admin.get(&format!("/admin/staff/{officer_id}")).await.body;
    assert!(page.contains("ล็อกถึง"), "locked badge");
    let v = admin_support::hidden(&page, "version");
    admin
        .post(
            &format!("/admin/staff/{officer_id}/unlock"),
            &format!("version={v}"),
        )
        .await;
    assert_eq!(flash(&admin), "ok:kernel.admin.staff.unlocked");
    assert_eq!(
        app.count("SELECT count(*) FROM staff WHERE locked_until IS NOT NULL")
            .await,
        0
    );
    assert_eq!(audits(&h, "staff.unlock").await, 1);

    let v = staff_version(&admin, &officer_id).await;
    let reset = admin
        .post(
            &format!("/admin/staff/{officer_id}/password"),
            &format!("version={v}"),
        )
        .await;
    assert_eq!(reset.status, 200, "{}", reset.body);
    let password = temp_password(&reset.body);
    assert_eq!(
        officer.get("/admin/students").await.header("location"),
        Some("/admin/login")
    );
    let b = app.browser();
    assert_eq!(b.login(&email_of("Officer"), PASSWORD).await.status, 401);
    assert_eq!(b.login(&email_of("Officer"), &password).await.status, 303);
    assert_eq!(
        b.get("/admin/students").await.header("location"),
        Some("/admin/password")
    );
    let after = text(
        &h,
        "SELECT after FROM audit_log WHERE action = ?",
        "staff.password_reset",
        "after",
    )
    .await
    .unwrap();
    assert!(
        !after.contains(&password) && !after.contains("argon2"),
        "{after}"
    );
}

#[tokio::test]
async fn posts_without_the_csrf_token_are_refused_and_write_nothing() {
    let (h, app) = boot().await;
    let (admin, _) = signed_in(&h, &app, "Administrator").await;
    let (_, officer_id) = signed_in(&h, &app, "Officer").await;
    let v = staff_version(&admin, &officer_id).await;
    let r = admin
        .post_raw(
            &format!("/admin/staff/{officer_id}/status"),
            &format!("active=0&version={v}"),
        )
        .await;
    assert_eq!(r.status, 403);
    let r = admin
        .post_raw("/admin/staff/new", "email=a%40b.example&display_name=A")
        .await;
    assert_eq!(r.status, 403);
    let r = admin.post_raw("/admin/texts/consent", "version=9").await;
    assert_eq!(r.status, 403);
    assert_eq!(
        app.count("SELECT count(*) FROM staff WHERE status = 'disabled'")
            .await,
        0
    );
    assert_eq!(app.count("SELECT count(*) FROM setting").await, 0);
    assert_eq!(audits(&h, "staff.create").await, 2);
}

#[tokio::test]
async fn the_status_form_needs_its_field_and_a_matching_status_and_own_reset_is_refused() {
    let (h, app) = boot().await;
    let (admin, admin_id) = signed_in(&h, &app, "Administrator").await;
    let (_, officer_id) = signed_in(&h, &app, "Officer").await;
    let path = format!("/admin/staff/{officer_id}/status");
    let v = staff_version(&admin, &officer_id).await;
    // No `active` field: refused, never read as "disable".
    let r = admin.post(&path, &format!("version={v}")).await;
    assert_eq!(r.status, 422);
    assert_eq!(
        admin
            .post(&path, &format!("active=yes&version={v}"))
            .await
            .status,
        422
    );
    // Enabling an active account changes nothing and logs nothing.
    admin.post(&path, &format!("active=1&version={v}")).await;
    assert_eq!(flash(&admin), "err:kernel.admin.conflict");
    assert_eq!(audits(&h, "staff.enable").await, 0);
    assert_eq!(audits(&h, "staff.disable").await, 0);
    assert_eq!(
        app.count("SELECT count(*) FROM staff WHERE status = 'disabled'")
            .await,
        0
    );

    // One's own temporary password is refused (no form, and a forged POST).
    let own = admin.get(&format!("/admin/staff/{admin_id}")).await.body;
    assert!(
        !own.contains("/password\""),
        "no reset form on one's own page"
    );
    let v = staff_version(&admin, &admin_id).await;
    admin
        .post(
            &format!("/admin/staff/{admin_id}/password"),
            &format!("version={v}"),
        )
        .await;
    assert_eq!(flash(&admin), "err:kernel.admin.staff.own_refused");
    assert_eq!(audits(&h, "staff.password_reset").await, 0);
}

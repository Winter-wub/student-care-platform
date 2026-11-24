//! An Administrator edits a Staff member's display name on
//! `/admin/staff/{id}`: trimmed and validated, compare-and-set on the
//! account's `version`, audited as `staff.rename` with the name before and
//! after, `kernel.staff.manage` only, and never the email.

mod admin_support;
mod support;

use admin_support::{audits, boot, email_of, flash, signed_in, staff_version, text};
use harpseal_testing::urlencode;

async fn name_of(h: &harpseal_testing::Harness, id: &str) -> String {
    text(
        h,
        "SELECT display_name FROM staff WHERE id = ?",
        id,
        "display_name",
    )
    .await
    .unwrap()
}

#[tokio::test]
async fn an_administrator_renames_a_staff_member_and_the_audit_has_both_names() {
    let (h, app) = boot().await;
    let (admin, _) = signed_in(&h, &app, "Administrator").await;
    let (_, officer_id) = signed_in(&h, &app, "Officer").await;
    let before = name_of(&h, &officer_id).await;
    let page = admin.get(&format!("/admin/staff/{officer_id}")).await;
    assert!(
        page.body
            .contains(&format!("/admin/staff/{officer_id}/name"))
    );
    assert!(page.body.contains("name=\"display_name\""));

    let v = staff_version(&admin, &officer_id).await;
    // An `email` field is ignored: the email never changes here.
    let form = format!(
        "display_name={}&email={}&version={v}",
        urlencode("  พิมพ์ใจ ใจดี  "),
        urlencode("thief@sealbay.example")
    );
    let r = admin
        .post(&format!("/admin/staff/{officer_id}/name"), &form)
        .await;
    assert_eq!(r.status, 303);
    assert_eq!(flash(&admin), "ok:kernel.admin.saved");
    assert_eq!(name_of(&h, &officer_id).await, "พิมพ์ใจ ใจดี", "trimmed");
    let email = text(
        &h,
        "SELECT email FROM staff WHERE id = ?",
        &officer_id,
        "email",
    )
    .await;
    assert_eq!(email.as_deref(), Some(email_of("Officer").as_str()));
    assert!(
        admin
            .get(&format!("/admin/staff/{officer_id}"))
            .await
            .body
            .contains("พิมพ์ใจ ใจดี")
    );

    assert_eq!(audits(&h, "staff.rename").await, 1);
    let row = h
        .db()
        .query(harpseal::Statement::new(
            "SELECT target_id, before, after FROM audit_log WHERE action = 'staff.rename'",
        ))
        .await
        .unwrap();
    assert_eq!(
        row.get(0, "target_id").and_then(|v| v.as_text()),
        Some(officer_id.as_str())
    );
    let json = |c: &str| -> serde_json::Value {
        serde_json::from_str(row.get(0, c).and_then(|v| v.as_text()).unwrap()).unwrap()
    };
    assert_eq!(
        json("before"),
        serde_json::json!({ "display_name": before })
    );
    assert_eq!(
        json("after"),
        serde_json::json!({ "display_name": "พิมพ์ใจ ใจดี" })
    );

    // The same name again writes nothing.
    let v = staff_version(&admin, &officer_id).await;
    let same = format!("display_name={}&version={v}", urlencode("พิมพ์ใจ ใจดี"));
    admin
        .post(&format!("/admin/staff/{officer_id}/name"), &same)
        .await;
    assert_eq!(flash(&admin), "ok:kernel.admin.saved");
    assert_eq!(audits(&h, "staff.rename").await, 1);
    assert_eq!(staff_version(&admin, &officer_id).await, v);
}

#[tokio::test]
async fn an_administrator_may_rename_their_own_account() {
    let (h, app) = boot().await;
    let (admin, admin_id) = signed_in(&h, &app, "Administrator").await;
    let v = staff_version(&admin, &admin_id).await;
    admin
        .post(
            &format!("/admin/staff/{admin_id}/name"),
            &format!("display_name=Ada&version={v}"),
        )
        .await;
    assert_eq!(flash(&admin), "ok:kernel.admin.saved");
    assert_eq!(name_of(&h, &admin_id).await, "Ada");
    assert_eq!(audits(&h, "staff.rename").await, 1);
}

#[tokio::test]
async fn invalid_names_are_refused_and_write_nothing() {
    let (h, app) = boot().await;
    let (admin, _) = signed_in(&h, &app, "Administrator").await;
    let (_, officer_id) = signed_in(&h, &app, "Officer").await;
    let before = name_of(&h, &officer_id).await;
    let path = format!("/admin/staff/{officer_id}/name");
    let v = staff_version(&admin, &officer_id).await;
    let long = "ก".repeat(101);
    for bad in ["", "   ", "Pim\u{7}", "Pim\nOfficer", long.as_str()] {
        let form = format!("display_name={}&version={v}", urlencode(bad));
        let r = admin.post(&path, &form).await;
        assert_eq!(r.status, 303, "{bad:?}");
        assert_eq!(flash(&admin), "err:kernel.admin.staff.err_name", "{bad:?}");
    }
    assert_eq!(name_of(&h, &officer_id).await, before);
    assert_eq!(audits(&h, "staff.rename").await, 0);
    assert_eq!(staff_version(&admin, &officer_id).await, v);

    // 100 characters is the longest accepted (counted in characters, not bytes).
    let max = "ก".repeat(100);
    admin
        .post(
            &path,
            &format!("display_name={}&version={v}", urlencode(&max)),
        )
        .await;
    assert_eq!(flash(&admin), "ok:kernel.admin.saved");
    assert_eq!(name_of(&h, &officer_id).await, max);

    // The add form keeps the same rule.
    let add = format!(
        "email=new%40sealbay.example&display_name={}&locale=th",
        urlencode(&long)
    );
    let r = admin.post("/admin/staff/new", &add).await;
    assert_eq!(r.status, 422);
    assert!(r.body.contains("ชื่อที่แสดงต้องมี 1 ถึง 100 ตัวอักษร"));
    assert_eq!(
        audits(&h, "staff.create").await,
        2,
        "only the two signed in"
    );
}

#[tokio::test]
async fn a_stale_form_is_refused() {
    let (h, app) = boot().await;
    let (admin, _) = signed_in(&h, &app, "Administrator").await;
    let (_, officer_id) = signed_in(&h, &app, "Officer").await;
    let path = format!("/admin/staff/{officer_id}/name");
    let v = staff_version(&admin, &officer_id).await;
    admin
        .post(&path, &format!("display_name=First&version={v}"))
        .await;
    assert_eq!(flash(&admin), "ok:kernel.admin.saved");
    // A second tab still holding the old version.
    admin
        .post(&path, &format!("display_name=Second&version={v}"))
        .await;
    assert_eq!(flash(&admin), "err:kernel.admin.conflict");
    assert_eq!(name_of(&h, &officer_id).await, "First");
    assert_eq!(audits(&h, "staff.rename").await, 1);
}

#[tokio::test]
async fn only_staff_managers_can_rename() {
    let (h, app) = boot().await;
    let (officer, officer_id) = signed_in(&h, &app, "Officer").await;
    let (_, reader_id) = signed_in(&h, &app, "Reader").await;
    let before = name_of(&h, &reader_id).await;
    let v = text(
        &h,
        "SELECT CAST(updated_at AS TEXT) AS v FROM staff WHERE id = ?",
        &reader_id,
        "v",
    )
    .await
    .unwrap();
    for id in [&reader_id, &officer_id] {
        let r = officer
            .post(
                &format!("/admin/staff/{id}/name"),
                &format!("display_name=Hacked&version={v}"),
            )
            .await;
        assert_eq!(r.status, 403);
    }
    assert_eq!(name_of(&h, &reader_id).await, before);
    assert_eq!(audits(&h, "staff.rename").await, 0);
}

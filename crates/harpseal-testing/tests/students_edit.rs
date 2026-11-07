//! Students pages (#22) over HTTP, the edits: `manual_overrides`,
//! "ใช้ค่าจากทะเบียน", invalid input and manual Tags. Status and Links:
//! `students_links.rs`.

mod students_support;
mod support;

use harpseal_testing::urlencode;
use harpseal_testing::{Browser, TestResponse};
use serde_json::json;
use students_support::{audit_json, boot, flash, one, run, signed_in, version};

const EDIT: &str = "/admin/students/s1/edit";

fn edit_form(name: &str, year: &str) -> String {
    format!(
        "display_name={}&email={}&locale=th&attr.year={}&attr.programme=MSC&attr.entered=",
        urlencode(name),
        urlencode("s68010001@student.sealbay.example"),
        urlencode(year)
    )
}

/// Posts the edit form of s1 with a fresh `version`.
async fn save(b: &Browser<'_>, form: &str) -> TestResponse {
    let v = version(b, "s1").await;
    b.post(EDIT, &format!("{form}&version={v}")).await
}

async fn overrides(app: &harpseal_testing::TestApp) -> serde_json::Value {
    let raw = one(
        app,
        "SELECT manual_overrides FROM student WHERE id = 's1'",
        "manual_overrides",
    )
    .await
    .unwrap();
    serde_json::from_str(&raw).unwrap()
}

#[tokio::test]
async fn an_edit_is_audited_and_recorded_as_an_override_until_cleared() {
    let (h, app) = boot().await;
    let (b, staff_id) = signed_in(&h, &app, "Officer").await;
    let form = b.get(EDIT).await;
    assert_eq!(form.status, 200);
    assert!(form.body.contains("value=\"Nok Thale\""));
    assert!(
        form.body
            .contains("<option value=\"MSC\" selected>MSC</option>")
    );
    assert!(form.body.contains("type=\"date\""), "date attribute");
    assert!(!form.body.contains("แก้ด้วยมือ</span>"), "no override yet");

    let r = save(&b, &edit_form("Nok Thale-Sai", "3")).await;
    assert_eq!(r.status, 303, "{}", r.body);
    assert_eq!(r.header("location"), Some("/admin/students/s1"));
    assert_eq!(flash(&b), "ok:kernel.students.saved");
    assert_eq!(
        overrides(&app).await,
        json!(["display_name", "attributes.year"])
    );
    let row = app
        .query("SELECT display_name, attributes FROM student WHERE id = 's1'")
        .await;
    assert_eq!(
        row.get(0, "display_name").unwrap().as_text(),
        Some("Nok Thale-Sai")
    );
    let attrs: serde_json::Value =
        serde_json::from_str(row.get(0, "attributes").unwrap().as_text().unwrap()).unwrap();
    assert_eq!(attrs, json!({ "year": 3, "programme": "MSC" }));

    let (before, after) = audit_json(&h, "student.update").await;
    assert_eq!(
        before,
        json!({ "display_name": "Nok Thale", "attributes.year": 2, "manual_overrides": [] })
    );
    assert_eq!(
        after,
        json!({
            "display_name": "Nok Thale-Sai",
            "attributes.year": 3,
            "manual_overrides": ["display_name", "attributes.year"],
        })
    );
    let actor = one(
        &app,
        "SELECT actor_id FROM audit_log WHERE action = 'student.update'",
        "actor_id",
    )
    .await;
    assert_eq!(actor, Some(staff_id));

    // The form marks both fields and offers "ใช้ค่าจากทะเบียน" for each.
    let form = b.get(EDIT).await.body;
    assert_eq!(form.matches("แก้ด้วยมือ</span>").count(), 2);
    assert!(form.contains(
        "formaction=\"/admin/students/s1/override\" name=\"field\" value=\"attributes.year\""
    ));
    let detail = b.get("/admin/students/s1").await.body;
    assert!(detail.contains("<dt>ชั้นปี <span class=\"chip override\">แก้ด้วยมือ</span></dt>"));

    // Saving the same values again changes nothing and audits nothing.
    let r = save(&b, &edit_form("Nok Thale-Sai", "3")).await;
    assert_eq!(r.status, 303);
    assert_eq!(flash(&b), "ok:kernel.students.no_change");
    assert_eq!(
        app.count("SELECT count(*) FROM audit_log WHERE action = 'student.update'")
            .await,
        1
    );

    // "ใช้ค่าจากทะเบียน" posts the whole form plus `field`; only the
    // override list changes.
    let clear = format!("{}&field=display_name", edit_form("ignored", "9"));
    let r = b.post("/admin/students/s1/override", &clear).await;
    assert_eq!(r.status, 303);
    assert_eq!(flash(&b), "ok:kernel.students.override_cleared");
    assert_eq!(overrides(&app).await, json!(["attributes.year"]));
    assert_eq!(
        one(
            &app,
            "SELECT display_name FROM student WHERE id = 's1'",
            "display_name"
        )
        .await
        .as_deref(),
        Some("Nok Thale-Sai"),
        "the value stays until the next import"
    );
    let (before, after) = audit_json(&h, "student.override_clear").await;
    assert_eq!(
        before,
        json!({ "manual_overrides": ["display_name", "attributes.year"] })
    );
    assert_eq!(after, json!({ "manual_overrides": ["attributes.year"] }));
    // Clearing it again is a conflict, not a second audit row.
    b.post("/admin/students/s1/override", "field=display_name")
        .await;
    assert_eq!(flash(&b), "err:kernel.students.conflict");
    let n = app.count("SELECT count(*) FROM audit_log WHERE action = 'student.override_clear'");
    assert_eq!(n.await, 1);
}

#[tokio::test]
async fn invalid_values_re_render_the_form_and_write_nothing() {
    let (h, app) = boot().await;
    let (b, _) = signed_in(&h, &app, "Officer").await;
    let form = format!(
        "display_name=&email=not-an-email&locale=th&attr.year=abc&attr.programme=MBA&attr.entered={}",
        urlencode("2027-02-30")
    );
    let r = save(&b, &form).await;
    assert_eq!(r.status, 422);
    for msg in [
        "กรุณาแก้ช่องที่มีข้อผิดพลาด",
        "ต้องกรอกช่องนี้",
        "รูปแบบอีเมลไม่ถูกต้อง",
        "ต้องเป็นจำนวนเต็ม",
        "ต้องเลือกจากรายการ",
        "ต้องเป็นวันที่",
    ] {
        assert!(r.body.contains(msg), "{msg}");
    }
    assert!(r.body.contains("value=\"abc\""), "what was typed is kept");
    assert_eq!(
        app.count("SELECT count(*) FROM audit_log WHERE action LIKE 'student.%'")
            .await,
        0
    );
    assert_eq!(overrides(&app).await, json!([]));
    assert_eq!(
        one(&app, "SELECT email FROM student WHERE id = 's1'", "email")
            .await
            .as_deref(),
        Some("s68010001@student.sealbay.example")
    );
}

#[tokio::test]
async fn the_form_applies_the_roster_email_check_so_the_list_never_flags_a_saved_one() {
    let (h, app) = boot().await;
    let (b, _) = signed_in(&h, &app, "Officer").await;
    for bad in ["นก@x.ac.th", "a@x..example", "a@-x.example"] {
        let form = edit_form("Nok Thale", "2").replace(
            &urlencode("s68010001@student.sealbay.example"),
            &urlencode(bad),
        );
        let r = save(&b, &form).await;
        assert_eq!(r.status, 422, "{bad}");
        assert!(r.body.contains("รูปแบบอีเมลไม่ถูกต้อง"), "{bad}");
    }
}

#[tokio::test]
async fn manual_tags_come_and_go_and_roster_tags_stay() {
    let (h, app) = boot().await;
    run(
        &h,
        "INSERT INTO tag (id, name, created_at) VALUES ('t1', 'MSC-1', 1)",
    )
    .await;
    run(
        &h,
        "INSERT INTO student_tag (student_id, tag_id, source, added_at) VALUES ('s1', 't1', 'roster', 1)",
    )
    .await;
    let (b, staff_id) = signed_in(&h, &app, "Officer").await;

    for name in ["  ทุนเรียนดี ", "MSC-1"] {
        let r = b
            .post(
                "/admin/students/s1/tags",
                &format!("name={}", urlencode(name)),
            )
            .await;
        assert_eq!(r.status, 303);
        assert_eq!(flash(&b), "ok:kernel.students.tag_added");
    }
    b.post("/admin/students/s1/tags", "name=MSC-1").await;
    assert_eq!(flash(&b), "err:kernel.students.tag_exists");
    b.post("/admin/students/s1/tags", "name=+").await;
    assert_eq!(flash(&b), "err:kernel.students.tag_invalid");
    let manual = app
        .count("SELECT count(*) FROM student_tag WHERE student_id = 's1' AND source = 'manual'")
        .await;
    assert_eq!(manual, 2);
    let by = one(
        &app,
        "SELECT added_by FROM student_tag WHERE source = 'manual'",
        "added_by",
    )
    .await;
    assert_eq!(by, Some(staff_id));
    let (before, after) = audit_json(&h, "student.tag_add").await;
    assert_eq!(before, json!({ "manual_tags": ["ทุนเรียนดี"] }));
    assert_eq!(after, json!({ "manual_tags": ["MSC-1", "ทุนเรียนดี"] }));

    // Remove the manual MSC-1: the roster MSC-1 (same tag id) stays.
    let r = b.post("/admin/students/s1/tags/remove", "tag_id=t1").await;
    assert_eq!(r.status, 303);
    assert_eq!(flash(&b), "ok:kernel.students.tag_removed");
    let left = app
        .query("SELECT source FROM student_tag WHERE student_id = 's1' AND tag_id = 't1'")
        .await;
    assert_eq!(left.len(), 1);
    assert_eq!(left.get(0, "source").unwrap().as_text(), Some("roster"));
    b.post("/admin/students/s1/tags/remove", "tag_id=t1").await;
    assert_eq!(
        flash(&b),
        "err:kernel.students.conflict",
        "a roster Tag cannot be removed"
    );
    let (before, after) = audit_json(&h, "student.tag_remove").await;
    assert_eq!(before, json!({ "manual_tags": ["MSC-1", "ทุนเรียนดี"] }));
    assert_eq!(after, json!({ "manual_tags": ["ทุนเรียนดี"] }));
}

#[tokio::test]
async fn a_stale_form_is_a_conflict_and_writes_no_override() {
    let (h, app) = boot().await;
    let (b, _) = signed_in(&h, &app, "Officer").await;
    let stale = version(&b, "s1").await;
    // Someone else (or an import) changes the email after the form opened.
    run(
        &h,
        "UPDATE student SET email = 'nok.new@sealbay.example' WHERE id = 's1'",
    )
    .await;
    let form = format!("{}&version={stale}", edit_form("Nok Thale", "2"));
    let r = b.post(EDIT, &form).await;
    assert_eq!(r.status, 303);
    assert_eq!(r.header("location"), Some(EDIT));
    assert_eq!(flash(&b), "err:kernel.students.conflict");
    assert_eq!(
        one(&app, "SELECT email FROM student WHERE id = 's1'", "email")
            .await
            .as_deref(),
        Some("nok.new@sealbay.example"),
        "the stale value is not written back"
    );
    assert_eq!(overrides(&app).await, json!([]));
    let n = app.count("SELECT count(*) FROM audit_log WHERE action = 'student.update'");
    assert_eq!(n.await, 0);
    // A form without any version is stale too.
    let r = b.post(EDIT, &edit_form("Nok", "2")).await;
    assert_eq!(r.header("location"), Some(EDIT));
    assert_eq!(flash(&b), "err:kernel.students.conflict");
}

#[tokio::test]
async fn untouched_values_round_trip_without_becoming_overrides() {
    let (h, app) = boot().await;
    // 3.0 is a float in JSON, `Ortho` is no longer a listed programme and
    // the name has spaces the form would trim.
    run(
        &h,
        "UPDATE student SET display_name = ' Nok Thale ', \
         attributes = '{\"year\":3.0,\"programme\":\"Ortho\"}' WHERE id = 's1'",
    )
    .await;
    let (b, _) = signed_in(&h, &app, "Officer").await;
    let page = b.get(EDIT).await.body;
    assert!(
        page.contains("<option value=\"Ortho\" selected>Ortho</option>"),
        "{page}"
    );
    assert!(page.contains("value=\"3.0\""));
    // The browser posts what it shows; only the email is edited.
    let form = format!(
        "display_name={}&email={}&locale=th&attr.year=3.0&attr.programme=Ortho&attr.entered=",
        urlencode(" Nok Thale "),
        urlencode("nok@sealbay.example")
    );
    let r = save(&b, &form).await;
    assert_eq!(r.status, 303, "{}", r.body);
    assert_eq!(flash(&b), "ok:kernel.students.saved");
    assert_eq!(overrides(&app).await, json!(["email"]));
    let row = app
        .query("SELECT display_name, attributes FROM student WHERE id = 's1'")
        .await;
    assert_eq!(
        row.get(0, "display_name").unwrap().as_text(),
        Some(" Nok Thale ")
    );
    let attrs: serde_json::Value =
        serde_json::from_str(row.get(0, "attributes").unwrap().as_text().unwrap()).unwrap();
    assert_eq!(attrs, json!({ "year": 3.0, "programme": "Ortho" }));
    let (before, after) = audit_json(&h, "student.update").await;
    assert_eq!(before["email"], json!("s68010001@student.sealbay.example"));
    assert_eq!(after["email"], json!("nok@sealbay.example"));
    assert!(before.get("attributes.year").is_none());
}

#[tokio::test]
async fn a_status_set_by_hand_is_an_override_cleared_like_any_field() {
    let (h, app) = boot().await;
    let (b, _) = signed_in(&h, &app, "Officer").await;
    b.post("/admin/students/s1/status", "status=left&confirm=yes")
        .await;
    assert_eq!(overrides(&app).await, json!(["status"]));
    let form = b.get(EDIT).await.body;
    assert!(form.contains("name=\"field\" value=\"status\""));
    assert!(
        b.get("/admin/students/s1")
            .await
            .body
            .contains("พ้นสภาพ</span> <span class=\"chip override\">")
    );
    b.post("/admin/students/s1/override", "field=status").await;
    assert_eq!(flash(&b), "ok:kernel.students.override_cleared");
    assert_eq!(overrides(&app).await, json!([]));
    assert_eq!(
        one(&app, "SELECT status FROM student WHERE id = 's1'", "status")
            .await
            .as_deref(),
        Some("left"),
        "the value stays until the next import"
    );
}

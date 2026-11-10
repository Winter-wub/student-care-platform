//! Bulk actions on the Students list (#145): a checkbox per row and the
//! choice of rows, then add or remove a manual Tag, unlink (a review step
//! naming the count, the number typed for more than one Student) and
//! "ประกาศถึงที่เลือก". Each action shows and works only with its own
//! Permission and writes one audit row per Student.

mod students_support;
mod support;

use harpseal::DeploymentConfig;
use harpseal::modules::announcements::Announcements;
use harpseal_testing::{Harness, TestApp, a11y};
use students_support::{flash, run, signed_in};
use support::{Chat, ROSTER};

const CONFIG: &str = r#"
name = "Seal Bay University (bulk)"
default_locale = "th"
utc_offset = "+07:00"

[roles]
Everything = ["kernel.student.view", "kernel.student.edit", "kernel.link.revoke", "announcements.send"]
Tagger = ["kernel.student.view", "kernel.student.edit"]
Viewer = ["kernel.student.view"]
Sender = ["kernel.student.view", "announcements.send"]
"#;

async fn boot() -> (Harness, TestApp) {
    let h = Harness::new();
    let config = DeploymentConfig::from_toml(CONFIG).unwrap();
    let app = h
        .boot(
            h.builder()
                .config(config)
                .module(Announcements)
                .build()
                .unwrap(),
        )
        .await;
    for sql in ROSTER {
        run(&h, sql).await;
    }
    (h, app)
}

const BULK: &str = "/admin/students/bulk";

#[tokio::test]
async fn the_list_shows_only_the_actions_a_permission_allows() {
    let (h, app) = boot().await;
    let (b, _) = signed_in(&h, &app, "Everything").await;
    let page = b.get("/admin/students").await.body;
    for action in ["tag_add", "tag_remove", "unlink", "announce"] {
        assert!(page.contains(&format!("value=\"{action}\"")), "{action}");
    }
    assert!(page.contains("name=\"student\" value=\"s1\""));
    assert!(page.contains("value=\"filter\""));
    assert!(page.contains("id=\"bulk-count\""));
    let problems = a11y::problems(&page);
    assert!(problems.is_empty(), "{problems:?}");

    let (t, _) = signed_in(&h, &app, "Tagger").await;
    let page = t.get("/admin/students").await.body;
    assert!(page.contains("value=\"tag_add\"") && page.contains("value=\"tag_remove\""));
    assert!(!page.contains("value=\"unlink\"") && !page.contains("value=\"announce\""));

    let (v, _) = signed_in(&h, &app, "Viewer").await;
    let page = v.get("/admin/students").await.body;
    assert!(!page.contains("name=\"student\""), "no checkboxes");
    assert!(!page.contains("bulk-bar"));
    // A crafted post is refused too.
    let r = v
        .post(BULK, "which=picked&student=s1&action=tag_add&tag=x")
        .await;
    assert_eq!(r.status, 403);
    let r = t.post(BULK, "which=picked&student=s1&action=unlink").await;
    assert_eq!(r.status, 403);
    assert_eq!(app.count("SELECT count(*) FROM student_tag").await, 0);
}

#[tokio::test]
async fn adding_a_tag_reviews_then_audits_each_student() {
    let (h, app) = boot().await;
    let (b, staff) = signed_in(&h, &app, "Tagger").await;
    // s1 already has it: only s3 changes.
    let pick = "which=picked&student=s1&student=s3&action=tag_add&tag=%E0%B8%97%E0%B8%B8%E0%B8%99";
    assert_eq!(
        b.post("/admin/students/s1/tags", "name=ทุน").await.status,
        303
    );
    let review = b.post(BULK, pick).await;
    assert_eq!(review.status, 200, "{}", review.body);
    assert!(review.body.contains("ให้นักศึกษา 1 คน"), "{}", review.body);
    assert!(review.body.contains("อีก 1 คนมีป้ายนี้อยู่แล้ว"));
    assert!(review.body.contains("68010003"));
    assert!(a11y::problems(&review.body).is_empty());
    assert_eq!(
        app.count("SELECT count(*) FROM student_tag").await,
        1,
        "not yet"
    );

    let r = b.post(BULK, &format!("{pick}&confirm=yes&n=1")).await;
    assert_eq!(r.status, 303, "{}", r.body);
    assert_eq!(flash(&b), "ok:kernel.students.bulk_tag_added");
    assert_eq!(
        app.count("SELECT count(*) FROM student_tag WHERE student_id = 's3' AND source = 'manual'")
            .await,
        1
    );
    let rows = app
        .query(
            "SELECT target_id, actor_id FROM audit_log WHERE action = 'student.tag_add' \
             ORDER BY rowid",
        )
        .await;
    assert_eq!(rows.len(), 2, "one per Student, the single add included");
    assert_eq!(rows.get(1, "target_id").unwrap().as_text(), Some("s3"));
    assert_eq!(
        rows.get(1, "actor_id").unwrap().as_text(),
        Some(staff.as_str())
    );

    // Remove from the whole filter: both now hold it.
    let all = "which=filter&q=&action=tag_remove&tag=%E0%B8%97%E0%B8%B8%E0%B8%99";
    let review = b.post(BULK, all).await;
    assert!(review.body.contains("ออกจากนักศึกษา 2 คน"), "{}", review.body);
    let r = b.post(BULK, &format!("{all}&confirm=yes&n=2")).await;
    assert_eq!(r.status, 303);
    assert_eq!(flash(&b), "ok:kernel.students.bulk_tag_removed");
    assert_eq!(app.count("SELECT count(*) FROM student_tag").await, 0);
    assert_eq!(
        app.count("SELECT count(*) FROM audit_log WHERE action = 'student.tag_remove'")
            .await,
        2
    );
}

#[tokio::test]
async fn a_changed_count_asks_again() {
    let (h, app) = boot().await;
    let (b, _) = signed_in(&h, &app, "Tagger").await;
    let all = "which=filter&q=&action=tag_add&tag=x";
    let r = b.post(BULK, &format!("{all}&confirm=yes&n=3")).await;
    assert_eq!(r.status, 200, "4 match, not 3");
    assert!(r.body.contains("รายชื่อเปลี่ยนไประหว่างนี้"));
    assert_eq!(app.count("SELECT count(*) FROM student_tag").await, 0);
    // Nobody picked, and a bad Tag name: back to the list.
    assert_eq!(
        b.post(BULK, "which=picked&action=tag_add&tag=x")
            .await
            .status,
        303
    );
    assert_eq!(flash(&b), "err:kernel.students.bulk_none");
    assert_eq!(
        b.post(BULK, "which=page&action=tag_add&tag=").await.status,
        303
    );
    assert_eq!(flash(&b), "err:kernel.students.tag_invalid");
}

#[tokio::test]
async fn unlinking_many_needs_the_number_typed() {
    let (h, app) = boot().await;
    Chat::new(&h, &app, "u1").link("68010001").await;
    Chat::new(&h, &app, "u4").link("68010004").await;
    let (b, _) = signed_in(&h, &app, "Everything").await;
    let pick = "which=picked&student=s1&student=s3&student=s4&action=unlink";
    let review = b.post(BULK, pick).await;
    assert_eq!(review.status, 200);
    assert!(review.body.contains("ของนักศึกษา 2 คน"), "{}", review.body);
    assert!(review.body.contains("อีก 1 คนยังไม่ได้เชื่อมบัญชี"));
    assert!(review.body.contains("class=\"danger\""));
    assert!(review.body.contains("id=\"bulk-typed\""));
    assert!(a11y::problems(&review.body).is_empty());

    for typed in ["", "ยืนยัน", "3"] {
        let r = b
            .post(BULK, &format!("{pick}&confirm=yes&n=2&typed={typed}"))
            .await;
        assert_eq!(r.status, 422, "{typed}");
        assert!(r.body.contains("bulk-typed-error"));
    }
    assert_eq!(
        app.count("SELECT count(*) FROM link WHERE status <> 'revoked'")
            .await,
        2
    );
    h.fake_channel().take_sent();
    let r = b
        .post(BULK, &format!("{pick}&confirm=yes&n=2&typed=%E0%B9%92"))
        .await;
    assert_eq!(r.status, 303, "{}", r.body);
    assert_eq!(flash(&b), "ok:kernel.students.bulk_unlinked");
    assert_eq!(
        app.count("SELECT count(*) FROM link WHERE status <> 'revoked'")
            .await,
        0
    );
    assert_eq!(
        app.count("SELECT count(*) FROM audit_log WHERE action = 'student.unlink'")
            .await,
        2
    );
    assert_eq!(
        app.count("SELECT count(*) FROM audit_log WHERE action = 'link.revoke'")
            .await,
        2
    );
    assert_eq!(h.fake_channel().take_sent().len(), 2, "each Student told");

    // Nothing left to unlink: the confirm is blocked with the reason.
    let r = b.post(BULK, pick).await;
    assert!(r.body.contains("blocked-reason") && r.body.contains("disabled"));
}

#[tokio::test]
async fn announcing_prefills_the_editor_with_the_selection() {
    let (h, app) = boot().await;
    let (b, _) = signed_in(&h, &app, "Sender").await;
    let page = b.get("/admin/students").await.body;
    assert!(page.contains("value=\"announce\"") && !page.contains("value=\"tag_add\""));
    let r = b
        .post(BULK, "which=picked&student=s3&student=s1&action=announce")
        .await;
    assert_eq!(r.status, 303);
    let to = r.header("location").unwrap().to_string();
    assert_eq!(
        to,
        "/admin/announcements/new?scope=students&students=68010001+68010003"
    );
    let editor = b.get(&to).await.body;
    assert!(editor.contains("68010001\n68010003"), "{editor}");

    // A whole filter travels as the filter and is read on the server.
    let editor = b
        .get("/admin/announcements/new?scope=students&from=students&q=Nok")
        .await
        .body;
    assert!(editor.contains(">68010001</textarea>"), "{editor}");
    let r = b.post(BULK, "which=filter&q=&action=announce").await;
    assert_eq!(r.status, 303);
    assert!(
        r.header("location")
            .unwrap()
            .contains("students=68010001+68010002")
    );
}

#[tokio::test]
async fn more_than_2000_students_are_refused_with_the_count() {
    let (h, app) = boot().await;
    let mut values = Vec::new();
    for i in 0..2000 {
        values.push(format!(
            "('x{i}', '6902{i:04}', 'Krathing {i}', NULL, 'th', 'active', 1, 1)"
        ));
    }
    run(
        &h,
        &format!(
            "INSERT INTO student (id, student_code, display_name, email, locale, status, \
             created_at, updated_at) VALUES {}",
            values.join(", ")
        ),
    )
    .await;
    let (b, _) = signed_in(&h, &app, "Everything").await;
    let r = b.post(BULK, "which=filter&q=&action=announce").await;
    assert_eq!(r.status, 422);
    assert!(r.body.contains("เลือกไว้ 2,004 คน"), "{}", r.body);
    assert!(r.body.contains("ไม่เกิน 2,000 คน"));
    assert!(a11y::problems(&r.body).is_empty());
    let r = b.post(BULK, "which=filter&q=&action=unlink").await;
    assert_eq!(r.status, 422);
    assert!(r.body.contains("ไม่เกิน 100 คน"));
    // The editor ignores a filter over the limit: a bare form.
    let editor = b
        .get("/admin/announcements/new?scope=students&from=students&q=")
        .await
        .body;
    assert!(!editor.contains("69020000"));
    // The HTMX count names the whole filter.
    let r = b
        .post("/admin/students/bulk/count", "which=filter&q=")
        .await;
    assert!(r.body.contains("เลือกแล้ว 2,004 คน"), "{}", r.body);
}

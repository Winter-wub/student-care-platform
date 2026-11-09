//! Roster import happy paths at the Deployment seam (#24): CSV and XLSX
//! uploads of the 50-row fixture, preview counts, confirm, the Job, roster
//! Tags, the audit row, full vs upsert and manual overrides.

mod roster_support;

use roster_support::*;

const STUDENTS: &str = "SELECT count(*) FROM student";

async fn happy_path(name: &str, bytes: &[u8]) {
    let (h, app) = boot().await;
    let b = signed_in(&h, &app, "Officer").await;
    let id = previewed(&b, name, bytes, "full").await;
    let page = b.get(&format!("/admin/roster/{id}")).await;
    assert_eq!(page.status, 200);
    assert!(page.body.contains("ยืนยันนำเข้า"), "confirm button");
    let c = counts(&h, &id).await;
    assert_eq!(c["preview"]["new"], 50);
    assert_eq!(c["preview"]["empty_email"], 2);
    assert_eq!(c["preview"]["will_become_left"], 0);
    assert_eq!(c["sha256"].as_str().unwrap().len(), 64);
    assert_eq!(import_text(&h, &id, "state").await, "previewed");
    assert_eq!(
        int(&h, STUDENTS, &[]).await,
        0,
        "a preview writes no Student"
    );
    let stored = import_text(&h, &id, "file_key").await;
    assert_eq!(stored, format!("roster/{id}/{name}"));

    let r = confirm(&b, &id).await;
    assert_eq!(r.status, 303);
    assert_eq!(flash(&b), "ok:kernel.roster.confirmed");
    assert_eq!(import_text(&h, &id, "state").await, "queued");
    assert_eq!(
        run_jobs(&h, &app).await,
        2,
        "the chunk, then one left batch"
    );
    assert_eq!(import_text(&h, &id, "state").await, "done");
    assert_eq!(int(&h, STUDENTS, &[]).await, 50);
    let c = counts(&h, &id).await;
    assert_eq!(c["result"]["new"], 50);

    // A Student with roster Tags and typed attributes.
    let one = "SELECT count(*) FROM student WHERE student_code = '68010004' AND \
               display_name = 'กุ้งนาง ทะเลใส' AND json_extract(attributes, '$.year') = 4 \
               AND json_extract(attributes, '$.programme') = 'MSC-Intl' AND status = 'active'";
    assert_eq!(int(&h, one, &[]).await, 1);
    let tags = "SELECT count(*) FROM student_tag st JOIN tag t ON t.id = st.tag_id \
                JOIN student s ON s.id = st.student_id \
                WHERE s.student_code = '68010005' AND st.source = 'roster'";
    assert_eq!(int(&h, tags, &[]).await, 3, "ปี5; MSC; ชมรมดนตรี");
    let no_email = "SELECT count(*) FROM student WHERE email IS NULL";
    assert_eq!(int(&h, no_email, &[]).await, 2);

    // One audit row for the import, with the file hash and counts.
    let audit = "SELECT count(*) FROM audit_log WHERE action = 'roster.import' AND target_id = ? \
                 AND json_extract(after, '$.sha256') = json_extract((SELECT counts FROM \
                 roster_import WHERE id = ?), '$.sha256') \
                 AND json_extract(after, '$.counts.new') = 50 \
                 AND json_extract(after, '$.mode') = 'full'";
    assert_eq!(int(&h, audit, &[&id, &id]).await, 1);
    let done = b.get(&format!("/admin/roster/{id}")).await.body;
    assert!(done.contains("เสร็จแล้ว"), "{done}");
    assert!(
        !done.contains("every 2s"),
        "a finished import stops polling"
    );
}

#[tokio::test]
async fn a_csv_of_50_students_is_previewed_confirmed_and_imported() {
    happy_path("seal_bay_50.csv", CSV_50).await;
}

#[tokio::test]
async fn an_xlsx_of_the_same_students_imports_the_same_way() {
    happy_path("seal_bay_50.xlsx", XLSX_50).await;
}

#[tokio::test]
async fn full_mode_marks_missing_students_left_and_upsert_never_does() {
    let (h, app) = boot().await;
    let b = signed_in(&h, &app, "Officer").await;
    let first = previewed(&b, "a.csv", &csv(0..10), "full").await;
    confirm(&b, &first).await;
    run_jobs(&h, &app).await;
    assert_eq!(
        int(
            &h,
            "SELECT count(*) FROM student WHERE status = 'active'",
            &[]
        )
        .await,
        10
    );

    // Upsert with 3 of them and 2 new: nobody leaves.
    let up = previewed(&b, "b.csv", &csv(8..13), "upsert").await;
    let c = counts(&h, &up).await;
    assert_eq!(
        (
            c["preview"]["new"].as_i64(),
            c["preview"]["unchanged"].as_i64()
        ),
        (Some(3), Some(2))
    );
    assert_eq!(c["preview"]["will_become_left"], 0);
    assert_eq!(
        c["preview"]["warning"], false,
        "a short upsert file is fine"
    );
    confirm(&b, &up).await;
    run_jobs(&h, &app).await;
    assert_eq!(
        int(
            &h,
            "SELECT count(*) FROM student WHERE status = 'active'",
            &[]
        )
        .await,
        13
    );

    // Full with 4 of them: 9 become left, with the red warning.
    let full = previewed(&b, "c.csv", &csv(0..4), "full").await;
    let c = counts(&h, &full).await;
    assert_eq!(c["preview"]["will_become_left"], 9);
    assert_eq!(c["preview"]["warning"], true);
    assert_eq!(c["preview"]["left_names"].as_array().unwrap().len(), 9);
    let page = b.get(&format!("/admin/roster/{full}")).await.body;
    assert!(page.contains("class=\"warn\""), "red warning");
    assert!(page.contains("นักศึกษา 12"), "names that will become left");
    confirm(&b, &full).await;
    run_jobs(&h, &app).await;
    assert_eq!(
        int(
            &h,
            "SELECT count(*) FROM student WHERE status = 'active'",
            &[]
        )
        .await,
        4
    );
    assert_eq!(
        int(
            &h,
            "SELECT count(*) FROM student WHERE status = 'left'",
            &[]
        )
        .await,
        9
    );
    assert_eq!(counts(&h, &full).await["left"], 9);

    // The left ones come back with a full file: "returning".
    let back = previewed(&b, "d.csv", &csv(0..13), "full").await;
    assert_eq!(counts(&h, &back).await["preview"]["returning"], 9);
}

#[tokio::test]
async fn manual_overrides_survive_the_import_and_are_counted() {
    let (h, app) = boot().await;
    let b = signed_in(&h, &app, "Officer").await;
    let first = previewed(&b, "a.csv", &csv(0..3), "full").await;
    confirm(&b, &first).await;
    run_jobs(&h, &app).await;
    // Staff renamed 68000001 by hand; the manual Tag stays too.
    run(
        &h,
        "UPDATE student SET display_name = 'ชื่อที่แก้เอง', manual_overrides = '[\"display_name\"]' \
         WHERE student_code = '68000001'",
    )
    .await;
    run(
        &h,
        "INSERT INTO tag (id, name, created_at) VALUES ('tm', 'ทุนเรียนดี', 1)",
    )
    .await;
    run(
        &h,
        "INSERT INTO student_tag (student_id, tag_id, source, added_at) \
         SELECT id, 'tm', 'manual', 1 FROM student WHERE student_code = '68000001'",
    )
    .await;
    let mut file = String::from_utf8(csv(0..3)).unwrap();
    file = file
        .replace("นักศึกษา 1,", "นักศึกษา หนึ่ง,")
        .replace("ปี1;MSC,MSC,1", "ปี2;MSC,MSC,2");
    let id = previewed(&b, "b.csv", file.as_bytes(), "full").await;
    let c = counts(&h, &id).await;
    assert_eq!(c["preview"]["skipped_by_override"], 1);
    assert_eq!(c["preview"]["updated"], 3, "year and Tags change for all");
    confirm(&b, &id).await;
    run_jobs(&h, &app).await;
    let kept = "SELECT count(*) FROM student WHERE student_code = '68000001' \
                AND display_name = 'ชื่อที่แก้เอง' AND json_extract(attributes, '$.year') = 2";
    assert_eq!(int(&h, kept, &[]).await, 1, "name kept, year imported");
    let tags = "SELECT group_concat(t.name || ':' || st.source, ',') FROM (SELECT * FROM \
                student_tag ORDER BY source, tag_id) st JOIN tag t ON t.id = st.tag_id \
                JOIN student s ON s.id = st.student_id WHERE s.student_code = '68000001'";
    let rows = h.db().query(harpseal::Statement::new(tags)).await.unwrap();
    let got = rows.rows[0][0].as_text().unwrap().to_string();
    let mut got: Vec<&str> = got.split(',').collect();
    got.sort();
    assert_eq!(got, vec!["MSC:roster", "ทุนเรียนดี:manual", "ปี2:roster"]);
}

#[tokio::test]
async fn the_template_has_the_configured_headers_with_a_bom() {
    let (h, app) = boot().await;
    let b = signed_in(&h, &app, "Viewer").await;
    let page = b.get("/admin/roster").await;
    assert_eq!(page.status, 200);
    assert!(page.body.contains("ชื่อ-สกุล"), "headers table");
    assert!(
        !page.body.contains("name=\"file\""),
        "no upload form for a Viewer"
    );
    let r = b.get("/admin/roster/template.csv").await;
    assert_eq!(r.status, 200);
    assert_eq!(r.header("content-type"), Some("text/csv; charset=utf-8"));
    assert_eq!(r.body, format!("\u{feff}{HEADERS}\r\n"));
}

#[tokio::test]
async fn a_deployment_without_a_roster_section_refuses_uploads() {
    let h = harpseal_testing::Harness::new();
    let toml = "name = \"Seal Bay University (no roster)\"\ndefault_locale = \"th\"\n\
                [roles]\nOfficer = [\"kernel.roster.view\", \"kernel.roster.import\"]\n";
    let config = harpseal::DeploymentConfig::from_toml(toml).unwrap();
    let app = h.boot(h.builder().config(config).build().unwrap()).await;
    let b = signed_in(&h, &app, "Officer").await;
    let page = b.get("/admin/roster").await;
    // Says what is missing and who can fix it, without the config key (#125).
    assert!(
        page.body.contains("ยังไม่ได้ตั้งค่าคอลัมน์ของไฟล์รายชื่อ"),
        "{}",
        page.body
    );
    assert!(page.body.contains("กรุณาแจ้งผู้ดูแลระบบ"), "{}", page.body);
    assert!(!page.body.contains("[roster]"), "{}", page.body);
    assert!(!page.body.contains("name=\"file\""));
    let r = upload(&b, "a.csv", &csv(0..2), "full").await;
    assert_eq!(r.status, 303);
    assert_eq!(flash(&b), "err:kernel.roster.no_roster");
    assert_eq!(int(&h, "SELECT count(*) FROM roster_import", &[]).await, 0);
}

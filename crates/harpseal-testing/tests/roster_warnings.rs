//! Roster import warnings vs blocking problems (#71): a file with only
//! warnings imports, each warning class is listed with its sheet/row and
//! counted, a Deployment can make a class blocking, and the report
//! includes the warnings.

mod roster_support;

use harpseal::DeploymentConfig;
use harpseal_testing::{Harness, TestApp};
use roster_support::*;

const OK: &str = "s@student.sealbay.example";

/// One row per class: 2 has no email, 3 a malformed one, 5 and 6 share an
/// email; 4 leaves its optional gpa empty, which is fine (#134); 7 is
/// complete.
fn warned_file() -> Vec<u8> {
    csv_lines(&[
        "68000002,ปลา,,,,MSC,1,3.0",
        "68000003,กุ้ง,nok@sealbay,,,MSC,1,3.0",
        "68000004,หอย,hoi@student.sealbay.example,,,MSC,1,",
        &format!("68000005,ปู,{OK},,,MSC,1,3.0"),
        &format!("68000006,ม้า,{},,,MSC,1,3.0", OK.to_uppercase()),
        "68000007,นก,nok@student.sealbay.example,,,MSC,1,3.0",
    ])
}

/// How many warnings an import's counts hold (none are stored when 0).
fn total(counts: &serde_json::Value) -> u64 {
    counts["warnings"]["total"].as_u64().unwrap_or(0)
}

async fn boot_blocking(classes: &str) -> (Harness, TestApp) {
    let h = Harness::new();
    let toml = CONFIG.replace("[roster]\n", &format!("[roster]\nblocking = {classes}\n"));
    let config = DeploymentConfig::from_toml(&toml).unwrap();
    let app = h.boot(h.builder().config(config).build().unwrap()).await;
    (h, app)
}

#[tokio::test]
async fn a_file_with_only_warnings_previews_lists_them_and_imports() {
    let (h, app) = boot().await;
    let b = signed_in(&h, &app, "Officer").await;
    let id = previewed(&b, "w.csv", &warned_file(), "full", false).await;
    let w = &counts(&h, &id).await["warnings"];
    assert_eq!(w["total"], 4);
    assert_eq!(w["classes"]["missing_email"], 1);
    assert_eq!(w["classes"]["invalid_email"], 1);
    assert!(
        w["classes"]["empty_attribute"].is_null(),
        "a blank gpa is fine"
    );
    assert_eq!(w["classes"]["duplicate_email"], 2, "both rows are named");

    let page = b.get(&format!("/admin/roster/{id}")).await;
    assert!(page.body.contains("ยืนยันนำเข้า"), "it can be confirmed");
    assert!(page.body.contains("data-warnings=\"4\""), "{}", page.body);
    for (class, n) in [
        ("missing_email", 1),
        ("invalid_email", 1),
        ("duplicate_email", 2),
    ] {
        let tag = format!("data-class=\"{class}\" data-count=\"{n}\"");
        assert!(page.body.contains(&tag), "{tag}");
    }
    assert_eq!(page.body.matches("data-severity=\"warning\"").count(), 4);
    assert!(!page.body.contains("data-severity=\"blocking\""));
    // Each warning names its Student (#134).
    assert!(
        page.body
            .contains("<span class=\"code id\">68000002</span> ปลา"),
        "{}",
        page.body
    );
    // The warning says what it means for onboarding.
    assert!(page.body.contains("เริ่มผูกบัญชีแชทไม่ได้"), "{}", page.body);

    confirm(&b, &id).await;
    run_jobs(&h, &app).await;
    assert_eq!(import_text(&h, &id, "state").await, "done");
    assert_eq!(int(&h, "SELECT count(*) FROM student", &[]).await, 6);
    // The address is kept as typed, so Staff can see and fix it.
    let sql = "SELECT count(*) FROM student WHERE student_code = '68000003' \
               AND email = 'nok@sealbay'";
    assert_eq!(int(&h, sql, &[]).await, 1);
    let sql = "SELECT count(*) FROM student WHERE student_code = '68000002' AND email IS NULL";
    assert_eq!(int(&h, sql, &[]).await, 1);
    assert_eq!(counts(&h, &id).await["warnings"]["total"], 4, "kept after");
    let done = b.get(&format!("/admin/roster/{id}")).await;
    assert!(
        done.body.contains("data-warnings=\"4\""),
        "still on the page"
    );
}

#[tokio::test]
async fn the_report_of_a_file_that_imports_lists_its_warnings() {
    let (h, app) = boot().await;
    let b = signed_in(&h, &app, "Officer").await;
    let id = previewed(&b, "w.csv", &warned_file(), "full", false).await;
    let r = b.get(&format!("/admin/roster/{id}/report.csv")).await;
    assert_eq!(r.status, 200);
    assert!(
        r.body.starts_with("\u{feff}แถว,คอลัมน์,ปัญหา,ระดับ,นักศึกษา\r\n"),
        "{}",
        r.body
    );
    // The level, then the Student (#134), last.
    let line = |row: usize, col: &str, text: &str, who: &str| {
        let l = r
            .body
            .lines()
            .find(|l| l.starts_with(&format!("{row},{col},")));
        assert!(
            l.is_some_and(|l| l.contains(text) && l.ends_with(&format!(",เตือน,{who}"))),
            "{row} {text}"
        );
    };
    line(2, "อีเมล", "ไม่มีอีเมล", "68000002 ปลา");
    line(3, "อีเมล", "รูปแบบอีเมลไม่ถูกต้อง", "68000003 กุ้ง");
    line(5, "อีเมล", "อีเมลซ้ำกับนักศึกษาคนอื่น", "68000005 ปู");
    line(6, "อีเมล", "อีเมลซ้ำกับนักศึกษาคนอื่น", "68000006 ม้า");
    assert_eq!(r.body.lines().count(), 5, "{}", r.body);
}

#[tokio::test]
async fn an_email_an_existing_student_has_is_a_warning_unless_it_is_theirs() {
    let (h, app) = boot().await;
    let b = signed_in(&h, &app, "Officer").await;
    linked_student(&h, "old", "68000099", "active").await;
    run(
        &h,
        &format!("UPDATE student SET email = '{OK}' WHERE id = 'old'"),
    )
    .await;
    let file = csv_lines(&[
        &format!("68000001,นก,{OK},,,MSC,1,3.0"),
        "68000099,ปลา,other@student.sealbay.example,,,MSC,1,3.0",
    ]);
    // 68000099 is in the file with another email, so their old one is free.
    let id = previewed(&b, "a.csv", &file, "upsert", false).await;
    assert_eq!(total(&counts(&h, &id).await), 0, "no warnings are stored");
    b.post(&format!("/admin/roster/{id}/cancel"), "").await;

    // Only 68000001 is listed: 68000099 keeps the address.
    let file = csv_lines(&[&format!("68000001,นก,{OK},,,MSC,1,3.0")]);
    let id = previewed(&b, "b.csv", &file, "upsert", false).await;
    let w = &counts(&h, &id).await["warnings"];
    assert_eq!(
        (
            w["total"].as_u64(),
            w["classes"]["duplicate_email"].as_u64()
        ),
        (Some(1), Some(1))
    );
    b.post(&format!("/admin/roster/{id}/cancel"), "").await;

    // The Student's own address is no duplicate.
    let file = csv_lines(&[&format!("68000099,ปลา,{OK},,,MSC,1,3.0")]);
    let id = previewed(&b, "c.csv", &file, "upsert", false).await;
    assert_eq!(total(&counts(&h, &id).await), 0, "no warnings are stored");
}

#[tokio::test]
async fn a_deployment_can_make_a_warning_class_blocking() {
    let (h, app) = boot_blocking("[\"invalid_email\"]").await;
    let b = signed_in(&h, &app, "Officer").await;
    let r = upload(&b, "w.csv", &warned_file(), "full").await;
    assert_eq!(r.status, 303, "{}", r.body);
    assert_eq!(flash(&b), "err:kernel.roster.flash_refused");
    let id = import_id(&r);
    assert_eq!(import_text(&h, &id, "state").await, "failed");
    assert_eq!(int(&h, "SELECT count(*) FROM student", &[]).await, 0);
    let page = b.get(&format!("/admin/roster/{id}")).await;
    assert!(!page.body.contains("/confirm"));
    assert!(
        page.body.contains("data-severity=\"blocking\""),
        "{}",
        page.body
    );
    assert!(
        page.body
            .contains("data-class=\"invalid_email\" data-count=\"1\"")
    );
    // The other classes still warn, in the same report, after the blocking one.
    let report = b.get(&format!("/admin/roster/{id}/report.csv")).await.body;
    let lines: Vec<&str> = report.lines().collect();
    assert!(
        lines[1].starts_with("3,อีเมล,") && lines[1].ends_with(",ต้องแก้,68000003 กุ้ง"),
        "{report}"
    );
    assert!(lines[2..].iter().all(|l| l.contains(",เตือน,")), "{report}");
    assert_eq!(lines.len(), 5, "{report}");
}

#[tokio::test]
async fn a_class_that_is_not_made_blocking_still_only_warns() {
    let (h, app) = boot_blocking("[\"duplicate_email\"]").await;
    let b = signed_in(&h, &app, "Officer").await;
    let file = csv_lines(&[
        "68000002,ปลา,,,,MSC,1,",
        "68000003,กุ้ง,nok@sealbay,,,MSC,1,3.0",
    ]);
    let id = previewed(&b, "w.csv", &file, "full", false).await;
    // A missing and a malformed email; the blank gpa is fine (#134).
    assert_eq!(counts(&h, &id).await["warnings"]["total"], 2);
}

#[tokio::test]
async fn blocking_problems_are_reported_with_the_warnings_and_refuse_the_file() {
    let (h, app) = boot().await;
    let b = signed_in(&h, &app, "Officer").await;
    let file = csv_lines(&[
        "68000001,นก,,,,MSC,1,",
        "68000001,ปลา,,,,MSC,1,3.0",
        "6800000X,กุ้ง,,,,MSC,1,3.0",
    ]);
    let r = upload(&b, "a.csv", &file, "full").await;
    let id = import_id(&r);
    assert_eq!(import_text(&h, &id, "state").await, "failed");
    let report = b.get(&format!("/admin/roster/{id}/report.csv")).await.body;
    // A blocking problem names no Student; the warnings do (#134).
    assert!(
        report.contains("3,รหัสนักศึกษา,รหัสนักศึกษาซ้ำกับแถวก่อนหน้า,ต้องแก้,\r\n"),
        "{report}"
    );
    assert!(
        report.contains("4,รหัสนักศึกษา,รหัสนักศึกษาไม่ตรงรูปแบบ,ต้องแก้,\r\n"),
        "{report}"
    );
    assert!(report.contains("2,อีเมล,ไม่มีอีเมล"), "{report}");
    assert_eq!(int(&h, "SELECT count(*) FROM student", &[]).await, 0);
}

#[tokio::test]
async fn a_refused_file_shows_its_warnings_apart_from_the_blocking_problems() {
    let (h, app) = boot().await;
    let b = signed_in(&h, &app, "Officer").await;
    let file = csv_lines(&[
        "68000001,นก,,,,MSC,1,",
        "6800000X,ปลา,x@student.sealbay.example,,,MSC,1,3.0",
    ]);
    let id = import_id(&upload(&b, "a.csv", &file, "full").await);
    let page = b.get(&format!("/admin/roster/{id}")).await.body;
    assert!(
        page.contains("data-count=\"problems\">1<"),
        "one blocking: {page}"
    );
    assert!(
        page.contains("id=\"roster-warnings\" data-warnings=\"1\""),
        "{page}"
    );
    let c = counts(&h, &id).await;
    assert_eq!(
        c["warnings"]["total"], 1,
        "a missing email; a blank gpa is fine (#134)"
    );
}

#[tokio::test]
async fn a_failed_job_still_shows_the_warnings_of_the_import() {
    let (h, app) = boot().await;
    let b = signed_in(&h, &app, "Officer").await;
    let id = previewed(&b, "w.csv", &warned_file(), "full", false).await;
    run(&h, &format!(
        "UPDATE roster_import SET state = 'failed', error = '{{\"job\":\"x\"}}' WHERE id = '{id}'"
    ))
    .await;
    let page = b.get(&format!("/admin/roster/{id}")).await.body;
    assert!(page.contains("data-warnings=\"4\""), "{page}");
}

#[tokio::test]
async fn blank_optional_cells_raise_nothing_unless_the_deployment_blocks_them() {
    let lines: Vec<String> = (1..=8)
        .map(|n| format!("6800000{n},นก{n},n{n}@student.sealbay.example,,,MSC,1,"))
        .collect();
    let refs: Vec<&str> = lines.iter().map(String::as_str).collect();
    let file = csv_lines(&refs);
    // By default a blank optional column is fine (#134).
    let (h, app) = boot().await;
    let b = signed_in(&h, &app, "Officer").await;
    let id = previewed(&b, "b.csv", &file, "full", false).await;
    assert_eq!(total(&counts(&h, &id).await), 0);
    let page = b.get(&format!("/admin/roster/{id}")).await.body;
    assert!(!page.contains("id=\"roster-warnings\""), "{page}");
    // `[roster] blocking = ["empty_attribute"]` refuses it: one problem per
    // column, with the count and the first rows.
    let (h, app) = boot_blocking("[\"empty_attribute\"]").await;
    let b = signed_in(&h, &app, "Officer").await;
    let id = import_id(&upload(&b, "b.csv", &file, "full").await);
    assert_eq!(import_text(&h, &id, "state").await, "failed");
    let page = b.get(&format!("/admin/roster/{id}")).await.body;
    assert!(
        page.contains("data-class=\"empty_attribute\" data-count=\"8\""),
        "{page}"
    );
    assert_eq!(page.matches("data-severity=\"blocking\"").count(), 1);
    let report = b.get(&format!("/admin/roster/{id}/report.csv")).await.body;
    assert!(report.contains("2,เกรดเฉลี่ย,"), "{report}");
    assert!(report.contains("8 แถว เช่นแถว 2, 3, 4, 5, 6, …"), "{report}");
    assert_eq!(report.lines().count(), 2, "{report}");
}

#[tokio::test]
async fn a_left_student_or_one_a_full_import_retires_does_not_make_a_duplicate() {
    let (h, app) = boot().await;
    let b = signed_in(&h, &app, "Officer").await;
    linked_student(&h, "gone", "68000098", "left").await;
    linked_student(&h, "away", "68000099", "active").await;
    run(
        &h,
        &format!("UPDATE student SET email = '{OK}' WHERE id IN ('gone', 'away')"),
    )
    .await;
    let file = csv_lines(&[&format!("68000001,นก,{OK},,,MSC,1,3.0")]);
    // upsert: 68000099 stays active with the address.
    let id = previewed(&b, "u.csv", &file, "upsert", false).await;
    assert_eq!(
        counts(&h, &id).await["warnings"]["classes"]["duplicate_email"],
        1
    );
    b.post(&format!("/admin/roster/{id}/cancel"), "").await;
    // full: 68000099 is not in the file, so becomes left; 68000098 is left.
    let id = previewed(&b, "f.csv", &file, "full", true).await;
    assert_eq!(total(&counts(&h, &id).await), 0);
}

#[tokio::test]
async fn an_email_is_stored_trimmed_of_every_unicode_space() {
    let (h, app) = boot().await;
    let b = signed_in(&h, &app, "Officer").await;
    let file = csv_lines(&["68000001,นก,\u{a0}nok@student.sealbay.example\t,,,MSC,1,3.0"]);
    let id = previewed(&b, "n.csv", &file, "full", false).await;
    confirm(&b, &id).await;
    run_jobs(&h, &app).await;
    let sql = "SELECT count(*) FROM student WHERE email = 'nok@student.sealbay.example'";
    assert_eq!(int(&h, sql, &[]).await, 1);
}

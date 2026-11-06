//! A roster file with any error is refused whole, with a report (#24):
//! one test per error class, each writing no Student.

mod roster_support;

use roster_support::*;

/// Uploads `bytes`, expects the refusal, and returns the report CSV.
async fn refused(name: &str, bytes: &[u8]) -> (harpseal_testing::Harness, String, String) {
    let (h, app) = boot().await;
    let b = signed_in(&h, &app, "Officer").await;
    let r = upload(&b, name, bytes, "full").await;
    assert_eq!(r.status, 303, "{}", r.body);
    assert_eq!(flash(&b), "err:kernel.roster.flash_refused");
    let id = import_id(&r);
    assert_eq!(import_text(&h, &id, "state").await, "failed");
    assert_eq!(int(&h, "SELECT count(*) FROM student", &[]).await, 0);
    let page = b.get(&format!("/admin/roster/{id}")).await;
    assert!(page.body.contains("ไม่นำเข้าเลย"), "{}", page.body);
    assert!(!page.body.contains("/confirm"), "nothing to confirm");
    let report = b.get(&format!("/admin/roster/{id}/report.csv")).await;
    assert_eq!(report.status, 200);
    assert_eq!(
        report.header("content-type"),
        Some("text/csv; charset=utf-8")
    );
    assert!(report.body.starts_with("\u{feff}แถว,คอลัมน์,ปัญหา\r\n"));
    // Uploading a refused file is audited; it writes no `roster.import`.
    let audit = "SELECT count(*) FROM audit_log WHERE action = 'roster.import.upload' \
                 AND json_extract(after, '$.state') = 'failed'";
    assert_eq!(int(&h, audit, &[]).await, 1);
    (h, id, report.body)
}

#[tokio::test]
async fn a_header_mismatch_lists_missing_and_unexpected_headers() {
    let file = "รหัสนักศึกษา,ชื่อ,อีเมล,ภาษา,กลุ่ม,หลักสูตร,ชั้นปี,เกรดเฉลี่ย,หมายเหตุ\r\n\
                68000001,นก,,,,DDS,1,,x\r\n";
    let (_, _, report) = refused("bad.csv", file.as_bytes()).await;
    assert!(report.contains("1,ชื่อ-สกุล,ไม่มีคอลัมน์นี้"), "{report}");
    assert!(report.contains("1,ชื่อ,คอลัมน์นี้ไม่อยู่ในแม่แบบหรือซ้ำ"), "{report}");
    assert!(report.contains("1,หมายเหตุ,"), "{report}");
}

#[tokio::test]
async fn a_student_id_off_the_pattern_is_refused() {
    let file = csv_lines(&["6800000X,นก,,,,DDS,1,", "68000002,ปลา,,,,DDS,1,"]);
    let (_, _, report) = refused("a.csv", &file).await;
    assert!(
        report.contains("2,รหัสนักศึกษา,รหัสนักศึกษาไม่ตรงรูปแบบ"),
        "{report}"
    );
}

#[tokio::test]
async fn a_duplicate_student_id_is_refused() {
    let file = csv_lines(&["68000001,นก,,,,DDS,1,", "68000001,ปลา,,,,DDS,1,"]);
    let (_, _, report) = refused("a.csv", &file).await;
    assert!(report.contains("3,รหัสนักศึกษา,รหัสนักศึกษาซ้ำ"), "{report}");
}

#[tokio::test]
async fn a_bad_email_is_refused_and_an_empty_one_is_only_counted() {
    let file = csv_lines(&["68000001,นก,nok@sealbay,,,DDS,1,"]);
    let (_, _, report) = refused("a.csv", &file).await;
    assert!(report.contains("2,อีเมล,อีเมลไม่ถูกรูปแบบ"), "{report}");

    let (h, app) = boot().await;
    let b = signed_in(&h, &app, "Officer").await;
    let id = previewed(&b, "a.csv", &csv_lines(&["68000001,นก,,,,DDS,1,"]), "full").await;
    assert_eq!(counts(&h, &id).await["preview"]["empty_email"], 1);
}

#[tokio::test]
async fn an_enum_value_off_the_list_is_refused() {
    let file = csv_lines(&["68000001,นก,,,,MBA,1,"]);
    let (_, _, report) = refused("a.csv", &file).await;
    assert!(report.contains("2,หลักสูตร,ไม่อยู่ในค่าที่อนุญาต"), "{report}");
}

#[tokio::test]
async fn a_required_attribute_left_empty_is_refused() {
    let file = csv_lines(&["68000001,นก,,,,DDS,,", "68000002,ปลา,,,,DDS,สอง,"]);
    let (_, _, report) = refused("a.csv", &file).await;
    assert!(report.contains("2,ชั้นปี,ต้องมีค่า"), "{report}");
    assert!(report.contains("3,ชั้นปี,ต้องเป็นจำนวนเต็ม"), "{report}");
}

#[tokio::test]
async fn a_file_that_is_neither_csv_nor_xlsx_is_refused() {
    let (_, _, report) = refused("roster.pdf", b"%PDF-1.7").await;
    assert!(
        report.contains("0,,รองรับเฉพาะไฟล์ .csv และ .xlsx"),
        "{report}"
    );
}

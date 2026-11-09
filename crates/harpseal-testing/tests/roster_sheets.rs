//! Workbooks with several sheets at the Deployment seam (#70): every
//! sheet is imported past its title rows and footer, and a refused
//! workbook's page and report name the sheet, row and cell of each
//! problem.

mod roster_support;

use roster_support::*;

const COHORTS: &[u8] = include_bytes!("../../harpseal/tests/fixtures/roster/cohorts.xlsx");
const DUPLICATE: &[u8] =
    include_bytes!("../../harpseal/tests/fixtures/roster/cohorts_duplicate.xlsx");
const NO_HEADER: &[u8] =
    include_bytes!("../../harpseal/tests/fixtures/roster/cohorts_no_header.xlsx");

#[tokio::test]
async fn every_sheet_of_a_workbook_is_imported() {
    let (h, app) = boot().await;
    let b = signed_in(&h, &app, "Officer").await;
    let id = previewed(&b, "cohorts.xlsx", COHORTS, "full").await;
    assert_eq!(counts(&h, &id).await["preview"]["new"], 5);
    assert_eq!(confirm(&b, &id).await.status, 303);
    run_jobs(&h, &app).await;
    assert_eq!(import_text(&h, &id, "state").await, "done");
    let students = "SELECT count(*) FROM student WHERE status = 'active'";
    assert_eq!(int(&h, students, &[]).await, 5);
    // A year stored as text (" 2 ") in the second sheet.
    let year = "SELECT json_extract(attributes, '$.year') FROM student \
                WHERE student_code = '67010001'";
    assert_eq!(int(&h, year, &[]).await, 2);
    let footer = "SELECT count(*) FROM student WHERE display_name LIKE '%คน%' \
                  OR display_name LIKE 'หมายเหตุ%'";
    assert_eq!(int(&h, footer, &[]).await, 0, "no totals or notes row");
}

#[tokio::test]
async fn a_refused_workbook_names_the_sheet_row_and_cell_of_each_problem() {
    let (h, app) = boot().await;
    let b = signed_in(&h, &app, "Officer").await;
    let r = upload(&b, "d.xlsx", DUPLICATE, "full").await;
    assert_eq!(flash(&b), "err:kernel.roster.flash_refused");
    let id = import_id(&r);
    assert_eq!(import_text(&h, &id, "state").await, "failed");
    assert_eq!(int(&h, "SELECT count(*) FROM student", &[]).await, 0);
    let page = b.get(&format!("/admin/roster/{id}")).await.body;
    assert!(page.contains("<th>ชีต</th>"), "{page}");
    assert!(page.contains("เกรดเฉลี่ย (H3)"), "{page}");
    let report = b.get(&format!("/admin/roster/{id}/report.csv")).await.body;
    assert!(report.starts_with("\u{feff}ชีต,แถว,คอลัมน์,ปัญหา"), "{report}");
    assert!(report.contains("ปี 2,2,รหัสนักศึกษา,รหัสนักศึกษาซ้ำ"), "{report}");
    assert!(report.contains("ปี 2,3,เกรดเฉลี่ย (H3),อ่านค่าไม่ได้"), "{report}");
}

#[tokio::test]
async fn a_sheet_without_the_header_refuses_the_workbook_by_name() {
    let (h, app) = boot().await;
    let b = signed_in(&h, &app, "Officer").await;
    let r = upload(&b, "n.xlsx", NO_HEADER, "full").await;
    let id = import_id(&r);
    assert_eq!(import_text(&h, &id, "state").await, "failed");
    let report = b.get(&format!("/admin/roster/{id}/report.csv")).await.body;
    assert_eq!(report.lines().count(), 2, "one clear problem: {report}");
    // No "row 0" (#125): the problem is the sheet's, and it names the
    // headers the template expects.
    assert!(report.contains("สรุป,ทั้งชีต,,\"ไม่พบแถวหัวคอลัมน์"), "{report}");
    assert!(report.contains("คอลัมน์ที่ต้องมี: รหัสนักศึกษา, ชื่อ-สกุล"), "{report}");
}

const TRAILING: &[u8] =
    include_bytes!("../../harpseal/tests/fixtures/roster/cohorts_trailing.xlsx");

#[tokio::test]
async fn skipped_rows_are_shown_on_the_preview_and_in_the_report() {
    let (h, app) = boot().await;
    let b = signed_in(&h, &app, "Officer").await;
    let id = previewed(&b, "cohorts.xlsx", COHORTS, "full").await;
    let page = b.get(&format!("/admin/roster/{id}")).await.body;
    assert!(page.contains("data-skipped=\"2\""), "{page}");
    assert!(
        page.contains("ข้ามแถวรวม/หมายเหตุใต้ข้อมูล (ปี 1) แถว 8"),
        "{page}"
    );
    let report = b.get(&format!("/admin/roster/{id}/report.csv")).await.body;
    assert!(
        report.contains("ปี 1,8,รวม | 3,ข้ามแถวรวม/หมายเหตุใต้ข้อมูล"),
        "{report}"
    );
}

#[tokio::test]
async fn a_full_import_never_drops_a_trailing_student_into_left() {
    let (h, app) = boot().await;
    let b = signed_in(&h, &app, "Officer").await;
    let id = previewed(&b, "cohorts.xlsx", COHORTS, "full").await;
    confirm(&b, &id).await;
    run_jobs(&h, &app).await;
    let active = "SELECT count(*) FROM student WHERE status = 'active'";
    assert_eq!(int(&h, active, &[]).await, 5);
    // The next file lists one Student, then one without an id and one
    // whose id is #REF!: refused, so nobody becomes left.
    let r = upload(&b, "t.xlsx", TRAILING, "full").await;
    let id = import_id(&r);
    assert_eq!(import_text(&h, &id, "state").await, "failed");
    let report = b.get(&format!("/admin/roster/{id}/report.csv")).await.body;
    assert!(report.contains("ปี 1,6,รหัสนักศึกษา,"), "{report}");
    assert!(
        report.contains("ปี 1,7,รหัสนักศึกษา (A7),อ่านค่าไม่ได้"),
        "{report}"
    );
    assert_eq!(int(&h, active, &[]).await, 5);
}

const HIDDEN_ROSTER: &[u8] =
    include_bytes!("../../harpseal/tests/fixtures/roster/cohorts_hidden_roster.xlsx");

#[tokio::test]
async fn a_hidden_roster_sheet_blocks_a_full_import_and_is_listed_in_upsert() {
    let (h, app) = boot().await;
    let b = signed_in(&h, &app, "Officer").await;
    let id = previewed(&b, "cohorts.xlsx", COHORTS, "full").await;
    confirm(&b, &id).await;
    run_jobs(&h, &app).await;
    let active = "SELECT count(*) FROM student WHERE status = 'active'";
    assert_eq!(int(&h, active, &[]).await, 5);
    // "ปี 2" is hidden: in full mode its two Students would become left.
    let r = upload(&b, "h.xlsx", HIDDEN_ROSTER, "full").await;
    let id = import_id(&r);
    assert_eq!(import_text(&h, &id, "state").await, "failed");
    let report = b.get(&format!("/admin/roster/{id}/report.csv")).await.body;
    assert!(report.contains("ปี 2,ทั้งชีต,,ชีตนี้ซ่อนอยู่แต่มีรายชื่อ"), "{report}");
    assert_eq!(int(&h, active, &[]).await, 5, "nobody became left");
    // In upsert nobody becomes left: it previews, listing the hidden sheet.
    let id = previewed(&b, "h.xlsx", HIDDEN_ROSTER, "upsert").await;
    let page = b.get(&format!("/admin/roster/{id}")).await.body;
    assert!(page.contains("ข้ามชีตที่ซ่อนอยู่ (มีหัวคอลัมน์รายชื่อ) (ปี 2)"), "{page}");
}

#[tokio::test]
async fn a_calculated_gpa_column_is_imported() {
    let (h, app) = boot().await;
    let b = signed_in(&h, &app, "Officer").await;
    let calc = include_bytes!("../../harpseal/tests/fixtures/roster/calculated.xlsx");
    let id = previewed(&b, "calculated.xlsx", calc, "upsert").await;
    assert_eq!(counts(&h, &id).await["preview"]["new"], 3);
}

#[tokio::test]
async fn a_hidden_old_cohort_missing_a_column_still_blocks_a_full_import() {
    let (h, app) = boot().await;
    let b = signed_in(&h, &app, "Officer").await;
    let partial =
        include_bytes!("../../harpseal/tests/fixtures/roster/cohorts_hidden_partial.xlsx");
    let r = upload(&b, "p.xlsx", partial, "full").await;
    let id = import_id(&r);
    assert_eq!(import_text(&h, &id, "state").await, "failed");
    let report = b.get(&format!("/admin/roster/{id}/report.csv")).await.body;
    assert!(report.contains("ปี 0,ทั้งชีต,,ชีตนี้ซ่อนอยู่แต่มีรายชื่อ"), "{report}");
    assert!(report.contains("ปี 0,ทั้งชีต,,ข้ามชีตที่ซ่อนอยู่"), "{report}");
}

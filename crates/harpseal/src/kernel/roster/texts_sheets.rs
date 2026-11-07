//! Default texts for workbooks with several sheets (#70): sheets, skipped
//! rows and sheet-wide problems, in Thai and English.

use crate::manifest::CatalogEntry as E;

macro_rules! th_en {
    ($( ($k:expr, $th:expr, $en:expr) ),* $(,)?) => {
        &[ $( E::new($k, "th", $th), E::new($k, "en", $en), )* ]
    };
}

pub(crate) const DEFAULTS: &[E] = th_en![
    ("kernel.roster.report_sheet", "ชีต", "Sheet"),
    (
        "kernel.roster.skipped",
        "ข้ามไม่นำเข้า",
        "Skipped, not imported"
    ),
    (
        "kernel.roster.skip_footer",
        "ข้ามแถวรวม/หมายเหตุใต้ข้อมูล",
        "Totals or notes row below the data, skipped"
    ),
    (
        "kernel.roster.skip_hidden_sheet",
        "ข้ามชีตที่ซ่อนอยู่",
        "Hidden sheet, skipped"
    ),
    (
        "kernel.roster.reason_too_many_sheets",
        "มีชีตเกิน 50 ชีต",
        "More than 50 sheets"
    ),
    (
        "kernel.roster.reason_no_header_row",
        "ไม่พบแถวหัวคอลัมน์ตามแม่แบบใน 10 แถวแรกของชีตนี้",
        "No template header row in the first 10 rows of this sheet"
    ),
    (
        "kernel.roster.reason_unreadable",
        "อ่านค่าไม่ได้: เซลล์นี้เป็นข้อผิดพลาดของ Excel (เช่น #REF!)",
        "Unreadable: the cell holds an Excel error (such as #REF!)"
    ),
    (
        "kernel.roster.skip_hidden_roster_sheet",
        "ข้ามชีตที่ซ่อนอยู่ (มีหัวคอลัมน์รายชื่อ)",
        "Hidden sheet with the roster header, skipped"
    ),
    (
        "kernel.roster.reason_hidden_roster_sheet",
        "ชีตนี้ซ่อนอยู่แต่มีรายชื่อ: แบบ full จะทำให้นักศึกษาในชีตนี้กลายเป็นออก เลิกซ่อนหรือลบชีตก่อน",
        "Hidden sheet with a roster: a full import would mark its Students left; unhide or delete it first"
    ),
    (
        "kernel.roster.reason_only_hidden",
        "มีข้อมูลเฉพาะในชีตที่ซ่อนอยู่: เลิกซ่อนชีตนี้ก่อน",
        "Data only in hidden sheets: unhide this sheet first"
    ),
    (
        "kernel.roster.reason_only_footer",
        "ใต้หัวคอลัมน์มีแต่แถวที่ดูเหมือนแถวรวม/หมายเหตุ ไม่พบข้อมูลนักศึกษา",
        "Only totals or notes rows under the header, no Student rows"
    ),
];

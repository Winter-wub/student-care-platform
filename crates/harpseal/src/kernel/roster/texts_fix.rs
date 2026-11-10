//! What to do about each problem of a roster file (#140): one plain line
//! per reason, `kernel.roster.fix_<reason>`, shown under the problem in its
//! row with the row and column beside it. A reason whose own text already
//! says what to do (a hidden sheet, the example row) has none.

use crate::manifest::CatalogEntry as E;

macro_rules! th_en {
    ($( ($k:expr, $th:expr, $en:expr) ),* $(,)?) => {
        &[ $( E::new($k, "th", $th), E::new($k, "en", $en), )* ]
    };
}

pub(crate) const DEFAULTS: &[E] = th_en![
    ("kernel.roster.fix_label", "วิธีแก้:", "To fix:"),
    // The file as a whole
    (
        "kernel.roster.fix_unsupported",
        "บันทึกไฟล์เป็น .xlsx หรือ CSV UTF-8 แล้วอัปโหลดใหม่",
        "Save the file as .xlsx or CSV UTF-8 and upload it again"
    ),
    (
        "kernel.roster.fix_not_utf8",
        "ใน Excel เลือก บันทึกเป็น แล้วเลือกชนิด CSV UTF-8 แล้วอัปโหลดใหม่",
        "In Excel choose Save As, pick CSV UTF-8, and upload it again"
    ),
    (
        "kernel.roster.fix_bad_csv",
        "ดูแถวนี้ในไฟล์ เครื่องหมาย \" ต้องมีทั้งเปิดและปิด หรือบันทึกเป็น .xlsx แทน",
        "Check this row: every \" needs a closing one. Or save the file as .xlsx instead"
    ),
    (
        "kernel.roster.fix_bad_xlsx",
        "เปิดไฟล์ใน Excel แล้วบันทึกเป็น .xlsx อีกครั้ง หรือบันทึกเป็น CSV UTF-8",
        "Open the file in Excel and save it as .xlsx again, or as CSV UTF-8"
    ),
    (
        "kernel.roster.fix_empty",
        "ใส่แถวหัวคอลัมน์ตามแม่แบบและรายชื่อนักศึกษาใต้แถวนั้น แล้วอัปโหลดใหม่",
        "Add the template's header row with the Students under it, and upload again"
    ),
    (
        "kernel.roster.fix_too_many_rows",
        "แบ่งเป็นหลายไฟล์ ไฟล์ละไม่เกิน 20,000 แถว แล้วนำเข้าแบบเพิ่ม/แก้ไขทีละไฟล์",
        "Split it into files of at most 20,000 rows and import each with add/update"
    ),
    (
        "kernel.roster.fix_too_many_columns",
        "ลบคอลัมน์ที่ไม่ใช้ทางขวาของข้อมูล รวมถึงคอลัมน์ว่างที่จัดรูปแบบไว้ แล้วอัปโหลดใหม่",
        "Delete unused columns right of the data, formatted empty ones too, and upload again"
    ),
    (
        "kernel.roster.fix_too_many_sheets",
        "ลบชีตที่ไม่ใช้ให้เหลือไม่เกิน 50 ชีต แล้วอัปโหลดใหม่",
        "Delete unused sheets to keep at most 50, and upload again"
    ),
    (
        "kernel.roster.fix_too_large",
        "ลบชีตหรือคอลัมน์ที่ไม่ใช้ แล้วอัปโหลดใหม่",
        "Delete unused sheets or columns, and upload again"
    ),
    // The flash of a file the guard stops before reading it (#140): the
    // problem and the fix in one line, as there is no problems table.
    (
        "kernel.roster.too_large_upload",
        "ไฟล์ใหญ่เกิน 4 MB วิธีแก้: ลบชีตหรือคอลัมน์ที่ไม่ใช้ แล้วอัปโหลดใหม่",
        "The file is larger than 4 MB. To fix: delete unused sheets or columns, and upload again"
    ),
    (
        "kernel.roster.fix_empty_full",
        "ตรวจว่าเลือกไฟล์ถูก ถ้าจะแก้เพียงบางคน ให้เลือกแบบเพิ่ม/แก้ไข",
        "Check you chose the right file. To change only some Students, choose add/update"
    ),
    // Headers
    (
        "kernel.roster.fix_missing_header",
        "เพิ่มคอลัมน์นี้ในแถวหัวคอลัมน์ ชื่อต้องตรงกับแม่แบบ",
        "Add this column to the header row, named as in the template"
    ),
    (
        "kernel.roster.fix_unexpected_header",
        "แก้ชื่อคอลัมน์ให้ตรงกับแม่แบบ หรือลบคอลัมน์ที่ซ้ำ",
        "Rename the column as in the template, or delete the repeated one"
    ),
    (
        "kernel.roster.fix_no_header",
        "ใส่ชื่อคอลัมน์ตามแม่แบบ หรือลบข้อมูลในคอลัมน์นี้",
        "Give the column a template header, or delete its data"
    ),
    (
        "kernel.roster.fix_no_header_row",
        "ย้ายแถวหัวคอลัมน์ขึ้นมาไว้ใน 10 แถวแรก และตั้งชื่อตามแม่แบบ",
        "Move the header row into the first 10 rows, named as in the template"
    ),
    (
        "kernel.roster.fix_only_footer",
        "ใส่รายชื่อนักศึกษาใต้แถวหัวคอลัมน์ แล้วอัปโหลดใหม่",
        "Put the Students under the header row and upload again"
    ),
    // One cell
    (
        "kernel.roster.fix_required",
        "ใส่ค่าในช่องนี้ แล้วอัปโหลดใหม่",
        "Fill in this cell and upload again"
    ),
    (
        "kernel.roster.fix_bad_code",
        "แก้รหัสในช่องนี้ให้เป็นแบบเดียวกับรหัสนักศึกษาของสถาบัน ไม่มีช่องว่างหรือตัวอักษรเกิน",
        "Correct the id to the institution's student id format, without spaces or extra characters"
    ),
    (
        "kernel.roster.fix_duplicate_code",
        "นักศึกษาหนึ่งคนมีได้แถวเดียว ลบแถวที่ซ้ำ หรือแก้รหัสให้ถูกคน",
        "One row per Student: delete the repeated row or correct the id"
    ),
    (
        "kernel.roster.fix_bad_email",
        "แก้เป็นอีเมลเต็ม เช่น name@sealbay.example หรือเว้นว่างไว้",
        "Write a full address such as name@sealbay.example, or leave it empty"
    ),
    (
        "kernel.roster.fix_bad_locale",
        "ใส่รหัสภาษา เช่น th หรือ en หรือเว้นว่างเพื่อใช้ภาษาเริ่มต้น",
        "Write a language code such as th or en, or leave it empty for the default"
    ),
    (
        "kernel.roster.fix_bad_tag",
        "ย่อชื่อป้ายให้ไม่เกิน 64 ตัวอักษร",
        "Shorten the Tag name to 64 characters"
    ),
    (
        "kernel.roster.fix_tag_separator",
        "แทนตัวคั่นในช่องนี้ด้วยตัวคั่นที่หน้านำเข้ารายชื่อกำหนด",
        "Replace the separators in this cell with the one the roster page names"
    ),
    (
        "kernel.roster.fix_too_many_tags",
        "ลดป้ายในช่องนี้ให้เหลือไม่เกิน 20 ป้าย",
        "Keep at most 20 Tags in this cell"
    ),
    (
        "kernel.roster.fix_too_long",
        "ย่อข้อความในช่องนี้ให้ไม่เกิน 200 ตัวอักษร",
        "Shorten this cell to 200 characters"
    ),
    (
        "kernel.roster.fix_not_integer",
        "ใส่จำนวนเต็ม เช่น 2 ไม่มีทศนิยมหรือตัวอักษร",
        "Write a whole number such as 2, without decimals or letters"
    ),
    (
        "kernel.roster.fix_not_number",
        "ใส่ตัวเลข เช่น 3.25 ไม่มีตัวอักษรหรือหน่วย",
        "Write a number such as 3.25, without letters or units"
    ),
    (
        "kernel.roster.fix_not_date",
        "พิมพ์วันที่ใหม่ เช่น 2026-11-09 หรือตั้งรูปแบบเซลล์เป็นวันที่ใน Excel",
        "Retype the date as 2026-11-09, or format the cell as a date in Excel"
    ),
    (
        "kernel.roster.fix_out_of_range",
        "ใส่ค่าที่อยู่ในช่วงที่บอกไว้",
        "Write a value inside the range shown"
    ),
    (
        "kernel.roster.fix_not_in_list",
        "ใส่ค่าใดค่าหนึ่งที่อนุญาต สะกดให้ตรงทุกตัวอักษร",
        "Write one of the allowed values, spelt exactly"
    ),
    (
        "kernel.roster.fix_unreadable",
        "แก้สูตรในเซลล์นี้ หรือคัดลอกแล้ววางแบบค่าอย่างเดียว",
        "Fix the formula in this cell, or copy it and paste values only"
    ),
    // Warnings: the file imports; these say what to do afterwards.
    (
        "kernel.roster.fix_missing_email",
        "เพิ่มอีเมลในไฟล์ครั้งหน้า หรือเพิ่มที่หน้าของนักศึกษาคนนี้",
        "Add the email in the next file, or on this Student's page"
    ),
    (
        "kernel.roster.fix_invalid_email",
        "แก้อีเมลในไฟล์ครั้งหน้า หรือแก้ที่หน้าของนักศึกษาคนนี้",
        "Correct the email in the next file, or on this Student's page"
    ),
    (
        "kernel.roster.fix_empty_attribute",
        "ใส่ค่าในช่องนี้แล้วอัปโหลดใหม่ หรือเพิ่มที่หน้าของนักศึกษาคนนี้หลังนำเข้า",
        "Fill in this cell and upload again, or add it on this Student's page after the import"
    ),
    (
        "kernel.roster.fix_duplicate_email",
        "ตรวจว่าอีเมลนี้เป็นของใคร แล้วให้แต่ละคนมีอีเมลของตัวเอง",
        "Check whose email this is, and give each Student their own"
    ),
];

#[cfg(test)]
mod tests {
    use super::super::{texts, texts_diff, texts_reach, texts_sheets, texts_warnings};
    use super::DEFAULTS;

    /// Reasons whose own text already says what to do.
    const SAYS_ITS_FIX: [&str; 3] = ["hidden_roster_sheet", "only_hidden", "example_row"];

    #[test]
    fn every_reason_has_a_fix_or_says_its_own() {
        let all = [
            texts::DEFAULTS,
            texts_sheets::DEFAULTS,
            texts_warnings::DEFAULTS,
            texts_diff::DEFAULTS,
            texts_reach::DEFAULTS,
        ];
        let fixes: Vec<&str> = DEFAULTS.iter().map(|e| e.key).collect();
        let mut reasons = 0;
        for e in all.iter().flat_map(|t| t.iter()) {
            let Some(reason) = e.key.strip_prefix("kernel.roster.reason_") else {
                continue;
            };
            reasons += 1;
            let fix = format!("kernel.roster.fix_{reason}");
            assert!(
                fixes.contains(&fix.as_str()) || SAYS_ITS_FIX.contains(&reason),
                "{reason} has no {fix}"
            );
        }
        assert!(reasons > 30, "{reasons} reasons");
        // Each fix is for a reason that exists, in both Locales.
        for e in DEFAULTS {
            if let Some(reason) = e.key.strip_prefix("kernel.roster.fix_") {
                let key = format!("kernel.roster.reason_{reason}");
                assert!(
                    reason == "label" || all.iter().flat_map(|t| t.iter()).any(|r| r.key == key),
                    "{} fixes no reason",
                    e.key
                );
            }
        }
        for locale in ["th", "en"] {
            let n = DEFAULTS.iter().filter(|e| e.locale == locale).count();
            assert_eq!(n * 2, DEFAULTS.len(), "{locale}");
        }
    }
}

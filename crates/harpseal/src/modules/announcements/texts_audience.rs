//! Default texts of the Audience fieldset's typed filters and its short
//! live lines (#156), in Thai and English.

use crate::manifest::CatalogEntry as E;

macro_rules! th_en {
    ($( ($k:expr, $th:expr, $en:expr) ),* $(,)?) => {
        &[ $( E::new($k, "th", $th), E::new($k, "en", $en), )* ]
    };
}

pub(crate) const AUDIENCE: &[E] = th_en![
    // A numeric attribute's range: two boxes under its name.
    ("announcements.form.range_min", "ตั้งแต่", "From"),
    ("announcements.form.range_max", "ถึง", "To"),
    (
        "announcements.form.range_help",
        "นับรวมค่าที่ใส่ด้วย เว้นว่างช่องใดช่องหนึ่งได้",
        "Both ends count. Either box may be left empty."
    ),
    // The range as Staff read it in lists, the review and saved Audiences.
    (
        "announcements.form.range_between",
        "ตั้งแต่ {min} ถึง {max}",
        "{min} to {max}"
    ),
    (
        "announcements.form.range_from",
        "ตั้งแต่ {min} ขึ้นไป",
        "{min} or more"
    ),
    (
        "announcements.form.range_to",
        "ไม่เกิน {max}",
        "{max} or less"
    ),
    (
        "announcements.form.range_not_number",
        "ช่วงค่าต้องเป็นตัวเลข เช่น 2.50 แก้ช่อง \"ตั้งแต่\" หรือ \"ถึง\" ให้เป็นตัวเลข",
        "A range takes numbers, such as 2.50. Correct the \"From\" or \"To\" box."
    ),
    (
        "announcements.form.range_reversed",
        "ช่อง \"ตั้งแต่\" มีค่ามากกว่าช่อง \"ถึง\" สลับค่าสองช่องนี้",
        "\"From\" is above \"To\". Swap the two values."
    ),
    // Exact values saved before #156 under a range field: kept, never dropped.
    (
        "announcements.form.exact_values",
        "ค่าที่ระบุไว้เดิม (ค่าใดค่าหนึ่ง คั่นด้วย ,)",
        "Exact values set earlier (any of, separated by ,)"
    ),
    // The short live lines: one per change, only when it changes.
    (
        "announcements.form.count_said",
        "กลุ่มผู้รับนี้มีนักศึกษา {n} คน",
        "This Audience has {n} Students."
    ),
    (
        "announcements.editor.preview_said",
        "ตัวอย่างแสดงข้อความแล้ว",
        "The preview shows the text."
    ),
    (
        "announcements.editor.preview_cleared",
        "ตัวอย่างว่าง ยังไม่มีข้อความ",
        "The preview is empty."
    ),
];

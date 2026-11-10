//! The Student picker's texts (#146), in Thai and English. Placeholders
//! are listed per key.

use crate::manifest::CatalogEntry as E;

/// `(key, thai, english)` -> two entries.
macro_rules! th_en {
    ($( ($k:expr, $th:expr, $en:expr) ),* $(,)?) => {
        &[ $( E::new($k, "th", $th), E::new($k, "en", $en), )* ]
    };
}

pub(crate) const TEXTS: &[E] = th_en![
    // The search box's label: with names, and for Staff who see codes only.
    (
        "kernel.picker.search",
        "ค้นหาจากรหัสหรือชื่อ",
        "Search by ID or name"
    ),
    (
        "kernel.picker.search_code",
        "ค้นหาจากรหัสนักศึกษา",
        "Search by student ID"
    ),
    (
        "kernel.picker.hint",
        "พิมพ์ต้นรหัสหรือบางส่วนของชื่อ แล้วกด เพิ่ม",
        "Type the start of an ID or part of a name, then press Add."
    ),
    ("kernel.picker.add", "เพิ่ม", "Add"),
    // A hit already in the list, in place of "เพิ่ม".
    ("kernel.picker.picked", "เลือกแล้ว", "Picked"),
    // {code}
    ("kernel.picker.add_aria", "เพิ่ม {code}", "Add {code}"),
    (
        "kernel.picker.none",
        "ไม่พบนักศึกษาที่ตรงกัน ลองพิมพ์ให้สั้นลง หรือตรวจรหัสอีกครั้ง",
        "No Student matches. Try fewer characters, or check the ID."
    ),
    // The status line after a search. {n}: how many are listed.
    ("kernel.picker.found", "พบ {n} คน", "{n} found."),
    (
        "kernel.picker.found_more",
        "พบมากกว่า {n} คน แสดง {n} คนแรก",
        "More than {n} found; the first {n} are shown."
    ),
    // {n}: how many are listed.
    (
        "kernel.picker.more",
        "แสดง {n} คนแรก พิมพ์เพิ่มเพื่อหาคนอื่น",
        "Showing the first {n}. Type more to find others."
    ),
    (
        "kernel.picker.paste",
        "วางรหัสหลายคนพร้อมกัน",
        "Paste many IDs"
    ),
    (
        "kernel.picker.paste_label",
        "รหัสนักศึกษา คั่นด้วยการขึ้นบรรทัดใหม่ เว้นวรรค หรือจุลภาค",
        "Student IDs, one per line or separated by spaces or commas"
    ),
    ("kernel.picker.paste_add", "เพิ่มรหัสที่วาง", "Add these IDs"),
    // {n} picked, {max} allowed.
    (
        "kernel.picker.count",
        "เลือกแล้ว {n} จาก {max} คน",
        "{n} of {max} picked"
    ),
    (
        "kernel.picker.empty",
        "ยังไม่ได้เลือกนักศึกษา ค้นหาด้านล่าง หรือวางรหัสหลายคนพร้อมกัน",
        "No Student picked yet. Search below, or paste many IDs."
    ),
    // {max}, {n}: how many to remove. Neutral: any Module's form uses it.
    (
        "kernel.picker.over",
        "เลือกได้ไม่เกิน {max} คน เกินมา {n} คน กรุณาลบออก",
        "At most {max}: {n} too many. Please remove them."
    ),
    // {n}
    (
        "kernel.picker.unknown_n",
        "ไม่พบ {n} รหัส กรุณาตรวจรหัสหรือลบออก",
        "{n} IDs not found. Please check them or remove them."
    ),
    // {n}
    ("kernel.picker.left_n", "{n} คนพ้นสภาพแล้ว", "{n} have left."),
    ("kernel.picker.unknown", "ไม่พบรหัสนี้", "ID not found"),
    ("kernel.picker.left", "พ้นสภาพแล้ว", "Left"),
    ("kernel.picker.remove_short", "ลบ", "Remove"),
    // {code}
    ("kernel.picker.remove", "ลบ {code}", "Remove {code}"),
    ("kernel.picker.clear", "ล้างรายชื่อทั้งหมด", "Clear the list"),
    // The live region after a change. {code}, {n}: picked now.
    (
        "kernel.picker.added",
        "เพิ่ม {code} แล้ว เลือกแล้ว {n} คน",
        "Added {code}. {n} picked."
    ),
    // {code}, {n}: picked now.
    (
        "kernel.picker.already",
        "{code} อยู่ในรายชื่อแล้ว เลือกแล้ว {n} คน",
        "{code} is already picked. {n} picked."
    ),
    // {code}, {max}: the most one list holds.
    (
        "kernel.picker.full",
        "เพิ่ม {code} ไม่ได้ รายชื่อเต็ม {max} คนแล้ว",
        "{code} was not added: the list holds at most {max}."
    ),
    // After "pasted" when some did not fit. {max}
    (
        "kernel.picker.full_n",
        "รหัสที่เหลือไม่ได้เพิ่ม เพราะรายชื่อเต็ม {max} คนแล้ว",
        "The rest were not added: the list holds at most {max}."
    ),
    (
        "kernel.picker.removed",
        "ลบ {code} แล้ว เลือกแล้ว {n} คน",
        "Removed {code}. {n} picked."
    ),
    // {added}: new codes, {n}: picked now.
    (
        "kernel.picker.pasted",
        "เพิ่ม {added} รหัส เลือกแล้ว {n} คน",
        "Added {added} IDs. {n} picked."
    ),
    (
        "kernel.picker.cleared",
        "ล้างรายชื่อแล้ว",
        "The list is cleared."
    ),
];

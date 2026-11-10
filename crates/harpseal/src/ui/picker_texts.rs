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
    // {code}
    ("kernel.picker.add_aria", "เพิ่ม {code}", "Add {code}"),
    (
        "kernel.picker.none",
        "ไม่พบนักศึกษาที่ตรงกัน ลองพิมพ์ให้สั้นลง หรือตรวจรหัสอีกครั้ง",
        "No Student matches. Try fewer characters, or check the ID."
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
    // {max}, {n}: how many to remove.
    (
        "kernel.picker.over",
        "เลือกได้ไม่เกิน {max} คน กรุณาลบออก {n} คนก่อนส่ง",
        "At most {max}. Please remove {n} before sending."
    ),
    // {n}
    (
        "kernel.picker.unknown_n",
        "ไม่พบ {n} รหัส กรุณาตรวจรหัสหรือลบออก",
        "{n} IDs not found. Please check them or remove them."
    ),
    // {n}
    (
        "kernel.picker.left_n",
        "{n} คนพ้นสภาพแล้ว จะไม่ได้รับข้อความ",
        "{n} have left and will get nothing."
    ),
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

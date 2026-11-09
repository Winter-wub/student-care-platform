//! Default texts of the per-Student preview and the header error (#134),
//! in Thai and English. Placeholders: `{n}` a count, `{left}` / `{active}`
//! / `{pct}` / `{links}` the leave alarm, `{rows}` the file's rows,
//! `{shown}` / `{total}` a capped list.

use crate::manifest::CatalogEntry as E;

macro_rules! th_en {
    ($( ($k:expr, $th:expr, $en:expr) ),* $(,)?) => {
        &[ $( E::new($k, "th", $th), E::new($k, "en", $en), )* ]
    };
}

pub(crate) const DEFAULTS: &[E] = th_en![
    // What changes
    ("kernel.roster.what_changes", "สิ่งที่จะเปลี่ยน", "What changes"),
    (
        "kernel.roster.links_cut",
        "ตัดการเชื่อมบัญชีแชท {n} บัญชี",
        "{n} chat Links cut"
    ),
    (
        "kernel.roster.no_leaving_upsert",
        "แบบเพิ่ม/แก้ไขไม่ทำให้ใครพ้นสภาพ",
        "Add/update makes no one left"
    ),
    (
        "kernel.roster.alarm_left",
        "{left} จาก {active} คน ({pct}%) จะพ้นสภาพ และถูกตัดการเชื่อมบัญชีแชท {links} บัญชี ตรวจให้แน่ใจว่าไฟล์นี้เป็นทะเบียนของภาคเรียนนี้ก่อนยืนยัน",
        "{left} of {active} Students ({pct}%) will become left and {links} chat Links will be cut. Make sure this file is this term's roster before you confirm"
    ),
    (
        "kernel.roster.alarm_left_unlinked",
        "{left} จาก {active} คน ({pct}%) จะพ้นสภาพ ตรวจให้แน่ใจว่าไฟล์นี้เป็นทะเบียนของภาคเรียนนี้ก่อนยืนยัน",
        "{left} of {active} Students ({pct}%) will become left. Make sure this file is this term's roster before you confirm"
    ),
    (
        "kernel.roster.alarm_short",
        "ไฟล์มี {rows} แถว น้อยกว่านักศึกษาที่ศึกษาอยู่ {active} คน ตรวจว่าไฟล์ครบทุกคนก่อนยืนยัน",
        "The file has {rows} rows, fewer than the {active} active Students. Check that the file lists everyone before you confirm"
    ),
    // Per-Student lists
    (
        "kernel.roster.changes_open",
        "ดูรายชื่อที่ข้อมูลจะเปลี่ยน ({n} คน)",
        "See who changes ({n} Students)"
    ),
    (
        "kernel.roster.no_changes",
        "ข้อมูลของนักศึกษาที่อยู่ในไฟล์ไม่เปลี่ยน",
        "The details of the Students in the file do not change"
    ),
    (
        "kernel.roster.list_shown",
        "แสดง {shown} คนแรกจาก {total} คน ดาวน์โหลด CSV เพื่อดูครบทุกคน",
        "The first {shown} of {total} are shown. Download the CSV to see everyone"
    ),
    (
        "kernel.roster.changes_csv",
        "ดาวน์โหลดรายการเปลี่ยนแปลง (CSV)",
        "Download the changes (CSV)"
    ),
    ("kernel.roster.change", "การเปลี่ยนแปลง", "Change"),
    ("kernel.roster.links_col", "บัญชีแชทที่จะถูกตัด", "Chat Links cut"),
    ("kernel.roster.csv_field", "ข้อมูล", "Field"),
    ("kernel.roster.csv_before", "ค่าเดิม", "Before"),
    ("kernel.roster.csv_after", "ค่าใหม่", "After"),
    ("kernel.roster.empty_value", "(ว่าง)", "(empty)"),
    ("kernel.roster.report_student", "นักศึกษา", "Student"),
    (
        "kernel.roster.lists_missing",
        "ไม่มีรายชื่อของการนำเข้านี้ให้ดูแล้ว อัปโหลดไฟล์อีกครั้งเพื่อดูว่าใครข้อมูลเปลี่ยนและใครจะพ้นสภาพ",
        "The lists of this import are no longer available. Upload the file again to see who changes and who leaves"
    ),
    (
        "kernel.roster.changes_gone",
        "รายการเปลี่ยนแปลงของการนำเข้านี้ไม่มีให้ดาวน์โหลดแล้ว",
        "The changes of this import are no longer available"
    ),
    // The template's example row
    (
        "kernel.roster.reason_example_row",
        "แถวนี้เป็นแถวตัวอย่างจากแม่แบบ ลบแถวนี้แล้วอัปโหลดใหม่",
        "This is the template's example row. Delete it and upload again"
    ),
    // The header error
    (
        "kernel.roster.headers_title",
        "หัวคอลัมน์ในไฟล์ไม่ตรงกับแม่แบบ จึงไม่นำเข้าเลย",
        "The file's column headers do not match the template, so nothing was imported"
    ),
    (
        "kernel.roster.headers_help",
        "ทุกคอลัมน์ด้านล่างต้องมีหัวคอลัมน์ในไฟล์ สะกดให้ตรง เรียงลำดับใดก็ได้ แก้ไฟล์แล้วอัปโหลดใหม่ได้ที่ด้านล่างของหน้านี้",
        "Every column below needs its header in the file, spelt exactly, in any order. Fix the file and upload it again at the bottom of this page"
    ),
    (
        "kernel.roster.headers_found",
        "คอลัมน์ในไฟล์",
        "Columns in the file"
    ),
    (
        "kernel.roster.headers_found_sheet",
        "คอลัมน์ในชีต {sheet}",
        "Columns in sheet {sheet}"
    ),
    (
        "kernel.roster.headers_required",
        "คอลัมน์ที่ต้องมีค่าทุกแถว",
        "Columns every row must fill"
    ),
    (
        "kernel.roster.headers_optional",
        "คอลัมน์ที่เว้นว่างได้ (แต่ต้องมีหัวคอลัมน์)",
        "Columns that may be empty (the header is still needed)"
    ),
    (
        "kernel.roster.header_missing",
        "ไม่มีในไฟล์",
        "Not in the file"
    ),
    (
        "kernel.roster.header_unknown",
        "ไม่รู้จักหรือซ้ำ",
        "Unknown or repeated"
    ),
    ("kernel.roster.header_ok", "ตรง", "Matches"),
    (
        "kernel.roster.no_header_found",
        "ไม่พบแถวหัวคอลัมน์ในไฟล์ แถวแรกที่มีข้อมูลต้องเป็นชื่อคอลัมน์",
        "No header row was found: the first row with data must hold the column names"
    ),
];

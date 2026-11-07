//! Default texts of the roster import pages, in Thai and English.

use crate::manifest::CatalogEntry as E;

macro_rules! th_en {
    ($( ($k:expr, $th:expr, $en:expr) ),* $(,)?) => {
        &[ $( E::new($k, "th", $th), E::new($k, "en", $en), )* ]
    };
}

pub(crate) const DEFAULTS: &[E] = th_en![
    ("kernel.roster.title", "นำเข้ารายชื่อ", "Roster import"),
    (
        "kernel.roster.no_roster",
        "ระบบนี้ยังไม่ได้กำหนดคอลัมน์ของไฟล์รายชื่อ ([roster] ใน deployment.toml) จึงยังนำเข้าไม่ได้",
        "This Deployment has no roster mapping ([roster] in deployment.toml), so files cannot be imported yet"
    ),
    // Upload
    (
        "kernel.roster.upload_title",
        "อัปโหลดไฟล์รายชื่อ",
        "Upload a roster file"
    ),
    ("kernel.roster.file", "ไฟล์ CSV หรือ XLSX", "CSV or XLSX file"),
    ("kernel.roster.mode", "แบบการนำเข้า", "Import mode"),
    (
        "kernel.roster.mode_full_help",
        "ทั้งทะเบียน: ใครไม่อยู่ในไฟล์จะพ้นสภาพและถูกยกเลิกการเชื่อมบัญชีแชท",
        "Whole roster: anyone missing from the file becomes left and loses their chat Links"
    ),
    (
        "kernel.roster.mode_upsert_help",
        "เพิ่มหรือแก้ไขเท่านั้น: ไม่มีใครพ้นสภาพ",
        "Add or update only: nobody becomes left"
    ),
    ("kernel.roster.mode_full", "ทั้งทะเบียน", "Whole roster"),
    ("kernel.roster.mode_upsert", "เพิ่ม/แก้ไข", "Add/update"),
    ("kernel.roster.check", "ตรวจไฟล์", "Check the file"),
    (
        "kernel.roster.expected",
        "หัวคอลัมน์ที่ไฟล์ต้องมี",
        "Headers the file must have"
    ),
    ("kernel.roster.header", "หัวคอลัมน์", "Header"),
    ("kernel.roster.holds", "ข้อมูล", "Holds"),
    ("kernel.roster.optional", "เว้นว่างได้", "May be empty"),
    ("kernel.roster.required", "ต้องมีค่า", "Required"),
    ("kernel.roster.one_of", "หนึ่งใน", "One of"),
    ("kernel.roster.separator", "คั่นด้วย", "Separated by"),
    (
        "kernel.roster.template",
        "ดาวน์โหลดแม่แบบ CSV",
        "Download the CSV template"
    ),
    // List
    ("kernel.roster.recent", "การนำเข้าล่าสุด", "Recent imports"),
    ("kernel.roster.none", "ยังไม่มีการนำเข้า", "No imports yet"),
    ("kernel.roster.when", "เมื่อ", "When"),
    ("kernel.roster.who", "โดย", "By"),
    ("kernel.roster.state", "สถานะ", "State"),
    ("kernel.roster.rows", "แถว", "Rows"),
    ("kernel.roster.open", "เปิด", "Open"),
    (
        "kernel.roster.state_previewed",
        "รอยืนยัน",
        "Awaiting confirmation"
    ),
    ("kernel.roster.state_queued", "รอคิว", "Queued"),
    ("kernel.roster.state_running", "กำลังนำเข้า", "Running"),
    ("kernel.roster.state_done", "เสร็จแล้ว", "Done"),
    ("kernel.roster.state_failed", "ไม่สำเร็จ", "Failed"),
    ("kernel.roster.state_cancelled", "ยกเลิกแล้ว", "Cancelled"),
    // Preview and progress
    ("kernel.roster.import", "การนำเข้า", "Import"),
    ("kernel.roster.total", "แถวในไฟล์", "Rows in the file"),
    ("kernel.roster.new", "เพิ่มใหม่", "New"),
    ("kernel.roster.updated", "แก้ไข", "Updated"),
    ("kernel.roster.unchanged", "ไม่เปลี่ยน", "Unchanged"),
    (
        "kernel.roster.will_become_left",
        "จะพ้นสภาพ",
        "Will become left"
    ),
    ("kernel.roster.returning", "กลับมาศึกษา", "Returning"),
    (
        "kernel.roster.skipped_by_override",
        "ช่องที่คงค่าเดิมเพราะเจ้าหน้าที่แก้ด้วยมือ",
        "Fields kept because Staff edited them"
    ),
    (
        "kernel.roster.empty_email",
        "ไม่มีอีเมล (ต้องให้เจ้าหน้าที่ผูกบัญชีให้)",
        "No email (need a Manual Link)"
    ),
    (
        "kernel.roster.active_before",
        "ศึกษาอยู่ก่อนนำเข้า",
        "Active before the import"
    ),
    (
        "kernel.roster.warning",
        "ระวัง: จะมีนักศึกษาพ้นสภาพเกิน 20% หรือไฟล์มีแถวน้อยกว่าทะเบียนปัจจุบัน ตรวจไฟล์ให้แน่ใจก่อนยืนยัน",
        "Careful: more than 20% of Students would become left, or the file is shorter than the current roster. Check the file before you confirm"
    ),
    (
        "kernel.roster.left_names",
        "รายชื่อที่จะพ้นสภาพ",
        "Who will become left"
    ),
    ("kernel.roster.more", "และอีก", "and more:"),
    ("kernel.roster.confirm", "ยืนยันนำเข้า", "Confirm the import"),
    ("kernel.roster.cancel", "ยกเลิก", "Cancel"),
    (
        "kernel.roster.download_file",
        "ดาวน์โหลดไฟล์ต้นฉบับ",
        "Download the original file"
    ),
    (
        "kernel.roster.report",
        "ดาวน์โหลดรายงานข้อผิดพลาด (CSV)",
        "Download the error report (CSV)"
    ),
    (
        "kernel.roster.refused",
        "ไฟล์นี้มีข้อผิดพลาด จึงไม่นำเข้าเลย แก้ไฟล์แล้วอัปโหลดใหม่",
        "This file has errors, so nothing was imported. Fix the file and upload it again"
    ),
    ("kernel.roster.problems", "ข้อผิดพลาดทั้งหมด", "Problems found"),
    ("kernel.roster.progress", "นำเข้าแล้ว", "Applied"),
    ("kernel.roster.result", "ผลการนำเข้า", "Result"),
    ("kernel.roster.left", "พ้นสภาพ", "Became left"),
    (
        "kernel.roster.kept_by_status",
        "ไม่อยู่ในไฟล์แต่ยังศึกษาอยู่ เพราะเจ้าหน้าที่ตั้งสถานะเอง",
        "Not in the file but kept active: Staff set their status"
    ),
    ("kernel.roster.in_preview", "ตอนตรวจไฟล์", "In the preview"),
    ("kernel.roster.applied", "นำเข้าจริง", "Imported"),
    (
        "kernel.roster.file_pruned",
        "ลบไฟล์แล้ว (เก็บไว้ 1 ปี)",
        "File deleted (kept for one year)"
    ),
    (
        "kernel.roster.links_revoked",
        "ยกเลิกการเชื่อมบัญชี",
        "Links revoked"
    ),
    (
        "kernel.roster.notices_failed",
        "แจ้งนักศึกษาไม่สำเร็จ",
        "Notices not delivered"
    ),
    (
        "kernel.roster.job_failed",
        "การนำเข้าหยุดกลางทาง",
        "The import stopped part-way"
    ),
    (
        "kernel.roster.stuck_help",
        "ถ้าการนำเข้านี้ค้างนานเกินไป ทำเครื่องหมายว่าไม่สำเร็จแล้วอัปโหลดใหม่ได้",
        "If this import is stuck, mark it failed and upload again"
    ),
    (
        "kernel.roster.mark_failed",
        "ทำเครื่องหมายว่าไม่สำเร็จ",
        "Mark as failed"
    ),
    (
        "kernel.roster.marked_failed",
        "ทำเครื่องหมายว่าไม่สำเร็จแล้ว",
        "Marked as failed"
    ),
    (
        "kernel.roster.not_stuck",
        "การนำเข้านี้เสร็จไปแล้ว หรือยังไม่ค้างนานพอ",
        "This import has finished, or has not been stuck long enough"
    ),
    (
        "kernel.roster.report_truncated",
        "แสดง {shown} จาก {total} ปัญหา",
        "{shown} of {total} problems shown"
    ),
    (
        "kernel.roster.back",
        "กลับไปหน้านำเข้า",
        "Back to roster import"
    ),
    (
        "kernel.roster.not_found",
        "ไม่พบการนำเข้านี้",
        "There is no such import"
    ),
    // Flashes
    (
        "kernel.roster.previewed",
        "ตรวจไฟล์แล้ว ดูตัวเลขให้ครบก่อนกดยืนยัน",
        "File checked. Review the numbers before you confirm"
    ),
    (
        "kernel.roster.flash_refused",
        "ไฟล์มีข้อผิดพลาด",
        "The file has errors"
    ),
    (
        "kernel.roster.busy",
        "มีการนำเข้าอื่นที่ยังไม่เสร็จ ทำให้เสร็จหรือยกเลิกก่อน",
        "Another import is not finished yet: finish or cancel it first"
    ),
    (
        "kernel.roster.confirmed",
        "เริ่มนำเข้าแล้ว",
        "The import has started"
    ),
    (
        "kernel.roster.not_previewed",
        "การนำเข้านี้ถูกยืนยันหรือยกเลิกไปแล้ว",
        "This import was already confirmed or cancelled"
    ),
    (
        "kernel.roster.cancelled",
        "ยกเลิกการนำเข้าแล้ว",
        "Import cancelled"
    ),
    ("kernel.roster.no_file", "เลือกไฟล์ก่อน", "Choose a file first"),
    (
        "kernel.roster.bad_upload",
        "อ่านไฟล์ที่อัปโหลดไม่ได้ (ไฟล์ต้องไม่เกิน 4 MB)",
        "The upload could not be read (files must be 4 MB or less)"
    ),
    // Report
    ("kernel.roster.report_sheet", "ชีต", "Sheet"),
    ("kernel.roster.report_row", "แถว", "Row"),
    ("kernel.roster.report_column", "คอลัมน์", "Column"),
    ("kernel.roster.report_reason", "ปัญหา", "Problem"),
    (
        "kernel.roster.reason_missing_header",
        "ไม่มีคอลัมน์นี้",
        "Header missing"
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
        "kernel.roster.reason_unexpected_header",
        "คอลัมน์นี้ไม่อยู่ในแม่แบบหรือซ้ำ",
        "Header not in the template, or repeated"
    ),
    (
        "kernel.roster.reason_unsupported",
        "รองรับเฉพาะไฟล์ .csv และ .xlsx",
        "Only .csv and .xlsx files are accepted"
    ),
    (
        "kernel.roster.reason_not_utf8",
        "ไฟล์ CSV ต้องเข้ารหัส UTF-8",
        "CSV files must be UTF-8"
    ),
    (
        "kernel.roster.reason_bad_csv",
        "เครื่องหมายคำพูดในแถวนี้ไม่ครบคู่",
        "Broken quotes in this row"
    ),
    (
        "kernel.roster.reason_bad_xlsx",
        "เปิดไฟล์ XLSX ไม่ได้",
        "The XLSX file cannot be opened"
    ),
    (
        "kernel.roster.reason_empty",
        "ไฟล์ว่าง ไม่มีแถวหัวคอลัมน์",
        "The file is empty: no header row"
    ),
    (
        "kernel.roster.reason_too_many_rows",
        "ไฟล์มีเกิน 20,000 แถว",
        "More than 20,000 rows"
    ),
    (
        "kernel.roster.reason_too_many_columns",
        "ไฟล์ XLSX ใช้คอลัมน์เกิน 64 คอลัมน์",
        "The XLSX sheet uses more than 64 columns"
    ),
    (
        "kernel.roster.reason_too_many_tags",
        "กลุ่มเกิน 20 กลุ่มต่อคน",
        "More than 20 Tags for one Student"
    ),
    (
        "kernel.roster.reason_empty_full",
        "ไฟล์ไม่มีนักศึกษาเลย ถ้านำเข้าแบบทั้งทะเบียนทุกคนจะพ้นสภาพ จึงไม่นำเข้า (แบบเพิ่ม/แก้ไขใช้ได้)",
        "The file lists no Students: a whole-roster import would mark everyone left, so it is refused (add/update is allowed)"
    ),
    (
        "kernel.roster.reason_too_large",
        "ไฟล์ใหญ่เกิน 4 MB",
        "The file is larger than 4 MB"
    ),
    (
        "kernel.roster.reason_required",
        "ต้องมีค่า",
        "A value is required"
    ),
    (
        "kernel.roster.reason_bad_code",
        "รหัสนักศึกษาไม่ตรงรูปแบบ",
        "The student ID does not fit the pattern"
    ),
    (
        "kernel.roster.reason_duplicate_code",
        "รหัสนักศึกษาซ้ำกับแถวก่อนหน้า",
        "The student ID repeats an earlier row"
    ),
    (
        "kernel.roster.reason_bad_email",
        "อีเมลไม่ถูกรูปแบบ (ใช้ได้เฉพาะตัวอักษรภาษาอังกฤษ ตัวเลข และเครื่องหมาย)",
        "Not a valid email address (Latin letters, digits and symbols only)"
    ),
    (
        "kernel.roster.reason_no_header",
        "มีข้อมูลในคอลัมน์ที่ไม่มีหัวคอลัมน์",
        "Data in a column without a header"
    ),
    (
        "kernel.roster.reason_bad_locale",
        "รหัสภาษาไม่ถูกต้อง (เช่น th, en)",
        "Not a language code (e.g. th, en)"
    ),
    (
        "kernel.roster.reason_bad_tag",
        "ชื่อกลุ่มยาวเกิน 64 ตัวอักษร",
        "A Tag name is longer than 64 characters"
    ),
    (
        "kernel.roster.reason_tag_separator",
        "ใช้ตัวคั่นกลุ่มผิด",
        "Wrong Tag separator"
    ),
    (
        "kernel.roster.reason_too_long",
        "ยาวเกิน 200 ตัวอักษร",
        "Longer than 200 characters"
    ),
    (
        "kernel.roster.reason_not_integer",
        "ต้องเป็นจำนวนเต็ม",
        "Must be a whole number"
    ),
    (
        "kernel.roster.reason_not_number",
        "ต้องเป็นตัวเลข",
        "Must be a number"
    ),
    (
        "kernel.roster.reason_not_date",
        "ต้องเป็นวันที่ YYYY-MM-DD",
        "Must be a date YYYY-MM-DD"
    ),
    (
        "kernel.roster.reason_not_in_list",
        "ไม่อยู่ในค่าที่อนุญาต",
        "Not one of the allowed values"
    ),
];

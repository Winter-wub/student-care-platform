//! The default texts of the announcement create / edit form, Thai and
//! English (split from `texts.rs`, #157).

use crate::manifest::CatalogEntry as E;

/// `(key, thai, english)` -> two entries.
macro_rules! th_en {
    ($( ($k:expr, $th:expr, $en:expr) ),* $(,)?) => {
        &[ $( E::new($k, "th", $th), E::new($k, "en", $en), )* ]
    };
}

/// The create / edit form.
pub(crate) const FORM: &[E] = th_en![
    (
        "announcements.form.thai",
        "ภาษาไทย (จำเป็น)",
        "Thai (required)"
    ),
    (
        "announcements.form.english",
        "ภาษาอังกฤษ (ไม่บังคับ)",
        "English (optional)"
    ),
    ("announcements.form.title", "หัวข้อ", "Title"),
    ("announcements.form.body", "รายละเอียด", "Details"),
    (
        "announcements.form.require_ack",
        "ขอให้กดรับทราบ",
        "Ask for an acknowledgement"
    ),
    (
        "announcements.form.expires",
        "หมดอายุหลังวันที่",
        "Expires after"
    ),
    (
        "announcements.form.notify",
        "แจ้งผู้รับว่ามีการแก้ไข",
        "Notify recipients of the edit"
    ),
    (
        "announcements.form.notify_help",
        "LINE แก้ข้อความที่ส่งไปแล้วไม่ได้ จึงส่งข้อความสั้น ๆ พร้อมลิงก์ไปหาผู้รับคนละ 1 ข้อความ (นับโควตา push) ส่วน Telegram แก้ข้อความเดิมให้เลยโดยไม่ส่งเพิ่ม",
        "LINE cannot edit a sent message, so each recipient there gets one short message with a link (counts against the push Quota). Telegram edits the message in place and sends nothing more."
    ),
    // {count}
    (
        "announcements.form.notify_reach",
        "แจ้งได้สูงสุด {count} คน (ใช้ข้อความที่คิดค่าบริการไม่เกิน {count} ข้อความ เฉพาะคนที่ข้อความในภาษาของตัวเองเปลี่ยน)",
        "up to {count} recipients (at most {count} pushes: only those whose own language's text changes)"
    ),
    // {left}
    (
        "announcements.form.notify_over",
        "เกินโควตาที่เหลือของเดือนนี้ (เหลือ {left} ครั้ง) ยังส่งได้ แต่จะใช้เกินโควตา",
        "More than this month's remaining Quota ({left} left). It still sends, over the Quota."
    ),
    (
        "announcements.form.notify_closed",
        "ข่าวนี้หมดอายุแล้ว (หรือจะหมดอายุเมื่อบันทึก) จึงแจ้งไม่ได้ ขยายวันหมดอายุก่อน แล้วค่อยแก้พร้อมแจ้งอีกครั้ง",
        "This announcement has expired (or would on saving), so no notice can go out. Extend its expiry first, then edit again with the notice."
    ),
    (
        "announcements.form.notify_unchanged",
        "ข้อความไม่ได้เปลี่ยน จึงไม่มีอะไรต้องแจ้ง เอาเครื่องหมายออกหรือแก้ข้อความ",
        "No text changed, so there is nothing to notify: untick the box or change the text."
    ),
    ("announcements.form.when", "เวลาส่ง", "When to send"),
    (
        "announcements.form.now",
        "ส่งทันทีเมื่อยืนยัน",
        "As soon as it is confirmed"
    ),
    ("announcements.form.later", "ตั้งเวลาส่ง", "At a set time"),
    ("announcements.form.audience", "ผู้รับ", "Audience"),
    (
        "announcements.form.audience_help",
        "ทุกเงื่อนไขต้องตรงพร้อมกัน ในแต่ละช่องตรงค่าใดค่าหนึ่งก็พอ",
        "Every filter must match; within one filter any value matches."
    ),
    (
        "announcements.form.any_of",
        "ค่าใดค่าหนึ่ง คั่นด้วย ,",
        "any of, separated by ,"
    ),
    ("announcements.form.tags", "ป้าย", "Tags"),
    (
        "announcements.form.fix_errors",
        "กรุณาแก้ช่องที่มีข้อผิดพลาดด้านล่าง",
        "Please fix the fields marked below"
    ),
    (
        "announcements.form.include_unreachable",
        "นับบัญชีที่ติดต่อไม่ได้ด้วย (จะข้ามตอนส่ง)",
        "Count unreachable accounts too (skipped when sending)"
    ),
    (
        "announcements.form.everyone",
        "นักศึกษาทุกคนที่ผูกบัญชี",
        "Every linked Student"
    ),
    (
        "announcements.form.counting",
        "กำลังนับผู้รับ…",
        "Counting the Audience…"
    ),
    (
        "announcements.form.audience_invalid",
        "เงื่อนไขผู้รับไม่ถูกต้อง",
        "The Audience filters are not valid."
    ),
    ("announcements.form.students", "นักศึกษา", "Students"),
    ("announcements.form.links", "บัญชี", "accounts"),
    (
        "announcements.form.unreachable_skipped",
        "ติดต่อไม่ได้ จะข้าม",
        "unreachable, skipped"
    ),
    (
        "announcements.form.estimate_channel",
        "ข้อความที่คิดค่าบริการประมาณ",
        "about pushes:"
    ),
    (
        "announcements.form.estimate",
        "จะใช้ข้อความที่คิดค่าบริการประมาณ",
        "Will use about"
    ),
    ("announcements.form.times", "ข้อความ", "metered messages"),
    (
        "announcements.form.preview",
        "ตัวอย่างที่นักศึกษาเห็น",
        "What Students see"
    ),
    ("announcements.form.save_draft", "บันทึกฉบับร่าง", "Save draft"),
    (
        "announcements.form.confirm",
        "ตรวจก่อนส่ง",
        "Review before sending"
    ),
    ("announcements.form.save", "บันทึก", "Save"),
    (
        "announcements.form.sent_note",
        "ประกาศนี้ส่งแล้ว แก้ได้เฉพาะข้อความและวันหมดอายุ ช่องทางที่แก้ข้อความได้ (Telegram) จะเห็นข้อความใหม่ ส่วน LINE จะไม่เปลี่ยน",
        "This announcement was sent: only the texts and the expiry can change. Channels that can edit messages (Telegram) show the new text; LINE does not."
    ),
    (
        "announcements.form.th_required",
        "ต้องมีหัวข้อและรายละเอียดภาษาไทย",
        "A Thai title and details are required."
    ),
    (
        "announcements.form.en_both",
        "ภาษาอังกฤษต้องมีทั้งหัวข้อและรายละเอียด หรือเว้นว่างทั้งคู่",
        "Give both an English title and details, or neither."
    ),
    (
        "announcements.form.too_long",
        "หัวข้อยาวได้ไม่เกิน 100 ตัวอักษร รายละเอียดไม่เกิน 2000",
        "Titles are at most 100 characters and details at most 2000."
    ),
    (
        "announcements.form.bad_expires",
        "วันหมดอายุต้องเป็นวันที่ในอนาคต",
        "The expiry must be a date in the future."
    ),
    (
        "announcements.form.bad_send_at",
        "เวลาส่งต้องเป็นเวลาในอนาคต",
        "The send time must be in the future."
    ),
    (
        "announcements.form.bad_audience",
        "เงื่อนไขผู้รับไม่ถูกต้อง",
        "The Audience filters are not valid."
    ),
    (
        "announcements.form.students_too_many",
        "เลือกรายชื่อเองได้ไม่เกิน 2000 รหัส",
        "A hand-picked list holds at most 2000 student IDs."
    ),
    (
        "announcements.form.hand_picked",
        "รายชื่อที่เลือกเอง (รหัสนักศึกษา)",
        "Hand-picked Students (student IDs)"
    ),
    (
        "announcements.form.hand_picked_help",
        "วางรหัสนักศึกษา บรรทัดละรหัส หรือคั่นด้วย , หรือเว้นวรรค ถ้ามีเงื่อนไขอื่นด้วย ต้องอยู่ในรายชื่อและตรงเงื่อนไขอื่นพร้อมกัน ถ้ากรอกแค่รายชื่อ จะส่งเฉพาะคนในรายชื่อ",
        "Paste student IDs, one per line or separated by , or spaces. With other filters a Student must be on the list and match them; on its own the list is the whole Audience."
    ),
    (
        "announcements.form.pick_search",
        "ค้นหานักศึกษาเพื่อเพิ่มเข้ารายชื่อ (ชื่อหรือรหัส)",
        "Find Students to add (name or ID)"
    ),
    ("announcements.form.pick_add", "เพิ่ม", "Add"),
    (
        "announcements.form.pick_none",
        "ไม่พบนักศึกษา",
        "No Students found."
    ),
    (
        "announcements.form.pick_more",
        "แสดง 50 คนแรก ค้นหาให้แคบลงเพื่อเห็นคนอื่น",
        "Showing the first 50: narrow the search to see others."
    ),
    (
        "announcements.form.hp_listed",
        "รหัสในรายชื่อ",
        "IDs on the list"
    ),
    (
        "announcements.form.hp_unknown",
        "ไม่พบรหัสเหล่านี้ จะไม่ส่ง",
        "Not found, will not be sent"
    ),
    (
        "announcements.form.hp_unlinked",
        "ยังไม่ได้ผูกบัญชี จะไม่ส่ง",
        "No live Link, will not be sent"
    ),
    (
        "announcements.form.hp_filtered",
        "ไม่ตรงเงื่อนไขอื่น จะไม่ส่ง:",
        "Do not match the other filters, will not be sent:"
    ),
    ("announcements.form.hp_more", "และอีก", "and"),
];

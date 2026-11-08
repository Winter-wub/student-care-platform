//! Default texts of the explicit Audience choice and the review step
//! before a send (#120), in Thai and English.

use crate::manifest::CatalogEntry as E;

macro_rules! th_en {
    ($( ($k:expr, $th:expr, $en:expr) ),* $(,)?) => {
        &[ $( E::new($k, "th", $th), E::new($k, "en", $en), )* ]
    };
}

pub(crate) const REVIEW: &[E] = th_en![
    // The Audience choice on the form.
    (
        "announcements.form.scope_everyone",
        "ทุกคน: นักศึกษาทุกคนที่เชื่อมบัญชี",
        "Everyone: every linked Student"
    ),
    (
        "announcements.form.scope_groups",
        "เลือกกลุ่ม: ตามข้อมูลนักศึกษาหรือกลุ่ม (Tag) ด้านล่าง",
        "Choose groups: by the Student data or Tags below"
    ),
    (
        "announcements.form.scope_students",
        "เลือกรายคน: ตามรหัสนักศึกษาที่เพิ่มด้านล่าง",
        "Choose Students: by the student ids added below"
    ),
    (
        "announcements.form.scope_required",
        "ยังไม่ได้เลือกผู้รับ เลือก ทุกคน เลือกกลุ่ม หรือเลือกรายคน",
        "Choose the Audience: everyone, groups or Students"
    ),
    (
        "announcements.form.groups_empty",
        "เลือกกลุ่มไว้แต่ยังไม่ได้ระบุกลุ่ม เลือกค่าอย่างน้อยหนึ่งช่อง หรือเปลี่ยนเป็น ทุกคน",
        "Groups is chosen but no group is set: set at least one, or choose everyone"
    ),
    (
        "announcements.form.students_empty",
        "เลือกรายคนไว้แต่ยังไม่มีรหัสนักศึกษา เพิ่มอย่างน้อยหนึ่งคน",
        "Students is chosen but no student id is added: add at least one"
    ),
    // The review step.
    (
        "announcements.review.title",
        "ตรวจก่อนส่ง",
        "Review before sending"
    ),
    (
        "announcements.review.lead",
        "ยังไม่ได้ส่ง ตรวจผู้รับ เวลาส่ง และข้อความ แล้วกดยืนยัน",
        "Nothing is sent yet. Check the Audience, the timing and the message, then confirm."
    ),
    ("announcements.review.who", "ผู้รับ", "Audience"),
    (
        "announcements.review.everyone",
        "ทุกคนที่เชื่อมบัญชี",
        "Everyone linked"
    ),
    (
        "announcements.review.groups",
        "ตามกลุ่มที่เลือก",
        "The groups chosen"
    ),
    (
        "announcements.review.picked",
        "เลือกรายคน {n} รหัส",
        "{n} hand-picked student ids"
    ),
    ("announcements.review.count", "จำนวน", "Recipients"),
    ("announcements.review.people", "{n} คน", "{n} Students"),
    ("announcements.review.channels", "ช่องทาง", "Channels"),
    (
        "announcements.review.channel",
        "{channel} {n} บัญชี",
        "{channel}: {n} accounts"
    ),
    (
        "announcements.review.skipped",
        "ติดต่อไม่ได้ {n} บัญชี จะข้าม",
        "{n} unreachable, skipped"
    ),
    ("announcements.review.quota", "โควตาข้อความ", "Message Quota"),
    (
        "announcements.review.quota_left",
        "จะใช้ข้อความ {channel} ประมาณ {n} ข้อความ จากโควตาเดือนนี้ที่เหลือ {left}",
        "Will use about {n} {channel} messages of the {left} left this month"
    ),
    (
        "announcements.review.quota_over",
        "เกินโควตาที่เหลือ {over} ข้อความ ระบบยังส่งต่อ แต่อาจมีค่าใช้จ่ายเพิ่ม",
        "{over} over what is left: sending goes on, but it may cost extra"
    ),
    (
        "announcements.review.quota_no_cap",
        "จะใช้ข้อความ {channel} ประมาณ {n} ข้อความ (ไม่ได้ตั้งโควตารายเดือน)",
        "Will use about {n} {channel} messages (no monthly Quota set)"
    ),
    (
        "announcements.review.quota_none",
        "ไม่ใช้โควตาข้อความ",
        "Uses no message Quota"
    ),
    ("announcements.review.when", "เวลาส่ง", "When"),
    (
        "announcements.review.now",
        "ส่งทันทีเมื่อกดยืนยัน",
        "As soon as you confirm"
    ),
    (
        "announcements.review.later",
        "ส่งตามเวลา {time}",
        "At {time}"
    ),
    ("announcements.review.confirm", "ยืนยันส่ง", "Confirm and send"),
    (
        "announcements.review.everyone_button",
        "ส่งถึงทุกคน {n} คน",
        "Send to everyone: {n}"
    ),
    ("announcements.review.back", "กลับไปแก้ไข", "Back to editing"),
    (
        "announcements.review.changed",
        "จำนวนผู้รับเปลี่ยนไปจากที่ตรวจไว้ ยังไม่ได้ส่ง ตรวจอีกครั้งแล้วกดยืนยัน",
        "The number of recipients changed since you checked. Nothing was sent; check again and confirm."
    ),
    (
        "announcements.review.not_draft",
        "ประกาศนี้ส่งหรือตั้งเวลาไปแล้ว",
        "This announcement was already sent or scheduled"
    ),
];

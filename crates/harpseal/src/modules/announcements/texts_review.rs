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
    ("announcements.form.scope_legend", "ส่งถึง", "Send to"),
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
        "announcements.form.scope_combined",
        "ผู้รับของประกาศนี้ใช้ทั้งกลุ่มและรายคนพร้อมกัน (ต้องตรงทั้งสองอย่าง) ถ้าบันทึกโดยไม่เลือก จะใช้ทั้งสองอย่างเหมือนเดิม ถ้าเลือก \"เลือกกลุ่ม\" รายคนจะถูกตัดออก ถ้าเลือก \"เลือกรายคน\" กลุ่มจะถูกตัดออก และถ้าเลือก \"ทุกคน\" ทั้งสองอย่างจะถูกตัดออก ผู้รับอาจเพิ่มขึ้นมาก",
        "This Audience uses groups and hand-picked Students together (both must match). Saved without a choice, it keeps both. \"Choose groups\" drops the list, \"Choose Students\" drops the groups, and \"Everyone\" drops both: the Audience may grow a lot."
    ),
    (
        "announcements.form.scope_required",
        "ยังไม่ได้เลือกผู้รับ เลือก \"ทุกคน\" \"เลือกกลุ่ม\" หรือ \"เลือกรายคน\"",
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
        "announcements.review.no_links",
        "ยังไม่มีบัญชีแชทที่เชื่อม จึงไม่มีใครได้รับ",
        "No linked chat accounts: nobody receives it"
    ),
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
    // Over what is left, by what happens then (#120). LINE with a package
    // that buys extra messages:
    (
        "announcements.review.quota_over",
        "เกินโควตาที่เหลือ {over} ข้อความ แพ็กเกจนี้ซื้อข้อความเพิ่มได้ จึงยังส่งต่อ แต่อาจมีค่าใช้จ่ายเพิ่ม",
        "{over} over what is left: this package buys extra messages, so sending goes on, but it may cost extra"
    ),
    // LINE Free: LINE refuses the rest; announcements do not fall back to email.
    (
        "announcements.review.quota_over_free",
        "เกินโควตาที่เหลือ {over} ข้อความ แพ็กเกจฟรีซื้อข้อความเพิ่มไม่ได้ LINE จะไม่ส่งส่วนที่เกิน บัญชีเหล่านั้นจะไม่ได้รับประกาศนี้ และระบบไม่ส่งทางอีเมลแทน",
        "{over} over what is left: the Free package cannot buy more, so LINE refuses the rest; those accounts do not get this announcement, and it is not sent by email instead"
    ),
    // LINE without a chosen package: it depends on the package.
    (
        "announcements.review.quota_over_unknown",
        "เกินโควตาที่เหลือ {over} ข้อความ ถ้าแพ็กเกจซื้อข้อความเพิ่มไม่ได้ LINE จะไม่ส่งส่วนที่เกิน และระบบไม่ส่งทางอีเมลแทน (เลือกแพ็กเกจได้ที่หน้าการใช้งานและโควตา)",
        "{over} over what is left: if the package cannot buy more, LINE refuses the rest and it is not sent by email instead (choose the package on the usage page)"
    ),
    // Any other Channel: the Quota only warns.
    (
        "announcements.review.quota_over_warn",
        "เกินโควตาที่เหลือ {over} ข้อความ โควตานี้มีไว้เตือนเท่านั้น ระบบยังส่งต่อ",
        "{over} over what is left: this Quota only warns, sending goes on"
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
    (
        "announcements.review.everyone_button_reach",
        "ส่งถึงทุกคน {n} คน (ติดต่อได้ {m})",
        "Send to everyone: {n} ({m} reachable)"
    ),
    ("announcements.review.back", "กลับไปแก้ไข", "Back to editing"),
    (
        "announcements.review.held_failed",
        "ยกเลิกกำหนดส่งแล้ว ประกาศนี้กลับเป็นฉบับร่าง แต่บันทึกการแก้ไขไม่สำเร็จ (อาจมีคนแก้ไขพร้อมกัน) กรุณาแก้ไขอีกครั้งแล้วตรวจก่อนส่ง",
        "The schedule was cancelled and this is a draft again, but the edit was not saved (someone may have edited it at the same time). Edit it again, then review it before sending."
    ),
    (
        "announcements.review.held",
        "ผู้รับเปลี่ยน จึงพักกำหนดส่งไว้ก่อน ตรวจแล้วกดยืนยันเพื่อตั้งเวลาส่งอีกครั้ง",
        "The Audience changed, so the schedule is on hold. Check it and confirm to schedule it again."
    ),
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

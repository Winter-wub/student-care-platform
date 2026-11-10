//! The Module's default texts, Thai and English, under `announcements.*`.
//! A Deployment overrides them with `[texts]` or on `/admin/texts`.

use crate::manifest::CatalogEntry as E;

/// `(key, thai, english)` -> two entries.
macro_rules! th_en {
    ($( ($k:expr, $th:expr, $en:expr) ),* $(,)?) => {
        &[ $( E::new($k, "th", $th), E::new($k, "en", $en), )* ]
    };
}

/// Chat: Menu entry, Card buttons, ack replies.
pub(crate) const CHAT: &[E] = th_en![
    ("announcements.menu.latest", "ข่าวล่าสุด", "Latest news"),
    ("announcements.card.ack", "รับทราบ", "Acknowledge"),
    ("announcements.card.open", "เปิดดูรายละเอียด", "Open details"),
    ("announcements.card.all", "ดูทั้งหมด", "See all"),
    (
        "announcements.ack.thanks",
        "บันทึกว่ารับทราบแล้ว ขอบคุณครับ",
        "Acknowledged. Thank you."
    ),
    (
        "announcements.ack.again",
        "รับทราบแล้ว",
        "Already acknowledged."
    ),
    (
        "announcements.ack.retracted",
        "ข่าวนี้ถูกถอนแล้ว",
        "This announcement was withdrawn."
    ),
    (
        "announcements.ack.expired",
        "ข่าวนี้ไม่เปิดให้ดูแล้ว",
        "This announcement is no longer available."
    ),
    (
        "announcements.notice.title",
        "ข่าวนี้มีการแก้ไข กดดูฉบับล่าสุดได้เลย",
        "This announcement was edited. Open it for the latest version."
    ),
    (
        "announcements.ack.unknown",
        "ไม่พบข่าวนี้",
        "This announcement could not be found."
    ),
    (
        "announcements.latest.empty",
        "ยังไม่มีข่าวสำหรับคุณ",
        "There is no news for you yet."
    ),
];

/// Back-office pages.
pub(crate) const PAGES: &[E] = th_en![
    ("announcements.nav", "ประกาศข่าว", "Announcements"),
    ("announcements.status.draft", "ฉบับร่าง", "Draft"),
    ("announcements.status.scheduled", "ตั้งเวลาไว้", "Scheduled"),
    ("announcements.status.sending", "กำลังส่ง", "Sending"),
    ("announcements.status.sent", "ส่งแล้ว", "Sent"),
    ("announcements.status.retracted", "ถอนแล้ว", "Withdrawn"),
    ("announcements.delivery.pending", "รอส่ง", "Pending"),
    ("announcements.delivery.sent", "ส่งแล้ว", "Sent"),
    ("announcements.delivery.failed", "ส่งไม่สำเร็จ", "Failed"),
    ("announcements.delivery.skipped", "ข้าม", "Skipped"),
    ("announcements.page.new", "สร้างประกาศ", "New announcement"),
    ("announcements.page.edit", "แก้ไขประกาศ", "Edit announcement"),
    (
        "announcements.page.not_found",
        "ไม่พบประกาศนี้",
        "Announcement not found."
    ),
    (
        "announcements.page.back",
        "กลับไปหน้ารายการ",
        "Back to the list"
    ),
    ("announcements.page.cancel", "ยกเลิก", "Cancel"),
    ("announcements.page.status", "สถานะ", "Status"),
    // Deprecated (#127): unused, kept so `[texts]` overrides still build.
    (
        "announcements.page.none",
        "ยังไม่มีประกาศ",
        "No announcements yet."
    ),
    ("announcements.page.time", "เวลา", "Time"),
    (
        "announcements.page.acked",
        "รับทราบ / ผู้รับ",
        "Acknowledged / in Audience"
    ),
    ("announcements.page.page", "หน้า", "Page"),
    ("announcements.page.send_at", "เวลาที่ตั้งส่ง", "Scheduled for"),
    ("announcements.page.sent_at", "ส่งเสร็จเมื่อ", "Sent at"),
    ("announcements.page.round", "รอบที่", "Round"),
    (
        "announcements.page.edited",
        "แก้ข้อความที่ส่งแล้ว",
        "Sent messages edited"
    ),
    ("announcements.page.edit_failed", "ไม่สำเร็จ", "failed"),
    (
        "announcements.page.notice",
        "แจ้งการแก้ไข (ช่องทางที่แก้ข้อความไม่ได้)",
        "Edit notice (Channels that cannot edit)"
    ),
    ("announcements.page.expired", "หมดอายุแล้ว", "Expired"),
    (
        "announcements.page.in_progress",
        "กำลังดำเนินการ",
        "in progress"
    ),
    ("announcements.page.yes", "ใช่", "Yes"),
    ("announcements.page.no", "ไม่", "No"),
    ("announcements.page.channel", "ช่องทาง", "Channel"),
    ("announcements.page.deliveries", "การส่งรายคน", "Deliveries"),
    ("announcements.page.all", "ทั้งหมด", "All"),
    ("announcements.page.student", "นักศึกษา", "Student"),
    ("announcements.page.confirm", "ยืนยัน", "Confirm"),
    (
        "announcements.page.resend_confirm",
        "ส่งประกาศนี้อีกรอบให้ทุกคนในกลุ่มผู้รับตอนนี้? รอบใหม่นับโควตา push ด้วย",
        "Send this announcement again to everyone now in its Audience? The new round counts against the push Quota."
    ),
    (
        "announcements.page.retract_confirm",
        "ถอนประกาศนี้? จะหายจากเมนูและ Mini App และลบข้อความในช่องทางที่ลบได้ โดยไม่แจ้งนักศึกษา",
        "Withdraw this announcement? It disappears from the Menu and the Mini App and is deleted where the Channel allows, without notice."
    ),
    ("announcements.action.edit", "แก้ไข", "Edit"),
    (
        "announcements.action.confirm",
        "ตรวจก่อนส่ง",
        "Review before sending"
    ),
    (
        "announcements.action.cancel",
        "ยกเลิกการตั้งเวลา",
        "Cancel the schedule"
    ),
    ("announcements.action.resend", "ส่งซ้ำ", "Send again"),
    ("announcements.action.retract", "ถอนประกาศ", "Withdraw"),
    ("announcements.flash.saved", "บันทึกฉบับร่างแล้ว", "Draft saved."),
    ("announcements.flash.scheduled", "ตั้งเวลาส่งแล้ว", "Scheduled."),
    (
        "announcements.flash.sending",
        "เริ่มส่งแล้ว",
        "Sending has started."
    ),
    (
        "announcements.flash.updated",
        "บันทึกการแก้ไขแล้ว",
        "Changes saved."
    ),
    (
        "announcements.flash.not_editable",
        "ประกาศนี้แก้ไขไม่ได้ในสถานะนี้",
        "This announcement cannot be changed now."
    ),
    (
        "announcements.flash.expired",
        "ข่าวนี้หมดอายุแล้ว ขยายวันหมดอายุก่อนจึงส่งซ้ำได้",
        "This announcement has expired. Extend its expiry before resending."
    ),
    (
        "announcements.flash.conflict",
        "มีคนแก้ประกาศนี้ไปก่อนแล้ว กรุณาลองใหม่",
        "Someone changed this announcement meanwhile. Please try again."
    ),
    (
        "announcements.flash.cancelled",
        "ยกเลิกการตั้งเวลาแล้ว กลับเป็นฉบับร่าง",
        "Schedule cancelled; it is a draft again."
    ),
    (
        "announcements.flash.resending",
        "เริ่มส่งซ้ำแล้ว",
        "Sending again."
    ),
    (
        "announcements.flash.retracted",
        "ถอนประกาศแล้ว",
        "Announcement withdrawn."
    ),
];

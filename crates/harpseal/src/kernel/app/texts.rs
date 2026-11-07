//! The Mini App's texts (`kernel.app.*`), Thai and English. Pages read
//! them through `Texts::load`, so `[texts]` and the back office's Texts
//! page change them without a deploy. Placeholders are listed per key.

use crate::manifest::CatalogEntry as E;

/// `(key, thai, english)` -> two entries.
macro_rules! th_en {
    ($( ($k:expr, $th:expr, $en:expr) ),* $(,)?) => {
        &[ $( E::new($k, "th", $th), E::new($k, "en", $en), )* ]
    };
}

pub(crate) const DEFAULTS: &[E] = th_en![
    ("kernel.app.title", "หน้าของฉัน", "My page"),
    (
        "kernel.app.opening",
        "กำลังเปิดหน้าของคุณ…",
        "Opening your page…"
    ),
    (
        "kernel.app.open_in_chat",
        "เปิดหน้านี้ไม่ได้ กรุณาปิดแล้วเปิดใหม่จากปุ่มในแชท",
        "This page could not open. Please close it and open it again from a button in the chat."
    ),
    (
        "kernel.app.refused",
        "ยืนยันบัญชีแชทของคุณไม่สำเร็จ กรุณาลองอีกครั้ง หรือปิดแล้วเปิดใหม่จากแชท",
        "We could not confirm your chat account. Please try again, or close this page and open it again from the chat."
    ),
    ("kernel.app.retry", "ลองอีกครั้ง", "Try again"),
    (
        "kernel.app.slow_down",
        "เปิดถี่เกินไป กรุณารอสักครู่แล้วลองอีกครั้ง",
        "Too many attempts. Please wait a moment and try again."
    ),
    (
        "kernel.app.error",
        "ระบบขัดข้องชั่วคราว กรุณาลองใหม่ภายหลัง",
        "Something went wrong. Please try again later."
    ),
    (
        "kernel.app.forbidden",
        "คำขอนี้ใช้ไม่ได้ กรุณาเปิดหน้านี้ใหม่จากแชท",
        "This request is not valid. Please open the page again from the chat."
    ),
    ("kernel.app.not_found", "ไม่พบหน้านี้", "Page not found."),
    ("kernel.app.back", "กลับหน้าของฉัน", "Back to my page"),
    (
        "kernel.app.not_linked",
        "บัญชีนี้ยังไม่ได้เชื่อมกับรหัสนักศึกษา กลับไปที่แชทแล้วกด เริ่มใช้งาน",
        "This account is not linked to a student ID yet. Go back to the chat and tap Get started."
    ),
    // {code}
    (
        "kernel.app.student_code",
        "รหัสนักศึกษา {code}",
        "Student ID {code}"
    ),
    // {channel}
    (
        "kernel.app.via",
        "เชื่อมต่อผ่าน {channel}",
        "Linked through {channel}"
    ),
    // {version}
    (
        "kernel.app.consent",
        "ยอมรับประกาศความเป็นส่วนตัวฉบับ {version} แล้ว",
        "You accepted privacy notice version {version}."
    ),
    (
        "kernel.app.consent_pending",
        "มีประกาศความเป็นส่วนตัวฉบับใหม่ กรุณายอมรับในแชท",
        "There is a new privacy notice. Please accept it in the chat."
    ),
    (
        "kernel.app.consent_none",
        "ยังไม่ได้ยอมรับประกาศความเป็นส่วนตัว",
        "You have not accepted the privacy notice yet."
    ),
    ("kernel.app.news", "ข่าวของฉัน", "My announcements"),
    // The label of the Modules' links on the home (#72), for screen readers.
    ("kernel.app.links", "บริการอื่น", "More services"),
    (
        "kernel.app.news_empty",
        "ยังไม่มีข่าวสำหรับคุณ",
        "No announcements for you yet."
    ),
    ("kernel.app.read_more", "อ่านต่อ", "Read more"),
    ("kernel.app.ack", "รับทราบ", "Acknowledge"),
    ("kernel.app.acked", "รับทราบแล้ว", "Acknowledged"),
    ("kernel.app.waiting", "รอการรับทราบ", "Please acknowledge"),
    (
        "kernel.app.retracted",
        "ข่าวนี้ถูกถอนแล้ว",
        "This announcement was withdrawn."
    ),
    (
        "kernel.app.unavailable",
        "ข่าวนี้ไม่เปิดให้ดูแล้ว",
        "This announcement is no longer available."
    ),
    // {version}
    (
        "kernel.app.notice_version",
        "ฉบับที่ {version}",
        "Version {version}"
    ),
    (
        "kernel.app.outside",
        "หน้านี้สำหรับนักศึกษา กรุณาเปิดจากปุ่มในแชท LINE หรือ Telegram",
        "This page is for students. Please open it from a button in the LINE or Telegram chat."
    ),
    (
        "kernel.app.staff_login",
        "เจ้าหน้าที่ เข้าสู่ระบบ",
        "Staff sign-in"
    ),
    (
        "kernel.app.line_login",
        "เข้าสู่ระบบด้วย LINE",
        "Log in with LINE"
    ),
    ("kernel.app.sign_out", "ออกจากระบบ", "Sign out"),
    (
        "kernel.app.signed_out",
        "ออกจากระบบแล้ว ปิดหน้านี้ได้เลย ครั้งหน้าเปิดจากปุ่มในแชทอีกครั้ง",
        "You have signed out. You can close this page; next time, open it again from a button in the chat."
    ),
    (
        "kernel.app.notice_link",
        "ประกาศความเป็นส่วนตัว",
        "Privacy notice"
    ),
];

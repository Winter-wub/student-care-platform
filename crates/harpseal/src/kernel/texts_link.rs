//! Default texts of the Link and OTP steps, in Thai and English.
//! Placeholders are `{name}`; each key's comment lists its own.

use crate::manifest::CatalogEntry as E;

macro_rules! th_en {
    ($( ($k:expr, $th:expr, $en:expr) ),* $(,)?) => {
        &[ $( E::new($k, "th", $th), E::new($k, "en", $en), )* ]
    };
}

pub(crate) const DEFAULTS: &[E] = th_en![
    (
        "kernel.link.ask_student_id",
        "ขอบคุณครับ กรุณาพิมพ์รหัสนักศึกษาของคุณ",
        "Thank you. Please type your student ID."
    ),
    (
        "kernel.link.bad_format",
        "รูปแบบรหัสนักศึกษาไม่ถูกต้อง กรุณาตรวจสอบแล้วพิมพ์ใหม่",
        "That does not look like a student ID. Please check it and type it again."
    ),
    // Not on the roster and left share this text: no enumeration.
    (
        "kernel.link.not_found",
        "ไม่พบรหัสนักศึกษานี้ในทะเบียน กรุณาตรวจสอบ หรือติดต่อเจ้าหน้าที่",
        "This student ID is not on the roster. Please check it or contact Staff."
    ),
    (
        "kernel.link.no_email",
        "ทะเบียนไม่มีอีเมลของรหัสนี้ จึงส่งรหัสยืนยันไม่ได้ กรุณาติดต่อเจ้าหน้าที่เพื่อเชื่อมบัญชีให้",
        "The roster has no email for this ID, so no code can be sent. Please contact Staff to link your account."
    ),
    (
        "kernel.link.already_linked",
        "รหัสนักศึกษานี้เชื่อมกับบัญชีอื่นในช่องทางนี้อยู่แล้ว หากไม่ใช่คุณ กรุณาติดต่อเจ้าหน้าที่",
        "This student ID is already linked to another account on this channel. If that was not you, please contact Staff."
    ),
    // Pushed to the account that already holds the Link.
    (
        "kernel.link.attempt_warning",
        "มีผู้พยายามเชื่อมรหัสนักศึกษาของคุณจากบัญชีอื่น หากไม่ใช่คุณ กรุณาแจ้งเจ้าหน้าที่",
        "Someone tried to link your student ID from another account. If that was not you, please tell Staff."
    ),
    ("kernel.link.success_title", "เชื่อมต่อสำเร็จ", "Linked"),
    // {name}
    (
        "kernel.link.success",
        "สวัสดี {name} บัญชีของคุณเชื่อมเรียบร้อยแล้ว",
        "Hello {name}, your account is now linked."
    ),
    (
        "kernel.link.unlink_confirm",
        "ยืนยันยกเลิกการเชื่อมบัญชีนี้หรือไม่ หลังยกเลิกจะไม่ได้รับข่าวทางช่องทางนี้",
        "Unlink this account? You will stop receiving news on this channel."
    ),
    ("kernel.link.unlink_yes", "ยืนยัน", "Unlink"),
    ("kernel.link.unlink_no", "ไม่", "Keep"),
    (
        "kernel.link.revoked_bye",
        "ยกเลิกการเชื่อมบัญชีแล้ว หากต้องการเชื่อมใหม่ พิมพ์รหัสนักศึกษาได้ทุกเมื่อ",
        "Your account is unlinked. Type your student ID any time to link again."
    ),
    // {email} (masked), {minutes}
    (
        "kernel.otp.sent",
        "ส่งรหัสยืนยันไปที่ {email} แล้ว (ใช้ได้ {minutes} นาที) พิมพ์รหัสที่ได้รับในแชทนี้",
        "We sent a code to {email} (valid for {minutes} minutes). Type it here."
    ),
    // {digits}
    (
        "kernel.otp.prompt",
        "กรุณาพิมพ์รหัสยืนยันเป็นตัวเลข {digits} หลัก",
        "Please type the {digits}-digit code."
    ),
    // {left}
    (
        "kernel.otp.wrong",
        "รหัสไม่ถูกต้อง เหลือโอกาสอีก {left} ครั้ง",
        "Wrong code. {left} attempts left."
    ),
    // {minutes}
    (
        "kernel.otp.locked",
        "ใส่รหัสผิดเกินกำหนด ระงับการยืนยันตัวตนไว้ชั่วคราว ลองใหม่ได้ในอีก {minutes} นาที",
        "Too many wrong codes. Verification is paused; try again in {minutes} minutes."
    ),
    // Pushed to the Student's other active Links.
    (
        "kernel.otp.lockout_notice",
        "มีการใส่รหัสยืนยันของคุณผิดหลายครั้งจากบัญชีอื่น หากไม่ใช่คุณ กรุณาแจ้งเจ้าหน้าที่",
        "Someone typed wrong codes for your student ID from another account. If that was not you, please tell Staff."
    ),
    (
        "kernel.otp.expired",
        "รหัสหมดอายุแล้ว กดส่งรหัสใหม่ได้เลย",
        "The code has expired. Press resend for a new one."
    ),
    // {minutes}
    (
        "kernel.otp.resend_limit",
        "ขอรหัสครบจำนวนที่กำหนดแล้ว กรุณารออีก {minutes} นาที",
        "You have asked for the most codes allowed. Please wait {minutes} minutes."
    ),
    (
        "kernel.otp.send_failed",
        "ส่งอีเมลรหัสยืนยันไม่สำเร็จ กรุณาลองใหม่ภายหลัง",
        "We could not send the code email. Please try again later."
    ),
    ("kernel.otp.resend", "ส่งรหัสใหม่", "Resend code"),
    ("kernel.otp.cancel", "ยกเลิก", "Cancel"),
    // Email. {code}, {minutes}, {name}
    (
        "kernel.otp.email_subject",
        "รหัสยืนยันตัวตน {code}",
        "Your verification code {code}"
    ),
    (
        "kernel.otp.email_body",
        "สวัสดี {name}\n\nรหัสยืนยันตัวตนของคุณคือ {code} ใช้ได้ {minutes} นาที\n\
         หากคุณไม่ได้ขอรหัสนี้ ไม่ต้องทำอะไร และอย่าบอกรหัสนี้กับผู้อื่น",
        "Hello {name},\n\nYour verification code is {code}. It is valid for {minutes} minutes.\n\
         If you did not ask for it, ignore this email and never share the code."
    ),
    // Pushed when Staff revoke a Link.
    (
        "kernel.link.revoked_by_staff",
        "เจ้าหน้าที่ยกเลิกการเชื่อมบัญชีนี้แล้ว หากต้องการเชื่อมใหม่ พิมพ์รหัสนักศึกษาได้ทุกเมื่อ",
        "Staff have unlinked this account. To link it again, type your student ID at any time."
    ),
    // Pushed when Staff create a Manual Link. {name}
    (
        "kernel.link.manual_notice",
        "เจ้าหน้าที่เชื่อมบัญชีนี้กับ {name} ให้แล้ว หากไม่ใช่คุณ กรุณาติดต่อเจ้าหน้าที่",
        "Staff have linked this account to {name}. If this is not you, please contact Staff."
    ),
];

//! The Kernel's default texts, each in Thai and English. A Deployment
//! overrides them with `[texts]` in its TOML, then with `setting` rows.
//! Placeholders are `{name}`; each key's comment lists its own.

use crate::manifest::CatalogEntry as E;

/// `(key, thai, english)` -> two entries.
macro_rules! th_en {
    ($( ($k:expr, $th:expr, $en:expr) ),* $(,)?) => {
        &[ $( E::new($k, "th", $th), E::new($k, "en", $en), )* ]
    };
}

/// Pages, generic answers and the Consent Card.
pub(crate) const DEFAULTS: &[E] = th_en![
    (
        "kernel.link.required",
        "กรุณายืนยันตัวตนก่อนใช้งาน",
        "Please verify your identity first."
    ),
    (
        "kernel.action.expired",
        "ปุ่มนี้ใช้ไม่ได้แล้ว กรุณาเลือกจากเมนูอีกครั้ง",
        "This button no longer works. Please pick from the Menu again."
    ),
    (
        "kernel.notice.title",
        "ประกาศความเป็นส่วนตัว",
        "Privacy notice"
    ),
    (
        "kernel.notice.body",
        "เราเก็บรหัสนักศึกษา ชื่อ อีเมล และบัญชีแชทของคุณเพื่อส่งข่าวและบริการนักศึกษาเท่านั้น\n\n\
         คุณขอดู แก้ไข หรือยกเลิกการเชื่อมบัญชีได้ทุกเมื่อโดยติดต่อเจ้าหน้าที่",
        "We keep your student ID, name, email and chat account only to send you news and student services.\n\n\
         You can ask to see, correct or unlink your data at any time by contacting Staff."
    ),
    (
        "kernel.backoffice.sign_in",
        "กรุณาเข้าสู่ระบบเจ้าหน้าที่",
        "Please sign in as Staff."
    ),
    (
        "kernel.flow.welcome",
        "สวัสดีครับ ยินดีต้อนรับสู่ช่องทางบริการนักศึกษา ก่อนเริ่มใช้งาน กรุณาอ่านและยอมรับประกาศความเป็นส่วนตัวด้านล่าง",
        "Hello and welcome to student services. Before we start, please read and accept the privacy notice below."
    ),
    (
        "kernel.flow.welcome_back",
        "ยินดีต้อนรับกลับมา บัญชีของคุณยังเชื่อมอยู่ ใช้เมนูได้ตามปกติ",
        "Welcome back. Your account is still linked; the Menu works as before."
    ),
    // Consent Card. `kernel.consent.notice` is the short summary on the Card.
    (
        "kernel.consent.title",
        "ประกาศความเป็นส่วนตัว",
        "Privacy notice"
    ),
    (
        "kernel.consent.updated",
        "ประกาศความเป็นส่วนตัวฉบับใหม่",
        "Updated privacy notice"
    ),
    (
        "kernel.consent.notice",
        "เราใช้รหัสนักศึกษา ชื่อ อีเมล และบัญชีแชทของคุณเพื่อยืนยันตัวตน ส่งข่าว และให้บริการนักศึกษาเท่านั้น",
        "We use your student ID, name, email and chat account only to verify you, send news and provide student services."
    ),
    ("kernel.consent.accept", "ยอมรับ", "Accept"),
    ("kernel.consent.read_full", "อ่านฉบับเต็ม", "Read in full"),
    // Shown instead of the Mini App button on a Channel without a Mini App.
    (
        "kernel.consent.read_full_hint",
        "อ่านฉบับเต็มได้ที่หน้า /notice บนเว็บไซต์ของหน่วยงาน หรือขอจากเจ้าหน้าที่",
        "Read the full notice at /notice on the office's website, or ask Staff."
    ),
    // Menu: the Choices listing it, and the Kernel's own entries.
    (
        "kernel.menu.pick",
        "เลือกบริการจากเมนูได้เลย",
        "Pick a service from the Menu."
    ),
    ("kernel.menu.unlink", "ยกเลิกการเชื่อมบัญชี", "Unlink my account"),
    ("kernel.menu.start", "เริ่มใช้งาน", "Start"),
    (
        "kernel.inbound.fallback",
        "ขออภัย ระบบยังตอบข้อความพิมพ์เองไม่ได้ เราเก็บข้อความไว้ให้เจ้าหน้าที่แล้ว กรุณาเลือกจากเมนู",
        "Sorry, typed messages are not answered here yet. We kept yours for Staff; please pick from the Menu."
    ),
    // {mb}
    (
        "kernel.inbound.file_too_large",
        "ไฟล์ใหญ่เกิน {mb} MB จึงไม่ได้บันทึกไว้",
        "The file is larger than {mb} MB, so it was not kept."
    ),
];

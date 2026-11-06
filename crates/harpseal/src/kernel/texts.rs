//! The Kernel's default texts (Thai and English). A Deployment overrides
//! them with `[texts]` in its TOML, then with `setting` rows.

use crate::manifest::CatalogEntry as E;

pub(crate) const DEFAULTS: &[E] = &[
    E::new("kernel.link.required", "th", "กรุณายืนยันตัวตนก่อนใช้งาน"),
    E::new(
        "kernel.link.required",
        "en",
        "Please verify your identity first.",
    ),
    E::new(
        "kernel.action.expired",
        "th",
        "ปุ่มนี้ใช้ไม่ได้แล้ว กรุณาเลือกจากเมนูอีกครั้ง",
    ),
    E::new(
        "kernel.action.expired",
        "en",
        "This button no longer works. Please pick from the Menu again.",
    ),
    E::new("kernel.notice.title", "th", "ประกาศความเป็นส่วนตัว"),
    E::new("kernel.notice.title", "en", "Privacy notice"),
    E::new(
        "kernel.notice.body",
        "th",
        "เราเก็บรหัสนักศึกษา ชื่อ อีเมล และบัญชีแชทของคุณเพื่อส่งข่าวและบริการนักศึกษาเท่านั้น\n\n\
         คุณขอดู แก้ไข หรือยกเลิกการเชื่อมบัญชีได้ทุกเมื่อโดยติดต่อเจ้าหน้าที่",
    ),
    E::new(
        "kernel.notice.body",
        "en",
        "We keep your student ID, name, email and chat account only to send you news and student services.\n\n\
         You can ask to see, correct or unlink your data at any time by contacting Staff.",
    ),
    E::new("kernel.backoffice.sign_in", "th", "กรุณาเข้าสู่ระบบเจ้าหน้าที่"),
    E::new(
        "kernel.backoffice.sign_in",
        "en",
        "Please sign in as Staff.",
    ),
];

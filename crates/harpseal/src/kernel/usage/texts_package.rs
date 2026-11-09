//! Texts of the LINE package block on the usage page (#115), Thai and
//! English. Placeholders: `{name}` (package), `{limit}` (messages per
//! month), `{url}`, `{date}`.

use crate::manifest::CatalogEntry as E;

macro_rules! th_en {
    ($( ($k:expr, $th:expr, $en:expr) ),* $(,)?) => {
        &[ $( E::new($k, "th", $th), E::new($k, "en", $en), )* ]
    };
}

pub(crate) const DEFAULTS: &[E] = th_en![
    (
        "kernel.usage.package.title",
        "แพ็กเกจ LINE Official Account",
        "LINE Official Account package"
    ),
    // {name} {limit}
    (
        "kernel.usage.package.current",
        "แพ็กเกจ {name}: {limit} ข้อความต่อเดือน",
        "{name} package: {limit} messages a month"
    ),
    // {limit}
    (
        "kernel.usage.package.current_custom",
        "กำหนดเอง: {limit} ข้อความต่อเดือน",
        "Custom: {limit} messages a month"
    ),
    // {limit}
    (
        "kernel.usage.package.current_config",
        "ยังไม่ได้เลือกแพ็กเกจ ใช้ค่าจากไฟล์ตั้งค่า: {limit} ข้อความต่อเดือน",
        "No package chosen yet, using the configured {limit} messages a month"
    ),
    (
        "kernel.usage.package.current_none",
        "ยังไม่ได้เลือกแพ็กเกจ และไม่ได้ตั้งโควตาในไฟล์ตั้งค่า",
        "No package chosen and no Quota in the configuration"
    ),
    (
        "kernel.usage.package.extra_yes",
        "ซื้อข้อความเพิ่มได้",
        "extra messages can be bought"
    ),
    (
        "kernel.usage.package.extra_no",
        "ซื้อข้อความเพิ่มไม่ได้",
        "extra messages cannot be bought"
    ),
    (
        "kernel.usage.package.unreadable",
        "ค่าที่บันทึกไว้อ่านไม่ได้ จึงใช้ค่าจากไฟล์ตั้งค่าอยู่ เลือกแพ็กเกจใหม่เพื่อแก้",
        "The saved choice cannot be read, so the configured value applies. Choose a package again to fix it."
    ),
    (
        "kernel.usage.package.help",
        "โควตาเดือนนี้เปลี่ยนตามแพ็กเกจที่เลือกทันที ระบบไม่ได้ถาม LINE ว่าใช้แพ็กเกจไหน \
         จึงต้องเลือกให้ตรงกับที่สมัครไว้",
        "This month's Quota follows the package at once. The system does not ask LINE which \
         package is in use, so choose the one you subscribed to."
    ),
    // {url} {date}
    (
        "kernel.usage.package.source",
        "ข้อมูลแพ็กเกจจาก {url} (อ่านเมื่อ {date})",
        "Package figures from {url} (read on {date})"
    ),
    ("kernel.usage.package.label", "แพ็กเกจ", "Package"),
    // {name} {limit}
    (
        "kernel.usage.package.option",
        "{name} ({limit} ข้อความ)",
        "{name} ({limit} messages)"
    ),
    ("kernel.usage.package.option_custom", "กำหนดเอง", "Custom"),
    (
        "kernel.usage.package.option_config",
        "ใช้ค่าจากไฟล์ตั้งค่า",
        "Use the configured value"
    ),
    (
        "kernel.usage.package.reset_done",
        "เลิกใช้แพ็กเกจที่เลือก กลับไปใช้ค่าจากไฟล์ตั้งค่าแล้ว",
        "The chosen package is removed, the configured value applies again"
    ),
    (
        "kernel.usage.package.label_custom",
        "จำนวนข้อความต่อเดือน (ใช้เมื่อเลือกกำหนดเอง)",
        "Messages a month (used with Custom)"
    ),
    ("kernel.usage.package.save", "บันทึกแพ็กเกจ", "Save package"),
    (
        "kernel.usage.package.saved",
        "บันทึกแพ็กเกจแล้ว",
        "Package saved"
    ),
    (
        "kernel.usage.package.same",
        "ตรงกับที่ใช้อยู่แล้ว ไม่มีอะไรเปลี่ยน",
        "That is what is in force already, nothing changed"
    ),
    (
        "kernel.usage.package.err_unknown",
        "ไม่รู้จักแพ็กเกจที่เลือก",
        "That package is not known"
    ),
    (
        "kernel.usage.package.err_custom",
        "กรอกจำนวนข้อความเป็นเลขจำนวนเต็ม ตั้งแต่ 1 ถึง 100000000",
        "Enter a whole number of messages from 1 to 100000000"
    ),
    (
        "kernel.usage.package.err_no_line",
        "ระบบนี้ไม่ได้ใช้ช่องทาง LINE",
        "This Deployment has no LINE Channel"
    ),
];

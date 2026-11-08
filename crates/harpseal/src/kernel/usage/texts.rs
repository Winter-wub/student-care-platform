//! Texts of the usage page, the Quota banner and the Quota emails (#26),
//! Thai and English. Placeholders: `{channel}` (`LINE`), `{percent}`,
//! `{used}`, `{cap}`, `{month}` (`YYYY-MM`), `{org}`, `{path}`.

use crate::manifest::CatalogEntry as E;

macro_rules! th_en {
    ($( ($k:expr, $th:expr, $en:expr) ),* $(,)?) => {
        &[ $( E::new($k, "th", $th), E::new($k, "en", $en), )* ]
    };
}

pub(crate) const DEFAULTS: &[E] = th_en![
    ("kernel.usage.title", "การใช้งานและโควตา", "Usage and Quotas"),
    (
        "kernel.usage.intro",
        "นับจาก Push ที่คิดค่าบริการ (หนึ่งครั้งต่อผู้รับต่อคำขอ) ตามวันในเขตเวลาของระบบ ระบบไม่หยุดส่งข้อความเมื่อถึงโควตา \
         ช่อง \"ส่งไม่สำเร็จ\" นับครั้งที่ส่งไปยังแพลตฟอร์มแล้วล้มเหลว ข้อความที่ล้มเหลวสองครั้งแล้วส่งได้ในครั้งที่สามจึงนับ 2",
        "Counts metered Pushes (one per request per recipient) by local day. Sending never stops at the Quota. \
         \"Failed sends\" counts failed attempts to the platform: a Message that fails twice and goes out the third time counts 2."
    ),
    // {month}
    (
        "kernel.usage.this_month",
        "เดือนนี้ ({month})",
        "This month ({month})"
    ),
    // {used} {cap} {percent}
    (
        "kernel.usage.used_of",
        "ใช้ไป {used} จาก {cap} ({percent}%)",
        "{used} of {cap} used ({percent}%)"
    ),
    // {used}
    (
        "kernel.usage.no_quota",
        "ไม่ได้ตั้งโควตา: ใช้ไป {used}",
        "No Quota set: {used} used"
    ),
    ("kernel.usage.metered", "Push ที่คิดค่าบริการ", "Metered pushes"),
    ("kernel.usage.free", "ข้อความตอบกลับ (ฟรี)", "Free replies"),
    ("kernel.usage.failed", "ส่งไม่สำเร็จ", "Failed sends"),
    ("kernel.usage.level_ok", "ปกติ", "Normal"),
    ("kernel.usage.level_warn", "ใกล้เต็มโควตา", "Near the Quota"),
    ("kernel.usage.level_over", "เกินโควตา", "Over the Quota"),
    ("kernel.usage.chart", "30 วันล่าสุด", "Last 30 days"),
    ("kernel.usage.chart_table", "ดูเป็นตาราง", "Show as a table"),
    ("kernel.usage.day", "วันที่", "Day"),
    ("kernel.usage.months", "6 เดือนล่าสุด", "Last 6 months"),
    ("kernel.usage.month", "เดือน", "Month"),
    // The technical details (#123).
    (
        "kernel.usage.tech_config",
        "ค่าจากไฟล์ตั้งค่า: [quotas.<ช่องทาง>] metered_per_month ใน deployment.toml (ใช้เมื่อยังไม่ได้เลือกแพ็กเกจ LINE)",
        "The configured value: [quotas.<channel>] metered_per_month in deployment.toml (used until a LINE package is chosen)"
    ),
    (
        "kernel.usage.tech_source",
        "ตัวเลขทั้งหมดอ่านจากตาราง usage_daily ไม่ได้อ่านจาก API ของแพลตฟอร์ม",
        "Every number is read from the usage_daily table, never from a platform API"
    ),
    // {channel} {percent}
    (
        "kernel.usage.banner_warn",
        "{channel} ใช้ไป {percent}% ของโควตาเดือนนี้",
        "{channel} has used {percent}% of this month's Quota"
    ),
    // {channel} {percent}
    (
        "kernel.usage.banner_over",
        "{channel} ใช้ไป {percent}% ของโควตาเดือนนี้ เกินโควตาแล้ว (ระบบยังส่งต่อ)",
        "{channel} has used {percent}% of this month's Quota: over it (sending goes on)"
    ),
    // {used} {cap}: the Announcements send page, beside its estimate.
    (
        "kernel.usage.send_used",
        "ใช้ไปแล้ว {used}/{cap} เดือนนี้",
        "{used}/{cap} used this month"
    ),
    // {used}
    (
        "kernel.usage.send_used_no_cap",
        "ใช้ไปแล้ว {used} เดือนนี้",
        "{used} used this month"
    ),
    // {org} {channel} {percent} {month}
    (
        "kernel.usage.email_subject_warn",
        "[{org}] {channel} ใช้ไป {percent}% ของโควตาเดือน {month}",
        "[{org}] {channel} has used {percent}% of the {month} Quota"
    ),
    // {org} {channel} {percent} {month}
    (
        "kernel.usage.email_subject_over",
        "[{org}] {channel} ถึงโควตาเดือน {month} แล้ว ({percent}%)",
        "[{org}] {channel} has reached the {month} Quota ({percent}%)"
    ),
    // {channel} {used} {cap} {percent} {month} {path}
    (
        "kernel.usage.email_body",
        "{channel} ใช้ Push ที่คิดค่าบริการไปแล้ว {used} จาก {cap} ครั้ง ({percent}%) ในเดือน {month}\n\n\
         ระบบยังส่งข้อความต่อตามปกติ หากต้องการเพิ่มโควตา กรุณาติดต่อผู้ให้บริการช่องทาง\n\n\
         เปิดหน้า Usage ในระบบหลังบ้าน ({path})",
        "{channel} has used {used} of {cap} metered pushes ({percent}%) in {month}.\n\n\
         Messages still go out. To raise the Quota, contact the Channel's provider.\n\n\
         Open the Usage page in the back office ({path})."
    ),
];

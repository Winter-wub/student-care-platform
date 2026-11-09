//! Default texts of the back-office shell's navigation and the term
//! status home (#123), Thai and English.

use crate::manifest::CatalogEntry as E;

pub(super) const DEFAULTS: &[E] = &[
    E::new("kernel.backoffice.skip", "th", "ข้ามไปยังเนื้อหา"),
    E::new("kernel.backoffice.skip", "en", "Skip to content"),
    E::new("kernel.backoffice.group_daily", "th", "งานประจำ"),
    E::new("kernel.backoffice.group_daily", "en", "Daily work"),
    E::new("kernel.backoffice.group_admin", "th", "ผู้ดูแลระบบ"),
    E::new("kernel.backoffice.group_admin", "en", "Administration"),
    E::new("kernel.backoffice.technical", "th", "ข้อมูลทางเทคนิค"),
    E::new("kernel.backoffice.technical", "en", "Technical details"),
    // Locale names (#125): `kernel.locale.<tag>`; a Deployment with another
    // Locale names it in `[texts]` (see `Catalog::locale_name`).
    E::new("kernel.locale.th", "th", "ไทย"),
    E::new("kernel.locale.th", "en", "Thai"),
    E::new("kernel.locale.en", "th", "อังกฤษ"),
    E::new("kernel.locale.en", "en", "English"),
    E::new("kernel.home.title", "th", "ภาพรวมภาคเรียน"),
    E::new("kernel.home.title", "en", "This term at a glance"),
    E::new("kernel.home.intro", "th", "สถานะงานที่คุณดูแล และสิ่งที่ควรทำต่อ"),
    E::new(
        "kernel.home.intro",
        "en",
        "Where your work stands, and what to do next.",
    ),
    E::new("kernel.home.state_empty", "th", "ยังไม่เริ่ม"),
    E::new("kernel.home.state_empty", "en", "Not started"),
    E::new("kernel.home.state_ok", "th", "เรียบร้อย"),
    E::new("kernel.home.state_ok", "en", "In order"),
    E::new("kernel.home.state_warn", "th", "ต้องดู"),
    E::new("kernel.home.state_warn", "en", "Needs attention"),
    E::new(
        "kernel.home.unreadable",
        "th",
        "อ่านข้อมูลส่วนนี้ไม่ได้ กรุณาโหลดหน้าใหม่",
    ),
    E::new(
        "kernel.home.unreadable",
        "en",
        "Could not read this. Please reload the page.",
    ),
    E::new("kernel.home.none", "th", "ยังไม่มีงานให้ติดตามที่นี่ เลือกงานจากเมนู"),
    E::new(
        "kernel.home.none",
        "en",
        "Nothing to follow here yet. Pick a page from the menu.",
    ),
    // Roster card.
    E::new("kernel.home.roster.title", "th", "รายชื่อนักศึกษา"),
    E::new("kernel.home.roster.title", "en", "Student roster"),
    E::new("kernel.home.roster.empty", "th", "ยังไม่ได้นำเข้ารายชื่อ"),
    E::new("kernel.home.roster.empty", "en", "No roster imported yet."),
    E::new("kernel.home.roster.start", "th", "นำเข้ารายชื่อ"),
    E::new("kernel.home.roster.start", "en", "Import the roster"),
    // {time} {rows}
    E::new(
        "kernel.home.roster.done",
        "th",
        "นำเข้าล่าสุด {time} · {rows} แถว",
    ),
    E::new(
        "kernel.home.roster.done",
        "en",
        "Last import {time} · {rows} rows",
    ),
    E::new("kernel.home.roster.running", "th", "กำลังนำเข้าไฟล์ {file}"),
    E::new("kernel.home.roster.running", "en", "Importing {file}"),
    E::new(
        "kernel.home.roster.waiting",
        "th",
        "ไฟล์ {file} รอยืนยันการนำเข้า",
    ),
    E::new(
        "kernel.home.roster.waiting",
        "en",
        "{file} is waiting to be confirmed.",
    ),
    E::new(
        "kernel.home.roster.failed",
        "th",
        "นำเข้าไฟล์ {file} ไม่สำเร็จ ({time})",
    ),
    E::new(
        "kernel.home.roster.failed",
        "en",
        "Importing {file} failed ({time}).",
    ),
    E::new("kernel.home.roster.open", "th", "ดูการนำเข้า"),
    E::new("kernel.home.roster.open", "en", "Open the import"),
    E::new("kernel.home.roster.new", "th", "นำเข้าไฟล์ใหม่"),
    E::new("kernel.home.roster.new", "en", "Import a new file"),
    // Quota card.
    E::new("kernel.home.quota.title", "th", "โควตาข้อความ"),
    E::new("kernel.home.quota.title", "en", "Message quota"),
    E::new(
        "kernel.home.quota.empty",
        "th",
        "ยังไม่ได้กำหนดโควตาข้อความต่อเดือน",
    ),
    E::new(
        "kernel.home.quota.empty",
        "en",
        "No monthly message quota set yet.",
    ),
    E::new("kernel.home.quota.set", "th", "เลือกแพ็กเกจ"),
    E::new("kernel.home.quota.set", "en", "Choose the package"),
    // {channel} {used} {cap} {percent}
    E::new(
        "kernel.home.quota.used",
        "th",
        "{channel}: เดือนนี้ใช้ไป {used} จาก {cap} ข้อความ ({percent}%)",
    ),
    E::new(
        "kernel.home.quota.used",
        "en",
        "{channel}: {used} of {cap} messages this month ({percent}%)",
    ),
    E::new("kernel.home.quota.open", "th", "ดูการใช้งาน"),
    E::new("kernel.home.quota.open", "en", "Open usage"),
    // Backup card.
    E::new("kernel.home.backup.title", "th", "สำรองข้อมูล"),
    E::new("kernel.home.backup.title", "en", "Backups"),
    E::new("kernel.home.backup.empty", "th", "ยังไม่เคยสำรองข้อมูล"),
    E::new("kernel.home.backup.empty", "en", "No backup yet."),
    E::new("kernel.home.backup.start", "th", "สำรองข้อมูล"),
    E::new("kernel.home.backup.start", "en", "Back up"),
    E::new("kernel.home.backup.done", "th", "สำรองสำเร็จล่าสุด {time}"),
    E::new("kernel.home.backup.done", "en", "Last backup {time}"),
    E::new(
        "kernel.home.backup.running",
        "th",
        "กำลังสำรองข้อมูล เริ่มเมื่อ {time}",
    ),
    E::new(
        "kernel.home.backup.running",
        "en",
        "Backing up since {time}",
    ),
    E::new(
        "kernel.home.backup.failed",
        "th",
        "สำรองข้อมูลไม่สำเร็จ ({time})",
    ),
    E::new(
        "kernel.home.backup.failed",
        "en",
        "The backup failed ({time}).",
    ),
    E::new("kernel.home.backup.open", "th", "ดูการสำรองข้อมูล"),
    E::new("kernel.home.backup.open", "en", "Open backups"),
];

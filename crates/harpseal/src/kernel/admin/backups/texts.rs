//! Default texts of the backups page and the failure email.

use crate::kernel::admin::texts::th_en;
use crate::manifest::CatalogEntry as E;

pub(super) const DEFAULTS: &[E] = th_en![
    ("kernel.backup.title", "สำรองข้อมูล", "Backups"),
    (
        "kernel.backup.intro",
        "ระบบสำรองข้อมูลทั้งหมด (ฐานข้อมูลและไฟล์) ไว้ในที่เก็บไฟล์ของระบบทุกคืน และลบชุดที่เก่ากว่า {days} วันหลังสำรองสำเร็จ ชุดสำรองมีรหัสผ่านที่เข้ารหัสแล้วและข้อมูลส่วนบุคคล จึงต้องเก็บรักษาเท่ากับฐานข้อมูล",
        "Every night the whole Deployment (database and files) is backed up to its file storage; after a successful backup, backups older than {days} days are deleted. A backup holds password hashes and personal data: keep a download as safe as the database."
    ),
    // {schedule} {offset}
    (
        "kernel.backup.schedule",
        "กำหนดเวลา: {schedule} (เวลา UTC{offset})",
        "Schedule: {schedule} (UTC{offset})"
    ),
    (
        "kernel.backup.disabled",
        "ระบบไม่ได้ตั้งให้สำรองอัตโนมัติ กด \"สำรองตอนนี้\" เพื่อสำรองเองได้",
        "Scheduled backups are off ([backup] enabled = false); you can still back up now."
    ),
    ("kernel.backup.now", "สำรองตอนนี้", "Back up now"),
    // {size}
    (
        "kernel.backup.total",
        "ชุดสำรองทั้งหมดใช้พื้นที่ {size}",
        "All backups take {size}"
    ),
    (
        "kernel.backup.requested",
        "เริ่มสำรองข้อมูลแล้ว รอสักครู่แล้วโหลดหน้านี้ใหม่",
        "The backup has started. Reload this page in a few minutes."
    ),
    (
        "kernel.backup.busy",
        "กำลังสำรองข้อมูลอยู่ รอให้เสร็จก่อน",
        "A backup is already running. Wait for it to finish."
    ),
    (
        "kernel.backup.export",
        "ดาวน์โหลดข้อมูลทั้งหมด (Excel)",
        "Download all data (Excel)"
    ),
    (
        "kernel.backup.export_note",
        "ข้อมูลแต่ละชุดอยู่คนละชีต ไม่รวมบันทึกการทำงานของระบบ งานเบื้องหลัง ข้อความขาออก รหัสผ่าน และข้อมูลลับอื่น ๆ ไฟล์นี้ใช้กู้คืนระบบไม่ได้",
        "One sheet per table, without the operational tables (logs, Jobs, outbound messages), password hashes, tokens and other secrets. Not a backup: it cannot be restored."
    ),
    // {cells} {limit} {table} {table_cells}
    (
        "kernel.backup.export_too_big",
        "ข้อมูลมี {cells} ช่อง เกินขีดจำกัด {limit} ช่องของการดาวน์โหลดเป็น Excel ชุดที่ใหญ่ที่สุดคือ {table} ({table_cells} ช่อง) ให้ดาวน์โหลดชุดนั้นแยกจากรายการใน \"ข้อมูลทางเทคนิค\" หรือขอให้ผู้ดูแลเซิร์ฟเวอร์เพิ่มขีดจำกัด",
        "The data has {cells} cells, over the Excel download's limit of {limit}. The largest table is {table} ({table_cells} cells): download it alone from the list under \"Technical details\", or ask your server administrator to raise the limit."
    ),
    // {table} {cells} {limit}
    (
        "kernel.backup.export_table_too_big",
        "ชุดข้อมูล {table} มี {cells} ช่อง เกินขีดจำกัด {limit} ช่อง ให้ดาวน์โหลดชุดสำรองแทน หรือขอให้ผู้ดูแลเซิร์ฟเวอร์เพิ่มขีดจำกัด",
        "Table {table} alone has {cells} cells, over the limit of {limit}. Download a backup instead, or ask your server administrator to raise the limit."
    ),
    (
        "kernel.backup.tables",
        "ดาวน์โหลดทีละตาราง (Excel)",
        "Download one table (Excel)"
    ),
    ("kernel.backup.table", "ตาราง", "Table"),
    ("kernel.backup.rows", "แถว", "Rows"),
    (
        "kernel.backup.not_in_all",
        "ไม่รวมใน \"ดาวน์โหลดข้อมูลทั้งหมด\"",
        "Not in \"Download all data\""
    ),
    ("kernel.backup.started", "เริ่ม", "Started"),
    ("kernel.backup.state", "สถานะ", "Status"),
    ("kernel.backup.size", "ขนาด", "Size"),
    ("kernel.backup.contents", "ข้อมูล", "Contents"),
    // {tables} {rows} {files}
    (
        "kernel.backup.counts",
        "{tables} ตาราง {rows} แถว {files} ไฟล์",
        "{tables} tables, {rows} rows, {files} files"
    ),
    ("kernel.backup.origin_schedule", "อัตโนมัติ", "Scheduled"),
    ("kernel.backup.origin_manual", "สั่งเอง", "Manual"),
    ("kernel.backup.state_running", "กำลังสำรอง", "Running"),
    ("kernel.backup.state_done", "สำเร็จ", "Done"),
    ("kernel.backup.state_failed", "ไม่สำเร็จ", "Failed"),
    (
        "kernel.backup.download",
        "ดาวน์โหลด (.tar)",
        "Download (.tar)"
    ),
    (
        "kernel.backup.none",
        "ยังไม่มีชุดสำรอง",
        "There are no backups yet."
    ),
    (
        "kernel.backup.failed_alert",
        "การสำรองข้อมูลครั้งล่าสุดไม่สำเร็จ ดูสาเหตุใน \"ข้อมูลทางเทคนิค\" ท้ายหน้า แล้วกด \"สำรองตอนนี้\" อีกครั้ง",
        "The latest backup failed. See why under \"Technical details\" at the end of the page, then press \"Back up now\" again."
    ),
    (
        "kernel.backup.job_errors",
        "ข้อผิดพลาดของงานสำรองข้อมูล (30 วันล่าสุด)",
        "Backup Job errors (last 30 days)"
    ),
    (
        "kernel.backup.restore_hint",
        "กู้คืน: แตกไฟล์ .tar แล้วใช้คำสั่ง harpseal restore กับโฟลเดอร์ที่ได้ (ดูคู่มือ self-hosting)",
        "To restore: unpack the .tar and run harpseal restore on the folder it makes (see the self-hosting guide)."
    ),
    // {org} {day}
    (
        "kernel.backup.email_subject",
        "[{org}] การสำรองข้อมูลล้มเหลว ({day})",
        "[{org}] A backup failed ({day})"
    ),
    // {path}
    (
        "kernel.backup.email_body",
        "การสำรองข้อมูลอัตโนมัติล้มเหลว ระบบยังทำงานตามปกติ แต่ข้อมูลล่าสุดยังไม่ถูกสำรอง\n\n\
         ดูสาเหตุได้ที่หน้าสำรองข้อมูลในระบบหลังบ้าน ({path}) อีเมลนี้ส่งไม่เกินวันละหนึ่งฉบับ",
        "A backup failed. The Deployment keeps working, but its latest data is not backed up.\n\n\
         See why on the Backups page of the back office ({path}). This email is sent at most once a day."
    ),
    // {org} {day}
    (
        "kernel.backup.prune_email_subject",
        "[{org}] ลบชุดสำรองข้อมูลเก่าไม่สำเร็จ ({day})",
        "[{org}] Deleting old backups failed ({day})"
    ),
    // {path}
    (
        "kernel.backup.prune_email_body",
        "สำรองข้อมูลสำเร็จ แต่ลบชุดสำรองที่เก่ากว่ากำหนดไม่สำเร็จ ชุดเก่าจึงยังใช้พื้นที่และยังมีข้อมูลส่วนบุคคลอยู่\n\n\
         ดูสาเหตุได้ที่หน้าสำรองข้อมูลในระบบหลังบ้าน ({path}) อีเมลนี้ส่งไม่เกินวันละหนึ่งฉบับ",
        "The backup succeeded, but deleting backups past their retention failed: they still take space and still hold personal data.\n\n\
         See why on the Backups page of the back office ({path}). This email is sent at most once a day."
    ),
];

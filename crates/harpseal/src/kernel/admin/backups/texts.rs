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
        "ปิดการสำรองอัตโนมัติอยู่ ([backup] enabled = false) ยังกดสำรองเองได้",
        "Scheduled backups are off ([backup] enabled = false); you can still back up now."
    ),
    ("kernel.backup.now", "สำรองตอนนี้", "Back up now"),
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
        "หนึ่งชีตต่อหนึ่งตาราง ไม่รวมรหัสผ่าน โทเคน และค่าลับอื่น ๆ ไม่ใช่ชุดสำรองสำหรับกู้คืน",
        "One sheet per table, without password hashes, tokens and other secrets. Not a backup: it cannot be restored."
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
    ("kernel.backup.state_failed", "ล้มเหลว", "Failed"),
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
        "การสำรองข้อมูลครั้งล่าสุดล้มเหลว",
        "The latest backup failed."
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
];

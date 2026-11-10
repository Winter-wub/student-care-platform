//! Default texts of saved Audiences (#144), in Thai and English.

use crate::manifest::CatalogEntry as E;

macro_rules! th_en {
    ($( ($k:expr, $th:expr, $en:expr) ),* $(,)?) => {
        &[ $( E::new($k, "th", $th), E::new($k, "en", $en), )* ]
    };
}

pub(crate) const SAVED: &[E] = th_en![
    // On the editor.
    (
        "announcements.saved.pick",
        "เลือกจากกลุ่มผู้รับที่บันทึกไว้",
        "Choose a saved Audience"
    ),
    (
        "announcements.saved.pick_none",
        "เลือกกลุ่มผู้รับ",
        "Choose an Audience"
    ),
    ("announcements.saved.use", "ใช้กลุ่มนี้", "Use this Audience"),
    (
        "announcements.saved.used",
        "ใช้กลุ่มผู้รับ \"{name}\" แล้ว จำนวนผู้รับด้านล่างนับใหม่จากข้อมูลวันนี้ และจะนับอีกครั้งก่อนส่ง",
        "Using the Audience \"{name}\". The reach below is counted from today's data, and again before sending."
    ),
    (
        "announcements.saved.use_gone",
        "ไม่พบกลุ่มผู้รับที่เลือก อาจถูกลบไปแล้ว ผู้รับด้านล่างยังเหมือนเดิม",
        "That saved Audience was not found; it may have been deleted. The Audience below is unchanged."
    ),
    (
        "announcements.saved.name",
        "ชื่อกลุ่มผู้รับ (ถ้าจะบันทึกไว้ใช้อีก)",
        "Audience name (to save it for later)"
    ),
    (
        "announcements.saved.save",
        "บันทึกกลุ่มผู้รับนี้",
        "Save this Audience"
    ),
    (
        "announcements.saved.saved",
        "บันทึกกลุ่มผู้รับ \"{name}\" แล้ว ครั้งหน้าเลือกใช้ได้จากด้านบนของส่วนผู้รับ",
        "Saved the Audience \"{name}\". Next time, choose it at the top of the Audience."
    ),
    (
        "announcements.saved.manage",
        "จัดการกลุ่มผู้รับที่บันทึกไว้",
        "Manage saved Audiences"
    ),
    (
        "announcements.saved.bad_name",
        "ใส่ชื่อกลุ่มผู้รับ ไม่เกิน 60 ตัวอักษร",
        "Enter a name of at most 60 characters"
    ),
    (
        "announcements.saved.taken",
        "มีกลุ่มผู้รับชื่อนี้แล้ว ใช้ชื่ออื่น หรือเปลี่ยนชื่อกลุ่มเดิมในหน้ากลุ่มผู้รับที่บันทึกไว้",
        "An Audience has this name already. Use another name, or rename the old one on the saved Audiences page."
    ),
    (
        "announcements.saved.full",
        "บันทึกกลุ่มผู้รับได้สูงสุด 100 กลุ่ม ลบกลุ่มที่ไม่ใช้แล้วในหน้ากลุ่มผู้รับที่บันทึกไว้ก่อน",
        "At most 100 Audiences can be saved. Delete one you no longer use first."
    ),
    (
        "announcements.saved.stale",
        "ค่าที่มีเครื่องหมายอยู่ด้านล่างนี้ไม่มีในข้อมูลนักศึกษาแล้ว กลุ่มผู้รับนี้จึงยังใช้ไม่ได้ เอาเครื่องหมายออก แล้วเลือกค่าที่ต้องการแทน ระบบจะไม่ส่งถึงทุกคนแทนค่าที่หายไป",
        "The marked values below are no longer in the Student data, so this Audience cannot be used yet. Untick them and choose the values you want. A missing value never turns into everyone."
    ),
    // The list page.
    (
        "announcements.saved.title",
        "กลุ่มผู้รับที่บันทึกไว้",
        "Saved Audiences"
    ),
    (
        "announcements.saved.help",
        "กลุ่มผู้รับที่บันทึกไว้เก็บเงื่อนไขการเลือกผู้รับ ไม่ได้เก็บรายชื่อนักศึกษา จำนวนผู้รับจึงนับใหม่ทุกครั้งที่ใช้ บันทึกกลุ่มใหม่ได้ในส่วนผู้รับของหน้าเขียนประกาศ",
        "A saved Audience keeps the rules, not a list of Students, so its reach is counted again each time it is used. Save a new one in the Audience part of the announcement editor."
    ),
    (
        "announcements.saved.empty",
        "ยังไม่มีกลุ่มผู้รับที่บันทึกไว้ ตั้งค่าผู้รับในหน้าเขียนประกาศ แล้วกด \"บันทึกกลุ่มผู้รับนี้\"",
        "No saved Audiences yet. Set the Audience on the announcement editor, then press \"Save this Audience\"."
    ),
    (
        "announcements.saved.write_to",
        "เขียนประกาศถึงกลุ่มนี้",
        "Write to this Audience"
    ),
    (
        "announcements.saved.edit",
        "เปลี่ยนชื่อหรือลบ",
        "Rename or delete"
    ),
    ("announcements.saved.new_name", "ชื่อใหม่", "New name"),
    ("announcements.saved.rename", "เปลี่ยนชื่อ", "Rename"),
    (
        "announcements.saved.delete",
        "ลบกลุ่มผู้รับนี้",
        "Delete this Audience"
    ),
    (
        "announcements.saved.delete_help",
        "ประกาศที่เคยใช้กลุ่มนี้ไม่เปลี่ยน",
        "Announcements that used it do not change."
    ),
    ("announcements.saved.updated", "แก้ไขล่าสุด", "Last changed"),
    (
        "announcements.saved.stale_list",
        "ค่าเหล่านี้ไม่มีในข้อมูลนักศึกษาแล้ว ต้องเลือกใหม่ก่อนส่ง:",
        "No longer in the Student data, choose again before sending:"
    ),
    (
        "announcements.saved.renamed",
        "เปลี่ยนชื่อกลุ่มผู้รับแล้ว",
        "Audience renamed"
    ),
    (
        "announcements.saved.deleted",
        "ลบกลุ่มผู้รับแล้ว",
        "Audience deleted"
    ),
    (
        "announcements.saved.gone",
        "ไม่พบกลุ่มผู้รับนี้ อาจถูกลบไปแล้ว",
        "That Audience was not found; it may have been deleted"
    ),
    (
        "announcements.saved.unreadable",
        "อ่านกลุ่มนี้ไม่ได้ ลบแล้วบันทึกใหม่",
        "This Audience cannot be read. Delete it and save it again."
    ),
    (
        "announcements.saved.use_unreadable",
        "อ่านกลุ่มผู้รับที่เลือกไม่ได้ ผู้รับด้านล่างยังเหมือนเดิม ลบกลุ่มนี้ในหน้ากลุ่มผู้รับที่บันทึกไว้แล้วบันทึกใหม่",
        "That saved Audience cannot be read; the Audience below is unchanged. Delete it on the saved Audiences page and save it again."
    ),
    (
        "announcements.saved.unchanged",
        "ชื่อเหมือนเดิม จึงไม่ได้เปลี่ยน",
        "The name is the same; nothing changed"
    ),
    (
        "announcements.saved.delete_title",
        "ลบกลุ่มผู้รับ \"{name}\"",
        "Delete the Audience \"{name}\""
    ),
    (
        "announcements.saved.delete_what",
        "ลบแล้วจะเลือกกลุ่มนี้ในหน้าเขียนประกาศไม่ได้อีก และกู้คืนไม่ได้ ประกาศที่เคยใช้กลุ่มนี้ไม่เปลี่ยน",
        "Once deleted it can no longer be chosen on the editor, and it cannot be restored. Announcements that used it do not change."
    ),
    ("announcements.saved.keep", "ไม่ลบ กลับไป", "Keep it, go back"),
];

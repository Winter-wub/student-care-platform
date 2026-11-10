//! Default texts of the bulk actions on the Students list (#145), in Thai
//! and English. `{n}` is a number, `{tag}` a Tag name, `{name}` a
//! Student's name.

use crate::manifest::CatalogEntry as E;

macro_rules! th_en {
    ($( ($k:expr, $th:expr, $en:expr) ),* $(,)?) => {
        &[ $( E::new($k, "th", $th), E::new($k, "en", $en), )* ]
    };
}

pub(crate) const DEFAULTS: &[E] = th_en![
    // The list's bulk form.
    (
        "kernel.students.bulk_title",
        "ทำกับหลายคนพร้อมกัน",
        "Act on several Students"
    ),
    ("kernel.students.bulk_col", "เลือก", "Select"),
    ("kernel.students.bulk_pick", "เลือก {name}", "Select {name}"),
    (
        "kernel.students.bulk_which",
        "นักศึกษาที่จะทำ",
        "Which Students"
    ),
    (
        "kernel.students.bulk_which_picked",
        "เฉพาะคนที่ติ๊กไว้ในตาราง",
        "Only the rows ticked in the table"
    ),
    (
        "kernel.students.bulk_which_page",
        "ทุกคนในหน้านี้ ({n} คน)",
        "Everyone on this page ({n})"
    ),
    (
        "kernel.students.bulk_which_filter",
        "ทุกคนที่ตรงกับการค้นหา ({n} คน)",
        "Everyone matching the search ({n})"
    ),
    (
        "kernel.students.bulk_count",
        "เลือกแล้ว {n} คน",
        "{n} selected"
    ),
    (
        "kernel.students.bulk_count_none",
        "ยังไม่ได้เลือกใคร ติ๊กช่องหน้าชื่อ หรือเลือกทั้งหน้า",
        "Nobody selected yet: tick rows, or choose a whole page"
    ),
    (
        "kernel.students.bulk_tag",
        "ชื่อป้าย (ใช้กับปุ่มเพิ่มป้ายและเอาป้ายออก)",
        "Tag name (for Add Tag and Remove Tag)"
    ),
    ("kernel.students.bulk_tag_add", "เพิ่มป้าย", "Add Tag"),
    ("kernel.students.bulk_tag_remove", "เอาป้ายออก", "Remove Tag"),
    (
        "kernel.students.bulk_unlink",
        "ยกเลิกการเชื่อมบัญชี",
        "Unlink chat accounts"
    ),
    (
        "kernel.students.bulk_announce",
        "ประกาศถึงที่เลือก",
        "Announce to the selection"
    ),
    // The review step.
    (
        "kernel.students.bulk_review",
        "ตรวจก่อนยืนยัน",
        "Check before you confirm"
    ),
    (
        "kernel.students.bulk_review_tag_add",
        "เพิ่มป้าย “{tag}” ให้นักศึกษา {n} คน",
        "Add the Tag “{tag}” to {n} Students"
    ),
    (
        "kernel.students.bulk_review_tag_remove",
        "เอาป้าย “{tag}” ออกจากนักศึกษา {n} คน (ป้ายจากทะเบียนไม่เปลี่ยน)",
        "Remove the Tag “{tag}” from {n} Students (roster Tags stay)"
    ),
    (
        "kernel.students.bulk_review_unlink",
        "ยกเลิกการเชื่อมบัญชีแชททุกช่องทางของนักศึกษา {n} คน แต่ละคนจะได้รับแจ้งในแชทและเมนูจะถูกล็อก",
        "Unlink every chat account of {n} Students. Each is told in the chat and their Menu is locked."
    ),
    (
        "kernel.students.bulk_skip_tag_add",
        "อีก {n} คนมีป้ายนี้อยู่แล้ว จะไม่เปลี่ยน",
        "{n} more already have this Tag and stay as they are"
    ),
    (
        "kernel.students.bulk_skip_tag_remove",
        "อีก {n} คนไม่มีป้ายนี้ จะไม่เปลี่ยน",
        "{n} more do not have this Tag and stay as they are"
    ),
    (
        "kernel.students.bulk_skip_unlink",
        "อีก {n} คนยังไม่ได้เชื่อมบัญชี จะไม่เปลี่ยน",
        "{n} more have no linked account and stay as they are"
    ),
    (
        "kernel.students.bulk_codes",
        "รหัสนักศึกษาที่เลือก",
        "Student IDs selected"
    ),
    ("kernel.students.bulk_more", "และอีก {n} คน", "and {n} more"),
    (
        "kernel.students.bulk_confirm_tag_add",
        "ยืนยัน: เพิ่มป้ายให้ {n} คน",
        "Confirm: tag {n} Students"
    ),
    (
        "kernel.students.bulk_confirm_tag_remove",
        "ยืนยัน: เอาป้ายออกจาก {n} คน",
        "Confirm: untag {n} Students"
    ),
    (
        "kernel.students.bulk_confirm_unlink",
        "ยืนยัน: ยกเลิกการเชื่อม {n} คน",
        "Confirm: unlink {n} Students"
    ),
    (
        "kernel.students.bulk_typed",
        "พิมพ์ตัวเลข {n} เพื่อยืนยันว่านักศึกษา {n} คนนี้จะถูกยกเลิกการเชื่อมบัญชีแชท",
        "Type the number {n} to confirm that these {n} Students lose their chat Links"
    ),
    (
        "kernel.students.bulk_typed_wrong",
        "ตัวเลขไม่ตรง พิมพ์ตัวเลข {n} แล้วกดยืนยันอีกครั้ง",
        "That number does not match. Type {n}, then confirm again"
    ),
    (
        "kernel.students.bulk_changed",
        "รายชื่อเปลี่ยนไประหว่างนี้ ตรวจจำนวนใหม่ด้านล่างแล้วยืนยันอีกครั้ง",
        "The selection changed meanwhile. Check the new numbers below, then confirm again"
    ),
    (
        "kernel.students.bulk_nothing_tag_add",
        "นักศึกษาที่เลือกทุกคนมีป้ายนี้อยู่แล้ว จึงไม่มีอะไรต้องเพิ่ม",
        "Every Student selected already has this Tag, so there is nothing to add"
    ),
    (
        "kernel.students.bulk_nothing_tag_remove",
        "นักศึกษาที่เลือกไม่มีใครมีป้ายนี้ จึงไม่มีอะไรต้องเอาออก",
        "No Student selected has this Tag, so there is nothing to remove"
    ),
    (
        "kernel.students.bulk_nothing_unlink",
        "นักศึกษาที่เลือกยังไม่ได้เชื่อมบัญชีแชท จึงไม่มีอะไรต้องยกเลิก",
        "No Student selected has a linked chat account, so there is nothing to unlink"
    ),
    // Refusals: the problem, then what to do.
    (
        "kernel.students.bulk_refused",
        "ทำรายการนี้ไม่ได้",
        "This cannot be done"
    ),
    (
        "kernel.students.bulk_too_many",
        "เลือกไว้ {n} คน ทำพร้อมกันได้ครั้งละไม่เกิน 2,000 คน ค้นหาให้แคบลงแล้วลองใหม่",
        "{n} Students are selected; one action takes at most 2,000. Narrow the search and try again."
    ),
    (
        "kernel.students.bulk_unlink_too_many",
        "เลือกไว้ {n} คน ยกเลิกการเชื่อมได้ครั้งละไม่เกิน 100 คน ค้นหาให้แคบลง หรือติ๊กเฉพาะบางคน แล้วลองใหม่",
        "{n} Students are selected; unlinking takes at most 100 at a time. Narrow the search or tick fewer rows, then try again."
    ),
    (
        "kernel.students.bulk_announce_too_many",
        "เลือกไว้ {n} คน ประกาศถึงรายคนได้ไม่เกิน 2,000 คน ค้นหาให้แคบลง หรือเขียนประกาศแล้วใช้ “เลือกกลุ่ม” แทน",
        "{n} Students are selected; a hand-picked Audience takes at most 2,000. Narrow the search, or write the announcement and use “Choose groups” instead."
    ),
    (
        "kernel.students.bulk_none",
        "ยังไม่ได้เลือกนักศึกษา ติ๊กช่องหน้าชื่อ หรือเลือก “ทุกคนในหน้านี้” แล้วลองใหม่",
        "No Student is selected. Tick rows or choose “Everyone on this page”, then try again."
    ),
    (
        "kernel.students.bulk_back",
        "กลับไปที่รายชื่อ",
        "Back to the list"
    ),
    // Flashes after a confirmed action.
    (
        "kernel.students.bulk_tag_added",
        "เพิ่มป้ายให้นักศึกษาที่เลือกแล้ว",
        "The Tag was added to the Students selected"
    ),
    (
        "kernel.students.bulk_tag_removed",
        "เอาป้ายออกจากนักศึกษาที่เลือกแล้ว",
        "The Tag was removed from the Students selected"
    ),
    (
        "kernel.students.bulk_unlinked",
        "ยกเลิกการเชื่อมแล้ว และแจ้งนักศึกษาทุกคนแล้ว",
        "Unlinked, and every Student was told"
    ),
    (
        "kernel.students.bulk_unlinked_unnotified",
        "ยกเลิกการเชื่อมแล้ว แต่ส่งข้อความแจ้งนักศึกษาบางคนไม่สำเร็จ",
        "Unlinked, but some Students could not be told"
    ),
    (
        "kernel.students.bulk_nothing",
        "ไม่มีอะไรเปลี่ยน นักศึกษาที่เลือกเป็นอย่างนั้นอยู่แล้ว",
        "Nothing changed: the Students selected were already like that"
    ),
];

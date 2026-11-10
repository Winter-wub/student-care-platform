//! Texts of #154: the may-be-empty columns a file leaves out, the header
//! an unknown one is close to, and the value a problem echoes.

use crate::manifest::CatalogEntry as E;

macro_rules! th_en {
    ($( ($k:expr, $th:expr, $en:expr) ),* $(,)?) => {
        &[ $( E::new($k, "th", $th), E::new($k, "en", $en), )* ]
    };
}

pub(crate) const DEFAULTS: &[E] = th_en![
    (
        "kernel.roster.absent_title",
        "คอลัมน์ที่ไม่มีในไฟล์",
        "Columns not in the file"
    ),
    (
        "kernel.roster.absent_help",
        "ไฟล์นี้ไม่มีคอลัมน์ด้านล่าง ซึ่งเว้นว่างได้ นักศึกษาที่มีอยู่แล้วจะคงค่าเดิมของคอลัมน์เหล่านี้ไว้ ไม่มีค่าใดถูกลบ ส่วนนักศึกษาใหม่จะยังไม่มีค่าในคอลัมน์เหล่านี้",
        "The file leaves out the columns below, which may be empty. Students already on the roster keep what is stored in them; nothing is cleared. New Students start with them empty"
    ),
    (
        "kernel.roster.absent_full",
        "นำเข้าแบบทั้งทะเบียนก็ไม่ล้างค่าในคอลัมน์เหล่านี้ ถ้าต้องการล้างค่า ให้ใส่คอลัมน์ในไฟล์แล้วเว้นว่าง",
        "A full import does not clear these columns either. To clear a value, put the column in the file and leave it empty"
    ),
    (
        "kernel.roster.absent_tags",
        "ป้ายจากไฟล์รายชื่อของนักศึกษาที่มีอยู่แล้วยังอยู่ครบ กลุ่มผู้รับที่เลือกตามป้ายเหล่านี้จึงยังเลือกนักศึกษากลุ่มเดิม",
        "Students already on the roster keep their roster Tags, so Audiences that select those Tags still select them"
    ),
    (
        "kernel.roster.absent_no_email",
        "{n} คนจะไม่มีอีเมล จึงผูกบัญชีแชตเองไม่ได้ จนกว่าเจ้าหน้าที่จะเพิ่มอีเมลหรือผูกบัญชีให้",
        "{n} Students will have no email, so they cannot link their chat account until Staff add one or link them by hand"
    ),
    (
        "kernel.roster.header_absent",
        "ไม่มีในไฟล์ ไม่เป็นไร",
        "Left out, which is fine"
    ),
    ("kernel.roster.header_near", "ใกล้เคียงกับ", "Close to"),
    ("kernel.roster.report_value", "ค่าในไฟล์:", "In the file:"),
    (
        "kernel.roster.report_value_column",
        "ค่าในไฟล์",
        "Value in the file"
    ),
];

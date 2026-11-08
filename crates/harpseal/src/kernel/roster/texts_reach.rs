//! Default texts of the confirmation in proportion to reach (#120), in
//! Thai and English. `{n}` is how many Students will become left.

use crate::manifest::CatalogEntry as E;

macro_rules! th_en {
    ($( ($k:expr, $th:expr, $en:expr) ),* $(,)?) => {
        &[ $( E::new($k, "th", $th), E::new($k, "en", $en), )* ]
    };
}

pub(crate) const DEFAULTS: &[E] = th_en![
    (
        "kernel.roster.confirm_left",
        "ยืนยัน: {n} คนจะพ้นสภาพ",
        "Confirm: {n} will become left"
    ),
    (
        "kernel.roster.typed_label",
        "พิมพ์ {n} หรือคำว่า ยืนยัน เพื่อยืนยันว่า {n} คนนี้จะพ้นสภาพและถูกยกเลิกการเชื่อมบัญชีแชท",
        "Type {n} or the word ยืนยัน to confirm that these {n} Students become left and lose their chat Links"
    ),
    (
        "kernel.roster.typed_wrong",
        "ข้อความไม่ตรง พิมพ์ {n} หรือคำว่า ยืนยัน แล้วกดยืนยันอีกครั้ง",
        "That does not match. Type {n} or ยืนยัน, then confirm again"
    ),
    (
        "kernel.roster.typed_refused",
        "ยังไม่ได้นำเข้า เพราะข้อความยืนยันไม่ตรง",
        "Nothing was imported: the typed confirmation did not match"
    ),
];

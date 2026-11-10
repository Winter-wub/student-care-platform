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
        "พิมพ์ตัวเลข {n} เพื่อยืนยันว่านักศึกษา {n} คนนี้จะพ้นสภาพและถูกยกเลิกการผูกบัญชีแชท",
        "Type the number {n} to confirm that these {n} Students become left and lose their chat Links"
    ),
    (
        "kernel.roster.typed_wrong",
        "ตัวเลขไม่ตรง พิมพ์ตัวเลข {n} แล้วกดยืนยันอีกครั้ง",
        "That number does not match. Type {n}, then confirm again"
    ),
    (
        "kernel.roster.typed_refused",
        "ยังไม่ได้นำเข้า เพราะตัวเลขที่พิมพ์ไม่ตรงกับจำนวนที่จะพ้นสภาพ",
        "Nothing was imported: the number typed did not match"
    ),
];

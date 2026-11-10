//! Default texts of the help panels, empty states, next steps and the
//! first-run checklist (#127), Thai and English. Example values are
//! fictional.

use crate::manifest::CatalogEntry as E;

pub(crate) const TEXTS: &[E] = &[
    // The upload help panel.
    E::new("kernel.upload.help_title", "th", "ไฟล์ที่ต้องเตรียม"),
    E::new("kernel.upload.help_title", "en", "What the file needs"),
    E::new(
        "kernel.upload.rule_types",
        "th",
        "ไฟล์ CSV หรือ XLSX ขนาดไม่เกิน {mb} MB",
    ),
    E::new(
        "kernel.upload.rule_types",
        "en",
        "A CSV or XLSX file of at most {mb} MB",
    ),
    E::new(
        "kernel.upload.rule_utf8",
        "th",
        "ไฟล์ CSV ให้บันทึกแบบ CSV UTF-8 ภาษาไทยจึงจะไม่เพี้ยน",
    ),
    E::new(
        "kernel.upload.rule_utf8",
        "en",
        "Save a CSV as CSV UTF-8 so Thai text reads correctly",
    ),
    E::new(
        "kernel.upload.rule_header_any",
        "th",
        "แถวแรกเป็นหัวคอลัมน์ตามรายการด้านล่าง เรียงลำดับใดก็ได้",
    ),
    E::new(
        "kernel.upload.rule_header_any",
        "en",
        "The first row holds the headers below, in any order",
    ),
    E::new(
        "kernel.upload.rule_header",
        "th",
        "หัวคอลัมน์ต้องสะกดตามรายการด้านล่าง",
    ),
    E::new(
        "kernel.upload.rule_header",
        "en",
        "Spell the headers exactly as below",
    ),
    E::new("kernel.upload.columns", "th", "คอลัมน์"),
    E::new("kernel.upload.columns", "en", "Columns"),
    E::new("kernel.upload.required", "th", "ต้องมีค่า"),
    E::new("kernel.upload.required", "en", "Required"),
    E::new("kernel.upload.optional", "th", "เว้นว่างได้"),
    E::new("kernel.upload.optional", "en", "May be empty"),
    // The roster's reader also takes a file without such a column (#154).
    E::new("kernel.upload.optional_omit", "th", "เว้นว่างหรือไม่ใส่คอลัมน์ก็ได้"),
    E::new(
        "kernel.upload.optional_omit",
        "en",
        "May be empty or left out",
    ),
    E::new(
        "kernel.upload.rule_omit",
        "th",
        "คอลัมน์ที่เว้นว่างได้จะไม่ใส่ในไฟล์ก็ได้ นักศึกษาที่มีอยู่แล้วจะคงค่าเดิมของคอลัมน์นั้นไว้",
    ),
    E::new(
        "kernel.upload.rule_omit",
        "en",
        "A column that may be empty may also be left out: Students already on the roster keep what is stored in it",
    ),
    E::new("kernel.upload.example", "th", "ตัวอย่าง:"),
    E::new("kernel.upload.example", "en", "Example:"),
    E::new("kernel.upload.template", "th", "ดาวน์โหลดแม่แบบ CSV"),
    E::new("kernel.upload.template", "en", "Download the CSV template"),
    // Example values of the roster's own columns (fictional).
    E::new("kernel.upload.example_code", "th", "68010001"),
    E::new("kernel.upload.example_code", "en", "68010001"),
    E::new("kernel.upload.example_name", "th", "นกน้อย ทะเลใส"),
    E::new("kernel.upload.example_name", "en", "Noknoi Thalesai"),
    E::new(
        "kernel.upload.example_email",
        "th",
        "noknoi@student.example",
    ),
    E::new(
        "kernel.upload.example_email",
        "en",
        "noknoi@student.example",
    ),
    E::new("kernel.upload.example_tag", "th", "ชมรมดนตรี"),
    E::new("kernel.upload.example_tag", "en", "Music club"),
    E::new("kernel.upload.example_tag2", "th", "ทุนเรียนดี"),
    E::new("kernel.upload.example_tag2", "en", "Merit scholarship"),
    // The step after a confirmed import.
    E::new("kernel.next.title", "th", "ขั้นต่อไป"),
    E::new("kernel.next.title", "en", "Next step"),
    E::new("kernel.next.announce", "th", "แจ้งนักศึกษา"),
    E::new("kernel.next.announce", "en", "Tell Students"),
    E::new(
        "kernel.next.named",
        "th",
        "ร่างประกาศถึงนักศึกษา {n} คนในไฟล์นี้ แล้วตรวจอีกครั้งก่อนส่ง",
    ),
    E::new(
        "kernel.next.named",
        "en",
        "Draft an announcement to the {n} Students in this file, then review it before it is sent.",
    ),
    E::new(
        "kernel.next.choose",
        "th",
        "ร่างประกาศใหม่ เลือกกลุ่มผู้รับเอง แล้วตรวจอีกครั้งก่อนส่ง",
    ),
    E::new(
        "kernel.next.choose",
        "en",
        "Draft a new announcement, choose its Audience, then review it before it is sent.",
    ),
    // Empty lists.
    E::new(
        "kernel.empty.students",
        "th",
        "รายชื่อนักศึกษามาจากไฟล์ทะเบียน เมื่อนำเข้าแล้วรายชื่อจะแสดงที่นี่",
    ),
    E::new(
        "kernel.empty.students",
        "en",
        "Students come from the roster file. Import it and they appear here.",
    ),
    E::new("kernel.empty.students_action", "th", "นำเข้ารายชื่อ"),
    E::new("kernel.empty.students_action", "en", "Import the roster"),
    E::new(
        "kernel.empty.students_no_match",
        "th",
        "ไม่พบนักศึกษาที่ตรงกับคำค้นหรือตัวกรองนี้ ลองค้นด้วยรหัสนักศึกษาหรือชื่อบางส่วน",
    ),
    E::new(
        "kernel.empty.students_no_match",
        "en",
        "No Student matches this search or filter. Try a student ID or part of a name.",
    ),
    E::new("kernel.empty.clear_search", "th", "ล้างการค้นหา"),
    E::new("kernel.empty.clear_search", "en", "Clear the search"),
    E::new(
        "kernel.empty.imports",
        "th",
        "การนำเข้าแต่ละครั้งจะแสดงที่นี่ พร้อมผลและรายงานปัญหา",
    ),
    E::new(
        "kernel.empty.imports",
        "en",
        "Each import appears here with its result and problem report.",
    ),
    E::new(
        "kernel.empty.announcements",
        "th",
        "ประกาศที่ส่งถึงนักศึกษาจะแสดงที่นี่ พร้อมจำนวนผู้รับทราบ",
    ),
    E::new(
        "kernel.empty.announcements",
        "en",
        "Announcements to Students appear here, with who acknowledged them.",
    ),
    E::new("kernel.empty.announcements_action", "th", "เขียนประกาศ"),
    E::new(
        "kernel.empty.announcements_action",
        "en",
        "Write an announcement",
    ),
    E::new(
        "kernel.empty.staff",
        "th",
        "ตอนนี้มีแค่บัญชีของคุณ เพิ่มเจ้าหน้าที่เพื่อให้คนอื่นช่วยนำเข้าไฟล์และส่งประกาศได้",
    ),
    E::new(
        "kernel.empty.staff",
        "en",
        "Only your account so far. Add Staff so others can import files and send announcements.",
    ),
    E::new(
        "kernel.empty.audit",
        "th",
        "ทุกการเปลี่ยนแปลงที่เจ้าหน้าที่ทำในหลังบ้านจะบันทึกไว้ที่นี่",
    ),
    E::new(
        "kernel.empty.audit",
        "en",
        "Every change Staff make in the back office is recorded here.",
    ),
    E::new(
        "kernel.empty.security",
        "th",
        "เหตุการณ์ความปลอดภัย เช่น การกรอกรหัสยืนยันผิดซ้ำ จะแสดงที่นี่เมื่อเกิดขึ้น",
    ),
    E::new(
        "kernel.empty.security",
        "en",
        "Security Events, such as repeated wrong codes, appear here when they happen.",
    ),
    E::new(
        "kernel.empty.filtered",
        "th",
        "ไม่พบรายการที่ตรงกับตัวกรองนี้ ลองขยายช่วงวันที่หรือเอาบางเงื่อนไขออก",
    ),
    E::new(
        "kernel.empty.filtered",
        "en",
        "Nothing matches these filters. Widen the dates or remove a condition.",
    ),
    E::new("kernel.empty.clear_filters", "th", "ล้างตัวกรอง"),
    E::new("kernel.empty.clear_filters", "en", "Clear the filters"),
    // The first-run checklist on the home.
    E::new("kernel.setup.title", "th", "เริ่มต้นใช้งาน"),
    E::new("kernel.setup.title", "en", "Getting started"),
    E::new(
        "kernel.setup.intro",
        "th",
        "เหลืออีก {left} จาก {total} ขั้น รายการนี้จะหายไปเมื่อทำครบ",
    ),
    E::new(
        "kernel.setup.intro",
        "en",
        "{left} of {total} steps left. This list goes away once they are done.",
    ),
    E::new("kernel.setup.branding", "th", "ตั้งชื่อ สี และโลโก้ของสถาบัน"),
    E::new(
        "kernel.setup.branding",
        "en",
        "Set the institution's name, colour and logo",
    ),
    E::new("kernel.setup.roster", "th", "นำเข้ารายชื่อนักศึกษา"),
    E::new("kernel.setup.roster", "en", "Import the Student roster"),
    E::new("kernel.setup.staff", "th", "เพิ่มเจ้าหน้าที่คนอื่น"),
    E::new("kernel.setup.staff", "en", "Add other Staff"),
    E::new("kernel.setup.announce", "th", "ส่งประกาศฉบับแรก"),
    E::new("kernel.setup.announce", "en", "Send the first announcement"),
    E::new("kernel.setup.done", "th", "เสร็จแล้ว"),
    E::new("kernel.setup.done", "en", "Done"),
    E::new("kernel.setup.todo", "th", "ยังไม่ได้ทำ"),
    E::new("kernel.setup.todo", "en", "To do"),
];

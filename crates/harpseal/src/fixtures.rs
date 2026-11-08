//! The first slice's messages, built from the ADR-0003 model (issue #3).
//! Fictional Deployment: มหาวิทยาลัยอ่าวแมวน้ำ (Seal Bay University).
//! Behind the `fixtures` feature; channel crates enable it as a dev-dependency.

use crate::message::{
    ActionId, Attachment, Button, Card, CardList, Choices, Menu, MenuEntry, Message,
};

/// One rendered sample: a Message (`render::message`) or a Menu (`render::menu`).
#[non_exhaustive]
pub enum Sample {
    /// A Message for `render::message`.
    Message(Message),
    /// A Menu for `render::menu`.
    Menu(Menu),
}

fn id(s: &str) -> ActionId {
    ActionId::new(s).expect("fixture Action ids are ≤ 64 bytes")
}

fn action(label: &str, action_id: &str) -> Button {
    Button::Action {
        label: label.into(),
        action: id(action_id),
    }
}

fn mini_app(label: &str, path: &str) -> Button {
    Button::MiniApp {
        label: label.into(),
        path: path.into(),
    }
}

/// A `Button::Url` with any URL, past [`Button::url`]'s check (#108): what
/// only code inside the Core could build, so adapter tests can show they
/// refuse it. Not a first-slice sample.
pub fn raw_url_button(label: &str, url: &str) -> Button {
    Button::Url {
        label: label.into(),
        url: url.into(),
    }
}

fn news(n: u32, title: &str, body: &str) -> Card {
    Card {
        image: None,
        title: title.into(),
        body: body.into(),
        buttons: vec![
            action("รับทราบ", &format!("news.ack.{n}")),
            mini_app("เปิดดูรายละเอียด", &format!("/news/{n}")),
        ],
    }
}

/// (a) One news Card with an acknowledge Action and a Mini App button.
pub fn news_card() -> Message {
    Message::Card(news(
        1042,
        "ปิดอาคารเรียนรวม 2 วันเสาร์ที่ 14 พ.ย.",
        "งานอาคารสถานที่จะปิดอาคารเรียนรวม 2 ทั้งวันเพื่อซ่อมระบบไฟฟ้า \
         นักศึกษาที่มีเรียนชดเชยให้ไปที่ห้อง 3105 อาคารเรียนรวม 3 แทน \
         กด “รับทราบ” เพื่อยืนยันว่าได้อ่านประกาศนี้แล้ว",
    ))
}

/// (b) Email-OTP Verification flow.
pub fn otp_ask_student_id() -> Message {
    Message::text("กรุณาพิมพ์รหัสนักศึกษา 8 หลักของคุณ เพื่อผูกบัญชีกับระบบของมหาวิทยาลัยอ่าวแมวน้ำ")
}

/// Fixture `otp_ask_code` (see `docs/prototypes/message-render/`).
pub fn otp_ask_code() -> Message {
    Message::text(
        "เราส่งรหัส 6 หลักไปที่อีเมล s6*****12@student.sealbay.example แล้ว \
         กรุณาพิมพ์รหัสภายใน 10 นาที",
    )
}

/// Fixture `otp_wrong_code` (see `docs/prototypes/message-render/`).
pub fn otp_wrong_code() -> Message {
    Message::text("รหัสไม่ถูกต้อง ลองใหม่ได้อีก 2 ครั้ง")
}

/// Fixture `otp_locked` (see `docs/prototypes/message-render/`).
pub fn otp_locked() -> Message {
    Message::text(
        "ใส่รหัสผิดเกินจำนวนครั้งที่กำหนด ระบบล็อกการยืนยันตัวตนไว้ 30 นาที \
         หากไม่ได้ทำรายการนี้เอง กรุณาติดต่อกองกิจการนักศึกษา",
    )
}

/// Fixture `otp_success` (see `docs/prototypes/message-render/`).
pub fn otp_success() -> Message {
    Message::Card(Card {
        image: None,
        title: "ผูกบัญชีเรียบร้อย".into(),
        body: "สวัสดีคุณสมหญิง (รหัส 65012345) ตอนนี้คุณจะได้รับประกาศและใช้เมนูทั้งหมดได้แล้ว".into(),
        buttons: vec![action("ดูประกาศล่าสุด", "news.latest")],
    })
}

/// (c) Unlink confirmation.
pub fn unlink_confirm() -> Message {
    Message::Choices(Choices {
        text: "ยืนยันการยกเลิกการเชื่อมต่อ?".into(),
        options: vec![
            action("ยืนยัน", "link.revoke.yes"),
            action("ยกเลิก", "link.revoke.no"),
        ],
    })
}

const HEADLINES: [&str; 5] = [
    "เปิดลงทะเบียนเรียนภาคฤดูร้อน",
    "ประกาศผลทุนการศึกษาประจำปี",
    "กำหนดการสอบปลายภาค",
    "รับสมัครนักศึกษาช่วยงานห้องสมุด",
    "ปรับเวลารถรับส่งภายในมหาวิทยาลัย",
];

fn news_list(n: u32) -> Message {
    Message::CardList(CardList(
        (1..=n)
            .map(|i| {
                let title = format!("{} #{i}", HEADLINES[(i as usize - 1) % HEADLINES.len()]);
                news(2000 + i, &title, "รายละเอียดย่อของข่าว อ่านฉบับเต็มได้จากปุ่มด้านล่าง")
            })
            .collect(),
    ))
}

/// (d) Card Lists of 3 and 15 news items.
pub fn news_list_3() -> Message {
    news_list(3)
}

/// Fixture `news_list_15` (see `docs/prototypes/message-render/`).
pub fn news_list_15() -> Message {
    news_list(15)
}

fn entry(label: &str, action_id: &str) -> MenuEntry {
    MenuEntry {
        label: label.into(),
        action: id(action_id),
    }
}

/// (e) Menu before a Link: only what an unverified Student may do.
pub fn menu_locked() -> Menu {
    Menu::new(vec![
        entry("ผูกบัญชีนักศึกษา", "link.start"),
        entry("ติดต่อเจ้าหน้าที่", "inbox.open"),
    ])
}

/// (e) Menu after a Link: 4 entries.
pub fn menu_unlocked() -> Menu {
    Menu::new(vec![
        entry("ประกาศล่าสุด", "news.latest"),
        entry("ติดต่อเจ้าหน้าที่", "inbox.open"),
        entry("คำถามที่พบบ่อย", "faq.list"),
        entry("ยกเลิกการเชื่อมต่อ", "link.revoke"),
    ])
}

// ── Degrade samples: not in the first slice, they exercise the rules. ──

/// Over both channels' text limits (LINE 5000, Telegram 4096).
pub fn degrade_long_text() -> Message {
    let para = "ระเบียบการแต่งกายของนักศึกษาให้เป็นไปตามประกาศของมหาวิทยาลัย ";
    Message::text(para.repeat(100))
}

/// 15 options: over LINE's 13 quick reply items; labels over 20 chars.
pub fn degrade_choices_15() -> Message {
    Message::Choices(Choices {
        text: "เลือกวิชาที่ต้องการดูตารางสอบ".into(),
        options: (1..=15)
            .map(|i| action(&format!("วิชาเลือกเสรีกลุ่มที่ {i:02}"), &format!("exam.pick.{i}")))
            .collect(),
    })
}

/// 120 options: over the 100-button inline keyboard ceiling.
pub fn degrade_choices_120() -> Message {
    Message::Choices(Choices {
        text: "เลือกห้อง".into(),
        options: (1..=120)
            .map(|i| action(&format!("ห้อง {i}"), &format!("room.{i}")))
            .collect(),
    })
}

/// A Card whose labels exceed LINE's 40-char Flex button label.
pub fn degrade_long_labels() -> Message {
    Message::Card(Card {
        image: None,
        title: "แบบสอบถาม".into(),
        body: "กรุณาเลือกหนึ่งข้อ".into(),
        buttons: vec![action(
            &"ยอมรับเงื่อนไขการใช้ข้อมูลส่วนบุคคลฉบับปรับปรุง ".repeat(2),
            "consent.accept",
        )],
    })
}

/// Fixture `degrade_pdf_30mb` (see `docs/prototypes/message-render/`).
pub fn degrade_pdf_30mb() -> Message {
    Message::Attachment(Attachment {
        file_key: "files/2026/calendar.pdf".into(),
        file_name: "ปฏิทินการศึกษา 2569.pdf".into(),
        content_type: "application/pdf".into(),
        size_bytes: 30 * 1_048_576,
        url: "https://files.sealbay.example/s/calendar.pdf?sig=x".into(),
    })
}

/// Fixture `degrade_image_3mb` (see `docs/prototypes/message-render/`).
pub fn degrade_image_3mb() -> Message {
    Message::Attachment(Attachment {
        file_key: "files/2026/map.png".into(),
        file_name: "แผนที่อาคาร.png".into(),
        content_type: "image/png".into(),
        size_bytes: 3 * 1_048_576,
        url: "https://files.sealbay.example/s/map.png?sig=x".into(),
    })
}

/// Every sample, by the file name its JSON is written under.
pub fn all() -> Vec<(&'static str, Sample)> {
    use Sample::{Menu as Mn, Message as M};
    vec![
        ("a_news_card", M(news_card())),
        ("b1_otp_ask_student_id", M(otp_ask_student_id())),
        ("b2_otp_ask_code", M(otp_ask_code())),
        ("b3_otp_wrong_code", M(otp_wrong_code())),
        ("b4_otp_locked", M(otp_locked())),
        ("b5_otp_success", M(otp_success())),
        ("c_unlink_confirm", M(unlink_confirm())),
        ("d1_news_list_3", M(news_list_3())),
        ("d2_news_list_15", M(news_list_15())),
        ("e1_menu_locked", Mn(menu_locked())),
        ("e2_menu_unlocked", Mn(menu_unlocked())),
        ("x_long_text", M(degrade_long_text())),
        ("x_choices_15", M(degrade_choices_15())),
        ("x_choices_120", M(degrade_choices_120())),
        ("x_long_labels", M(degrade_long_labels())),
        ("x_pdf_30mb", M(degrade_pdf_30mb())),
        ("x_image_3mb", M(degrade_image_3mb())),
    ]
}

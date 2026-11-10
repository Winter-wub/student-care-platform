//! One word per idea on Staff pages (#157), over the example's whole
//! catalog: the Kernel's, every Module's and the example's own texts,
//! after `[texts]`. GLOSSARY.md names a Link "การผูกบัญชี", so every Thai
//! text Staff read says ผูก, never เชื่อม. Texts a Student reads in the
//! chat or the Mini App are left out by prefix, so a new Staff text file
//! is checked without being listed here.

use example_deployment::{Secrets, builder};
use harpseal::{Catalog, Locale};
use harpseal_testing::Harness;

fn secrets() -> Secrets {
    Secrets::from_lookup(|k| match k {
        "LINE_LIFF_ID" => Some("1234567890-AbCdEfGh".into()),
        "TELEGRAM_WEB_APP_URL" => Some("https://app.seal-bay.invalid/tg".into()),
        _ => None,
    })
}

fn catalog() -> Catalog {
    let h = Harness::new();
    let d = builder(h.services(), secrets())
        .unwrap()
        .channel(h.fake_channel())
        .build()
        .expect("the example Deployment builds");
    d.catalog().clone()
}

/// Texts a Student reads (chat, Menu, Mini App, privacy notice, OTP).
const STUDENT_FACING: [&str; 9] = [
    "kernel.link.",
    "kernel.notice.",
    "kernel.flow.",
    "kernel.menu.",
    "kernel.app.",
    "kernel.otp.",
    "kernel.action.",
    "kernel.inbound.",
    "kernel.consent.",
];

/// The verb GLOSSARY.md does not use for a Link.
const OTHER_VERB: &str = "เชื่อม";

#[test]
fn staff_texts_use_one_link_verb() {
    let c = catalog();
    let mut hits: Vec<String> = c
        .entries()
        .filter(|(l, k, t)| {
            *l == "th" && t.contains(OTHER_VERB) && !STUDENT_FACING.iter().any(|p| k.starts_with(p))
        })
        .map(|(_, k, t)| format!("{k}: {t}"))
        .collect();
    hits.sort();
    assert!(hits.is_empty(), "say ผูก for a Link:\n{}", hits.join("\n"));
    // The check sees the texts it is meant to: Staff ones say ผูก.
    let th = Locale::th();
    assert_eq!(c.t(&th, "kernel.students.link_none"), "ยังไม่ผูก");
}

#[test]
fn only_the_final_review_buttons_say_send() {
    let c = catalog();
    let th = Locale::th();
    for key in ["announcements.form.confirm", "announcements.action.confirm"] {
        assert_eq!(c.t(&th, key), "ตรวจก่อนส่ง", "{key} opens the review");
    }
    assert_eq!(c.t(&th, "announcements.review.confirm"), "ยืนยันส่ง");
    assert_eq!(c.t(&th, "reminders.send"), "ตรวจก่อนส่งเตือน");
    assert!(c.t(&th, "reminders.confirm").starts_with("ยืนยันส่งเตือน"));
    assert_eq!(c.tag_name(&th, "fee_due"), "ค้างชำระค่าธรรมเนียม");
}

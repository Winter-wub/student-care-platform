//! The reminder itself, in Thai and English, and the link to the
//! Student's fees page its email carries (#163): the Mini App URL of
//! that page in LINE, built by the Core for a Job or a page, behind the
//! switch [`EMAIL_LINK`] (a Boolean Module setting, on by default).

use harpseal::{CatalogEntry, Locale, Message, Notice, Result, Settings};

/// The switch: whether the reminder email links to the fees page.
pub const EMAIL_LINK: &str = "reminders.email_link";

/// The fees page the email links to, opened in LINE: a Telegram Web App
/// URL opens nothing outside Telegram.
pub(super) const FEES_PAGE: (&str, &str) = ("line", crate::fees::student::PATH);

/// The Thai reminder: the email subject and the text.
pub(super) const NOTICE_TH: (&str, &str) = (
    "เตือนชำระค่าธรรมเนียม",
    "มหาวิทยาลัยซีลเบย์: กรุณาชำระค่าธรรมเนียมการศึกษาภายในวันศุกร์นี้",
);

/// The switch's label and help on `/admin/settings`.
pub(super) const TEXTS: &[CatalogEntry] = &[
    CatalogEntry::new(
        "reminders.email_link_label",
        "th",
        "ใส่ลิงก์หน้าค่าธรรมเนียมในอีเมลเตือน",
    ),
    CatalogEntry::new(
        "reminders.email_link_label",
        "en",
        "Link the fees page in the reminder email",
    ),
    CatalogEntry::new(
        "reminders.email_link_help",
        "th",
        "เปิดไว้ อีเมลเตือนจะมีลิงก์ให้นักศึกษาเปิดดูยอดที่ต้องชำระใน LINE",
    ),
    CatalogEntry::new(
        "reminders.email_link_help",
        "en",
        "When on, the reminder email links to the Student's fees page in LINE.",
    ),
];

/// The reminder, in Thai and English, under `key`. `fees_url` is the
/// Mini App URL of the Student's fees page ([`fees_url`]): the email
/// links to it; a chat message needs no URL.
pub fn notice(key: &str, fees_url: Option<&str>) -> Notice {
    let (subject, th) = NOTICE_TH;
    let en = "Seal Bay University: please pay your tuition fee by this Friday.";
    let mail = |text: &str, see: &str| match fees_url {
        Some(url) => format!("{text}\n\n{see}: {url}"),
        None => text.to_string(),
    };
    let th_mail = mail(th, "ดูยอดที่ต้องชำระใน LINE");
    let en_mail = mail(en, "See what you owe in LINE");
    Notice::new("reminders", key)
        .locale(Locale::th(), vec![Message::text(th)], subject, th_mail)
        .locale(
            Locale::en(),
            vec![Message::text(en)],
            "Fee reminder",
            en_mail,
        )
}

/// `url` (from [`harpseal::JobContext::mini_app_url`] or
/// [`harpseal::PageState::mini_app_url`] for the fees page) while the
/// switch [`EMAIL_LINK`] is on, else `None`.
pub async fn fees_url(settings: &Settings, url: Option<String>) -> Result<Option<String>> {
    Ok(if settings.bool(EMAIL_LINK).await? {
        url
    } else {
        None
    })
}

//! The Fees Module's status card on the back-office home (#123), through
//! [`harpseal::ModuleRegistrar::status_card`]: how many Students still
//! owe, and when the weekly reminder ([`crate::reminders`]) goes next.
//! One query; the Core runs it only on the home, and only for Staff
//! holding [`super::IMPORT`].

use harpseal::{
    AppState, CardState, CatalogEntry as E, CurrentStaff, Result, Statement, StatusCard, Value,
};

/// After the Kernel's cards (10 to 40).
pub const ORDER: u16 = 50;

pub(super) const TEXTS: &[E] = &[
    E::new("fees.home.title", "th", "ค่าธรรมเนียม"),
    E::new("fees.home.title", "en", "Fees"),
    E::new("fees.home.empty", "th", "ยังไม่ได้นำเข้ายอดค้างชำระ"),
    E::new("fees.home.empty", "en", "No fee file imported yet."),
    E::new("fees.home.import", "th", "นำเข้าไฟล์ค่าธรรมเนียม"),
    E::new("fees.home.import", "en", "Import the fee file"),
    E::new("fees.home.clear", "th", "ไม่มีนักศึกษาค้างชำระ"),
    E::new("fees.home.clear", "en", "No Student owes fees."),
    // {n}: the weekly Job `reminders.weekly` runs "0 9 * * 1".
    E::new(
        "fees.home.owing",
        "th",
        "{n} คนค้างชำระ · เตือนอัตโนมัติรอบถัดไป วันจันทร์ 09:00",
    ),
    E::new(
        "fees.home.owing",
        "en",
        "{n} Students owe fees · next reminder Monday 09:00",
    ),
    E::new("fees.home.remind", "th", "ส่งเตือนตอนนี้"),
    E::new("fees.home.remind", "en", "Send a reminder now"),
    E::new("fees.home.open", "th", "ดูยอดค้างชำระ"),
    E::new("fees.home.open", "en", "Open fees"),
];

/// The card: empty before the first import, warn while anyone owes.
pub async fn card(s: AppState, staff: CurrentStaff) -> Result<StatusCard> {
    let rows = s
        .services
        .db
        .query(Statement::new(
            "SELECT (SELECT count(*) FROM fees_balance WHERE amount_satang > 0) AS owing, \
             (SELECT count(*) FROM fees_import) AS imports",
        ))
        .await?;
    let n = |c: &str| rows.get(0, c).and_then(Value::as_i64).unwrap_or(0);
    let t = |k: &str| s.catalog.t(&staff.locale, k);
    let title = t("fees.home.title");
    if n("imports") == 0 {
        let card = StatusCard::new(CardState::Empty, title, t("fees.home.empty"));
        return Ok(card.action(t("fees.home.import"), super::BASE));
    }
    let owing = n("owing");
    if owing == 0 {
        let card = StatusCard::new(CardState::Ok, title, t("fees.home.clear"));
        return Ok(card.action(t("fees.home.import"), super::BASE));
    }
    let fact = t("fees.home.owing").replace("{n}", &owing.to_string());
    let card = StatusCard::new(CardState::Warn, title, fact);
    Ok(if staff.can(crate::reminders::PERMISSION.id) {
        card.action(t("fees.home.remind"), "/admin/reminders")
    } else {
        card.action(t("fees.home.open"), super::BASE)
    })
}

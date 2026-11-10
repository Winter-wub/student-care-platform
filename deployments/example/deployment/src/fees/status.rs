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
    // {n} {date} {time}: the next run of `reminders.weekly` (Mondays).
    E::new(
        "fees.home.owing",
        "th",
        "{n} คนค้างชำระ · เตือนอัตโนมัติรอบถัดไป วันจันทร์ {date} {time}",
    ),
    E::new(
        "fees.home.owing",
        "en",
        "{n} Students owe fees · next reminder Monday {date} {time}",
    ),
    E::new("fees.home.remind", "th", "เลือกนักศึกษาที่จะเตือน"),
    E::new("fees.home.remind", "en", "Choose Students to remind"),
    E::new("fees.home.open", "th", "ดูยอดค้างชำระ"),
    E::new("fees.home.open", "en", "Open fees"),
];

const DAY_MS: i64 = 86_400_000;

/// The next run of [`crate::reminders::SCHEDULE`] after `now_ms`, local at
/// `offset_minutes`: (`YYYY-MM-DD`, `HH:00`).
pub fn next_reminder(now_ms: i64, offset_minutes: i32) -> (String, String) {
    let hour = crate::reminders::HOUR;
    let local = now_ms + i64::from(offset_minutes) * 60_000;
    let (day, ms) = (local.div_euclid(DAY_MS), local.rem_euclid(DAY_MS));
    // Day 0 (1970-01-01) was a Thursday: Monday is 0 here.
    let weekday = (day + 3).rem_euclid(7);
    let mut ahead = (7 - weekday) % 7;
    if ahead == 0 && ms >= hour * 3_600_000 {
        ahead = 7;
    }
    (civil(day + ahead), format!("{hour:02}:00"))
}

/// `days` since 1970-01-01 as `YYYY-MM-DD` (Howard Hinnant's algorithm).
fn civil(days: i64) -> String {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02}")
}

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
    let tf = |k: &str, args: &[(&str, &str)]| s.catalog.tf(&staff.locale, k, args);
    let t = |k: &str| tf(k, &[]);
    if n("imports") == 0 {
        let card = StatusCard::new(CardState::Empty, t("fees.home.empty"));
        return Ok(card.action(t("fees.home.import"), super::BASE));
    }
    let owing = n("owing");
    if owing == 0 {
        let card = StatusCard::new(CardState::Ok, t("fees.home.clear"));
        return Ok(card.action(t("fees.home.import"), super::BASE));
    }
    let offset = s.config.utc_offset_minutes()?;
    let (date, time) = next_reminder(s.services.now_ms(), offset);
    // Staff read `16 พ.ย. 2569` and `1,250` (#125).
    let date = harpseal::ui::thai_day(&date);
    let args = [
        ("n", harpseal::ui::number(owing)),
        ("date", date),
        ("time", time),
    ];
    let args: Vec<(&str, &str)> = args.iter().map(|(k, v)| (*k, v.as_str())).collect();
    let card = StatusCard::new(CardState::Warn, tf("fees.home.owing", &args));
    Ok(if staff.can(crate::reminders::PERMISSION.id) {
        card.action(t("fees.home.remind"), "/admin/reminders")
    } else {
        card.action(t("fees.home.open"), super::BASE)
    })
}

#[cfg(test)]
mod tests {
    use super::next_reminder;

    #[test]
    fn the_next_monday_at_nine_local() {
        // 2026-11-05T00:00:00Z is Thursday 07:00 in +07:00.
        let thu = 1_793_836_800_000;
        assert_eq!(
            next_reminder(thu, 420),
            ("2026-11-09".into(), "09:00".into())
        );
        // Monday 08:59 local: this morning; 09:00: next week.
        let mon_0859 = thu + 4 * 86_400_000 + (119 * 60_000);
        assert_eq!(next_reminder(mon_0859, 420).0, "2026-11-09");
        assert_eq!(next_reminder(mon_0859 + 60_000, 420).0, "2026-11-16");
    }
}

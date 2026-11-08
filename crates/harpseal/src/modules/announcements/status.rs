//! The announcements' status card on the term status home (#123):
//! what is scheduled, sending and drafted, when the last one went out, and
//! a warning when a schedule is overdue.
//! One query, for Staff holding `announcements.view`.

use crate::kernel::students::view::format_ms;
use crate::manifest::CatalogEntry as E;
use crate::port_types::{Statement, Value};
use crate::{AppState, CardState, CurrentStaff, Result, StatusCard};

/// Who sees the card.
pub(super) const PERMISSION: &str = "announcements.view";
/// The card's title.
pub(super) const TITLE: &str = "announcements.home.title";
/// After the roster (10), before the quota (30).
pub(super) const ORDER: u16 = 20;

pub(super) const TEXTS: &[E] = &[
    E::new("announcements.home.title", "th", "ประกาศ"),
    E::new("announcements.home.title", "en", "Announcements"),
    E::new("announcements.home.empty", "th", "ยังไม่มีประกาศ"),
    E::new("announcements.home.empty", "en", "No announcements yet."),
    // {n} {time}
    E::new(
        "announcements.home.scheduled",
        "th",
        "ตั้งเวลาไว้ {n} ฉบับ ฉบับถัดไป {time}",
    ),
    E::new(
        "announcements.home.scheduled",
        "en",
        "{n} scheduled, the next {time}",
    ),
    E::new("announcements.home.sending", "th", "กำลังส่ง {n} ฉบับ"),
    E::new("announcements.home.sending", "en", "{n} sending"),
    E::new("announcements.home.sent", "th", "ส่งล่าสุด {time}"),
    E::new("announcements.home.sent", "en", "Last sent {time}"),
    E::new("announcements.home.drafts", "th", "ร่างที่ยังไม่ได้ส่ง {n} ฉบับ"),
    E::new("announcements.home.drafts", "en", "{n} drafts not sent"),
    // {time}: a schedule more than 15 minutes past that has not gone out.
    E::new(
        "announcements.home.overdue",
        "th",
        "ประกาศที่ตั้งเวลาไว้ {time} ยังไม่ได้ส่ง",
    ),
    E::new(
        "announcements.home.overdue",
        "en",
        "The announcement scheduled for {time} has not gone out",
    ),
    E::new("announcements.home.write", "th", "เขียนประกาศ"),
    E::new("announcements.home.write", "en", "Write an announcement"),
    E::new("announcements.home.open", "th", "ดูประกาศ"),
    E::new("announcements.home.open", "en", "Open announcements"),
];

/// A schedule this late that has not gone out is a warning (the send Job
/// is not running).
const OVERDUE_MS: i64 = 15 * 60_000;

/// Empty before the first announcement; Ok with what is scheduled, sent
/// and drafted; Warn when a schedule is overdue.
pub(super) async fn card(s: AppState, staff: CurrentStaff) -> Result<StatusCard> {
    let rows = s
        .services
        .db
        .query(Statement::new(
            "SELECT \
             (SELECT count(*) FROM announcement WHERE status = 'scheduled') AS scheduled, \
             (SELECT min(send_at) FROM announcement WHERE status = 'scheduled') AS next_at, \
             (SELECT count(*) FROM announcement WHERE status = 'sending') AS sending, \
             (SELECT max(sent_at) FROM announcement WHERE sent_at IS NOT NULL) AS last_sent, \
             (SELECT count(*) FROM announcement WHERE status = 'draft') AS drafts",
        ))
        .await?;
    let int = |c: &str| rows.get(0, c).and_then(Value::as_i64);
    let n = |c: &str| int(c).unwrap_or(0);
    let tf = |k: &str, args: &[(&str, &str)]| s.catalog.tf(&staff.locale, k, args);
    let t = |k: &str| tf(k, &[]);
    let time = |ms: i64| format_ms(ms, s.config.utc_offset_minutes().unwrap_or(0));
    let action = if staff.can("announcements.send") {
        (t("announcements.home.write"), "/admin/announcements/new")
    } else {
        (t("announcements.home.open"), "/admin/announcements")
    };
    let overdue = int("next_at").filter(|at| *at < s.services.now_ms() - OVERDUE_MS);
    let mut facts = Vec::new();
    if let Some(at) = overdue {
        facts.push(tf("announcements.home.overdue", &[("time", &time(at))]));
    } else if n("scheduled") > 0 {
        let (count, next) = (n("scheduled").to_string(), time(n("next_at")));
        facts.push(tf(
            "announcements.home.scheduled",
            &[("n", &count), ("time", &next)],
        ));
    }
    for (col, key) in [
        ("sending", "announcements.home.sending"),
        ("drafts", "announcements.home.drafts"),
    ] {
        if n(col) > 0 {
            facts.push(tf(key, &[("n", &n(col).to_string())]));
        }
    }
    if let Some(at) = int("last_sent") {
        facts.push(tf("announcements.home.sent", &[("time", &time(at))]));
    }
    if facts.is_empty() {
        let card = StatusCard::new(CardState::Empty, t("announcements.home.empty"));
        return Ok(card.action(action.0, action.1));
    }
    let fact = facts.join(" · ");
    Ok(match overdue {
        Some(_) => StatusCard::new(CardState::Warn, fact)
            .action(t("announcements.home.open"), "/admin/announcements"),
        None => StatusCard::new(CardState::Ok, fact).action(action.0, action.1),
    })
}

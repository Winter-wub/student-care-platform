//! The announcements' status card on the term status home (#123):
//! what is scheduled, when the last one went out, and drafts not sent.
//! One query, for Staff holding `announcements.view`.

use crate::kernel::students::view::format_ms;
use crate::kernel::text::fill;
use crate::manifest::CatalogEntry as E;
use crate::port_types::{Statement, Value};
use crate::{AppState, CardState, CurrentStaff, Result, StatusCard};

/// Who sees the card.
pub(super) const PERMISSION: &str = "announcements.view";
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
    E::new("announcements.home.write", "th", "เขียนประกาศ"),
    E::new("announcements.home.write", "en", "Write an announcement"),
    E::new("announcements.home.open", "th", "ดูประกาศ"),
    E::new("announcements.home.open", "en", "Open announcements"),
];

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
    let t = |k: &str| s.catalog.t(&staff.locale, k);
    let time = |ms: i64| format_ms(ms, s.config.utc_offset_minutes().unwrap_or(0));
    let title = t("announcements.home.title");
    let (write, open) = (t("announcements.home.write"), t("announcements.home.open"));
    let action = if staff.can("announcements.send") {
        (write, "/admin/announcements/new")
    } else {
        (open.clone(), "/admin/announcements")
    };
    let mut facts = Vec::new();
    if n("scheduled") > 0 {
        let (count, next) = (n("scheduled").to_string(), time(n("next_at")));
        let text = t("announcements.home.scheduled");
        facts.push(fill(&text, &[("n", &count), ("time", &next)]));
    }
    if n("sending") > 0 {
        let count = n("sending").to_string();
        facts.push(fill(&t("announcements.home.sending"), &[("n", &count)]));
    }
    if let Some(at) = int("last_sent") {
        facts.push(fill(&t("announcements.home.sent"), &[("time", &time(at))]));
    }
    let drafts = n("drafts");
    if drafts > 0 {
        let count = drafts.to_string();
        facts.push(fill(&t("announcements.home.drafts"), &[("n", &count)]));
    }
    if facts.is_empty() {
        let card = StatusCard::new(CardState::Empty, title, t("announcements.home.empty"));
        return Ok(card.action(action.0, action.1));
    }
    let fact = facts.join(" · ");
    Ok(if drafts > 0 {
        StatusCard::new(CardState::Warn, title, fact).action(open, "/admin/announcements")
    } else {
        StatusCard::new(CardState::Ok, title, fact).action(action.0, action.1)
    })
}

//! The chat side: the announcement Card, the ack Action
//! (`announcements.ack.<id>`) and the "ข่าวล่าสุด" Menu entry.

use super::store::{self, Summary, Texts};
use super::{ACK_PREFIX, MINI_APP_PATH};
use crate::manifest::Catalog;
use crate::message::{ActionId, Button, Card, CardList, Locale, Message};
use crate::module::ActionContext;
use crate::port_types::Statement;
use crate::{Deployment, Result, Services};

/// Announcements the "ข่าวล่าสุด" Menu entry lists.
pub(crate) const LATEST_LIMIT: i64 = 5;

/// The ack Action of announcement `id` (≤ 64 bytes for a UUIDv7 id).
pub fn ack_action(id: &str) -> Result<ActionId> {
    ActionId::new(format!("{ACK_PREFIX}{id}"))
}

/// The Mini App page of announcement `id`.
pub fn detail_path(id: &str) -> String {
    format!("{MINI_APP_PATH}/{id}")
}

/// The Card of announcement `id` in `locale` (Thai fallback): title, body,
/// "รับทราบ" when `ack`, "เปิดดูรายละเอียด".
pub(crate) fn card(
    catalog: &Catalog,
    locale: &Locale,
    id: &str,
    texts: &Texts,
    ack: bool,
) -> Result<Card> {
    let (title, body) = texts.pick(locale);
    let mut buttons = Vec::new();
    if ack {
        buttons.push(Button::action(
            catalog.t(locale, "announcements.card.ack"),
            ack_action(id)?,
        ));
    }
    buttons.push(Button::mini_app(
        catalog.t(locale, "announcements.card.open"),
        detail_path(id),
    ));
    Ok(Card {
        title: title.to_string(),
        body: body.to_string(),
        buttons,
    })
}

/// What an acknowledgement did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum AckOutcome {
    /// Recorded now (the first time).
    Recorded,
    /// The Student had already acknowledged it.
    Already,
    /// The announcement was retracted: nothing recorded. Only for a
    /// Student with a delivery row (otherwise [`AckOutcome::Unknown`]).
    Retracted,
    /// No such announcement, or the Student is not in its Audience.
    Unknown,
}

/// Records Student `student`'s acknowledgement of announcement `id`:
/// `acked_at = now` on every delivery row of that Student (every Channel,
/// every round) still without one, in one conditional statement, so it is
/// idempotent and two taps racing record it once. It is once per Student:
/// after an acknowledgement (any round), a later tap is
/// [`AckOutcome::Already`]. Expired announcements
/// still accept it; retracted ones do not. For the chat Action and the
/// Mini App's ack endpoint.
pub async fn ack(d: &Deployment, id: &str, student: &str) -> Result<AckOutcome> {
    ack_with(d.services(), id, student).await
}

pub(crate) async fn ack_with(s: &Services, id: &str, student: &str) -> Result<AckOutcome> {
    let Some(row) = store::load(s, id).await? else {
        return Ok(AckOutcome::Unknown);
    };
    // The delivery row first: a Student outside the Audience learns
    // nothing, not even that a retracted announcement exists (#49).
    let rows =
        s.db.query(
            Statement::new(
                "SELECT count(*) AS n, max(acked_at) AS acked FROM announcement_delivery \
                 WHERE announcement_id = ? AND student_id = ?",
            )
            .bind(id)
            .bind(student),
        )
        .await?;
    if store::int(&rows, 0, "n").unwrap_or(0) == 0 {
        return Ok(AckOutcome::Unknown);
    }
    if row.status == "retracted" {
        return Ok(AckOutcome::Retracted);
    }
    // Once per Student: an earlier acknowledgement (any round, any
    // Channel) stands, and covers rows added since (a resend).
    let earlier = store::int(&rows, 0, "acked");
    let done =
        s.db.execute(
            Statement::new(
                "UPDATE announcement_delivery SET acked_at = ? WHERE announcement_id = ? \
                 AND student_id = ? AND acked_at IS NULL \
                 AND EXISTS (SELECT 1 FROM announcement WHERE id = ? AND status <> 'retracted')",
            )
            .bind(earlier.unwrap_or_else(|| s.now_ms()))
            .bind(id)
            .bind(student)
            .bind(id),
        )
        .await?;
    Ok(if earlier.is_none() && done.rows_affected > 0 {
        AckOutcome::Recorded
    } else {
        AckOutcome::Already
    })
}

/// `announcements.ack.<id>`: one Text, by outcome.
pub(crate) async fn on_ack(cx: ActionContext) -> Result<Vec<Message>> {
    let id = cx
        .action
        .as_str()
        .strip_prefix(ACK_PREFIX)
        .unwrap_or_default();
    let key = if id.len() != 36 {
        "announcements.ack.unknown"
    } else {
        match ack_with(&cx.services, id, &cx.student.0).await? {
            AckOutcome::Recorded => "announcements.ack.thanks",
            AckOutcome::Already => "announcements.ack.again",
            AckOutcome::Retracted => "announcements.ack.retracted",
            AckOutcome::Unknown => "announcements.ack.unknown",
        }
    };
    Ok(vec![Message::text(cx.t(key))])
}

/// The `limit` newest announcements Student `student` was sent (the Menu
/// entry shows 5), unexpired and not retracted, the ones waiting for an
/// acknowledgement first; texts in `locale` (Thai fallback). The Menu
/// entry's query; the Mini App lists with it too.
pub async fn latest(
    d: &Deployment,
    student: &str,
    locale: &Locale,
    limit: i64,
) -> Result<Vec<Summary>> {
    let s = d.services();
    store::for_student(s, student, locale, s.now_ms(), limit).await
}

/// `announcements.latest`: a Card List of [`latest`] with "รับทราบ" on
/// those waiting for it and "ดูทั้งหมด" on the last Card, or a Text when
/// there is none.
pub(crate) async fn on_latest(cx: ActionContext) -> Result<Vec<Message>> {
    let s = &cx.services;
    let list = store::for_student(s, &cx.student.0, &cx.locale, s.now_ms(), LATEST_LIMIT).await?;
    if list.is_empty() {
        return Ok(vec![Message::text(cx.t("announcements.latest.empty"))]);
    }
    let mut cards = Vec::with_capacity(list.len());
    for a in &list {
        let mut buttons = Vec::new();
        if a.needs_ack() {
            buttons.push(Button::action(
                cx.t("announcements.card.ack"),
                ack_action(&a.id)?,
            ));
        }
        buttons.push(Button::mini_app(
            cx.t("announcements.card.open"),
            detail_path(&a.id),
        ));
        cards.push(Card {
            title: a.title.clone(),
            body: a.body.clone(),
            buttons,
        });
    }
    if let Some(last) = cards.last_mut() {
        last.buttons.push(Button::mini_app(
            cx.t("announcements.card.all"),
            MINI_APP_PATH,
        ));
    }
    Ok(vec![Message::CardList(CardList(cards))])
}

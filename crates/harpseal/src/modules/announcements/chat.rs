//! The chat side: the announcement Card, the ack Action
//! (`announcements.ack.<id>`) and the "ข่าวล่าสุด" Menu entry.

use super::files::{Stored, hero, pdfs};
use super::store::{self, Summary, Texts};
use super::{ACK_PREFIX, MINI_APP_PATH};
use crate::config::PublicUrl;
use crate::manifest::Catalog;
use crate::message::{ActionId, Button, Card, CardImage, CardList, Locale, Message};
use crate::module::ActionContext;
use crate::port_types::Statement;
use crate::{Deployment, Result, Services};

/// The ack Action of announcement `id` (≤ 64 bytes for a UUIDv7 id).
pub fn ack_action(id: &str) -> Result<ActionId> {
    ActionId::new(format!("{ACK_PREFIX}{id}"))
}

/// The Mini App page of announcement `id`.
pub fn detail_path(id: &str) -> String {
    format!("{MINI_APP_PATH}/{id}")
}

/// Most URL buttons a pushed Card carries (#108).
pub(crate) const URL_BUTTONS: usize = 2;

/// The Card of announcement `id` in `locale` (Thai fallback): title, the
/// body as plain text ([`crate::rich_text`]: markup removed, link text
/// kept), "รับทราบ" when `ack`, "เปิดดูรายละเอียด", then a URL button for
/// each of the body's first [`URL_BUTTONS`] distinct links, labelled with
/// the link text (each adapter cuts it to its limit). With `public_url`
/// set, the image is the first one the body places, else the lowest slot
/// (#109; [`super::files::hero`]); without it, none. With `public_url`
/// set and a PDF attached (#113), the first PDF takes the last of the
/// [`URL_BUTTONS`]: a URL button labelled with its name, after the links,
/// marked [`Button::external`] (LINE opens it in the phone's browser).
/// A PDF URL the button refuses leaves the button out, never the Card.
/// `files` holds every kind.
pub(crate) fn card(
    catalog: &Catalog,
    locale: &Locale,
    id: &str,
    texts: &Texts,
    ack: bool,
    images: &[Stored],
    public_url: Option<&PublicUrl>,
) -> Result<Card> {
    let (title, body) = texts.pick(locale);
    let doc = crate::rich_text::parse(body);
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
    // The PDF button first, so a refused one gives its place back to a link.
    let pdf = public_url
        .zip(pdfs(images).next())
        .and_then(|(base, f)| Button::url(f.file_name.clone(), base.join(&f.public_path())).ok())
        .map(Button::external);
    let links = URL_BUTTONS - usize::from(pdf.is_some());
    for link in doc.first_links(links) {
        buttons.push(Button::url(link.text, link.url)?);
    }
    buttons.extend(pdf);
    let image = match (public_url, hero(images, body)) {
        (Some(base), Some(f)) => Some(CardImage::new(base.join(&f.public_path()), title)?),
        _ => None,
    };
    Ok(Card {
        image,
        title: title.to_string(),
        body: doc.plain(),
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
    /// The announcement expired (`expires_at` passed): nothing recorded,
    /// the Student is told it is no longer available (#77). Only for a
    /// Student with a delivery row.
    Expired,
    /// No such announcement, or the Student is not in its Audience.
    Unknown,
}

/// Records Student `student`'s acknowledgement of announcement `id`:
/// `acked_at = now` on every delivery row of that Student (every Channel,
/// every round) still without one, in one conditional statement, so it is
/// idempotent and two taps racing record it once. It is once per Student:
/// after an acknowledgement (any round), a later tap is
/// [`AckOutcome::Already`]. Retracted and expired announcements refuse
/// it ([`AckOutcome::Retracted`], [`AckOutcome::Expired`]; #77: before,
/// an expired one still took it). For the chat Action and the Mini App's
/// ack endpoint.
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
    let now = s.now_ms();
    if row.expires_at.is_some_and(|e| e <= now) {
        return Ok(AckOutcome::Expired);
    }
    // Once per Student: an earlier acknowledgement (any round, any
    // Channel) stands, and covers rows added since (a resend).
    let earlier = store::int(&rows, 0, "acked");
    let done =
        s.db.execute(
            Statement::new(
                "UPDATE announcement_delivery SET acked_at = ? WHERE announcement_id = ? \
                 AND student_id = ? AND acked_at IS NULL \
                 AND EXISTS (SELECT 1 FROM announcement WHERE id = ? AND status <> 'retracted' \
                 AND (expires_at IS NULL OR expires_at > ?))",
            )
            .bind(earlier.unwrap_or(now))
            .bind(id)
            .bind(student)
            .bind(id)
            .bind(now),
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
            AckOutcome::Expired => "announcements.ack.expired",
            AckOutcome::Unknown => "announcements.ack.unknown",
        }
    };
    Ok(vec![Message::text(cx.t(key))])
}

/// The `limit` newest announcements Student `student` was sent (the Menu
/// entry shows `[announcements] latest`, default 5), unexpired and not retracted, the ones waiting for an
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

/// `announcements.latest`: a Card List of the `[announcements] latest`
/// newest ([`latest`]) with "รับทราบ" on
/// those waiting for it and "ดูทั้งหมด" on the last Card, or a Text when
/// there is none.
pub(crate) async fn on_latest(cx: ActionContext) -> Result<Vec<Message>> {
    let s = &cx.services;
    let limit = i64::from(cx.config.announcements.latest);
    let list = store::for_student(s, &cx.student.0, &cx.locale, s.now_ms(), limit).await?;
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
            image: None,
            title: a.title.clone(),
            // Plain text (#108): markup removed, link text kept.
            body: crate::rich_text::plain(&a.body),
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

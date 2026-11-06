//! Reads of the Module's tables: one announcement, its texts, the list
//! page, a Student's latest announcements and the stats.

use std::collections::BTreeMap;

use serde_json::Value as Json;

use crate::audience::Audience;
use crate::message::Locale;
use crate::port_types::{Rows, Statement, Value};
use crate::{Result, Services};

/// One `announcement` row.
#[derive(Debug, Clone)]
pub(crate) struct Row {
    pub(crate) id: String,
    pub(crate) status: String,
    pub(crate) audience: Audience,
    pub(crate) send_at: Option<i64>,
    pub(crate) expires_at: Option<i64>,
    pub(crate) require_ack: bool,
    pub(crate) created_at: i64,
    pub(crate) sent_at: Option<i64>,
    pub(crate) retracted_at: Option<i64>,
    pub(crate) round: i64,
    pub(crate) snapshot_round: i64,
    pub(crate) version: i64,
    pub(crate) edit_report: Option<Json>,
}

pub(crate) const COLUMNS: &str = "a.id, a.status, a.audience, a.send_at, a.expires_at, \
    a.require_ack, a.created_at, a.sent_at, a.retracted_at, a.round, a.snapshot_round, \
    a.version, a.edit_report";

pub(crate) fn text(rows: &Rows, i: usize, c: &str) -> String {
    rows.get(i, c)
        .and_then(Value::as_text)
        .unwrap_or_default()
        .to_string()
}

pub(crate) fn int(rows: &Rows, i: usize, c: &str) -> Option<i64> {
    rows.get(i, c).and_then(Value::as_i64)
}

pub(crate) fn row_at(rows: &Rows, i: usize) -> Row {
    Row {
        id: text(rows, i, "id"),
        status: text(rows, i, "status"),
        audience: serde_json::from_str(&text(rows, i, "audience")).unwrap_or_default(),
        send_at: int(rows, i, "send_at"),
        expires_at: int(rows, i, "expires_at"),
        require_ack: int(rows, i, "require_ack") == Some(1),
        created_at: int(rows, i, "created_at").unwrap_or(0),
        sent_at: int(rows, i, "sent_at"),
        retracted_at: int(rows, i, "retracted_at"),
        round: int(rows, i, "round").unwrap_or(0),
        snapshot_round: int(rows, i, "snapshot_round").unwrap_or(-1),
        version: int(rows, i, "version").unwrap_or(0),
        edit_report: rows
            .get(i, "edit_report")
            .and_then(Value::as_text)
            .and_then(|t| serde_json::from_str(t).ok()),
    }
}

/// The announcement `id`, if it exists.
pub(crate) async fn load(s: &Services, id: &str) -> Result<Option<Row>> {
    let sql = format!("SELECT {COLUMNS} FROM announcement a WHERE a.id = ?");
    let rows = s.db.query(Statement::new(sql).bind(id)).await?;
    Ok((!rows.is_empty()).then(|| row_at(&rows, 0)))
}

/// Title and body per Locale.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Texts(pub(crate) BTreeMap<String, (String, String)>);

impl Texts {
    /// The text in `locale`, else Thai, else any.
    pub(crate) fn pick(&self, locale: &Locale) -> (&str, &str) {
        let m = &self.0;
        m.get(locale.as_str())
            .or_else(|| m.get("th"))
            .or_else(|| m.values().next())
            .map_or(("", ""), |(t, b)| (t.as_str(), b.as_str()))
    }
    /// The Locale [`Texts::pick`] uses for `locale`.
    pub(crate) fn locale_for(&self, locale: &Locale) -> Locale {
        if self.0.contains_key(locale.as_str()) {
            locale.clone()
        } else {
            Locale::th()
        }
    }
}

/// The texts of `id`.
pub(crate) async fn texts(s: &Services, id: &str) -> Result<Texts> {
    let rows =
        s.db.query(
            Statement::new(
                "SELECT locale, title, body FROM announcement_text WHERE announcement_id = ?",
            )
            .bind(id),
        )
        .await?;
    Ok(Texts(
        (0..rows.len())
            .map(|i| {
                (
                    text(&rows, i, "locale"),
                    (text(&rows, i, "title"), text(&rows, i, "body")),
                )
            })
            .collect(),
    ))
}

/// One line of the list page.
#[derive(Debug, Clone)]
pub(crate) struct Listed {
    pub(crate) row: Row,
    pub(crate) title: String,
    pub(crate) students: i64,
    pub(crate) acked: i64,
}

/// Rows per list page.
pub(crate) const PAGE: i64 = 50;

/// Page `page` (1-based, clamped) of the newest announcements, and the
/// number of pages.
pub(crate) async fn list(s: &Services, page: i64) -> Result<(Vec<Listed>, i64, i64)> {
    let total =
        s.db.query(Statement::new("SELECT count(*) AS n FROM announcement"))
            .await?;
    let total = int(&total, 0, "n").unwrap_or(0);
    let pages = ((total + PAGE - 1) / PAGE).max(1);
    let page = page.clamp(1, pages);
    let sql = format!(
        "SELECT {COLUMNS}, \
         (SELECT title FROM announcement_text t WHERE t.announcement_id = a.id AND t.locale = 'th') AS title, \
         (SELECT count(DISTINCT student_id) FROM announcement_delivery d WHERE d.announcement_id = a.id) AS students, \
         (SELECT count(DISTINCT student_id) FROM announcement_delivery d WHERE d.announcement_id = a.id \
          AND d.acked_at IS NOT NULL) AS acked \
         FROM announcement a ORDER BY a.created_at DESC, a.id DESC LIMIT ? OFFSET ?"
    );
    let rows =
        s.db.query(Statement::new(sql).bind(PAGE).bind((page - 1) * PAGE))
            .await?;
    let out = (0..rows.len())
        .map(|i| Listed {
            row: row_at(&rows, i),
            title: text(&rows, i, "title"),
            students: int(&rows, i, "students").unwrap_or(0),
            acked: int(&rows, i, "acked").unwrap_or(0),
        })
        .collect();
    Ok((out, page, pages))
}

/// One announcement as a Student sees it in a list.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct Summary {
    /// `announcement.id`.
    pub id: String,
    /// Title in the Student's Locale (Thai fallback).
    pub title: String,
    /// Body in the Student's Locale (Thai fallback).
    pub body: String,
    /// The announcement asks for an acknowledgement.
    pub require_ack: bool,
    /// When the Student acknowledged it.
    pub acked_at: Option<i64>,
    /// When it was sent (the latest round finished), if it has finished.
    pub sent_at: Option<i64>,
}

impl Summary {
    /// Still waiting for this Student's acknowledgement.
    pub fn needs_ack(&self) -> bool {
        self.require_ack && self.acked_at.is_none()
    }
}

/// The newest `limit` announcements Student `student` has a delivery row
/// for (in its Audience at send time), not retracted, not expired at
/// `now`, then the ones waiting for an acknowledgement first. Texts in
/// `locale` with Thai fallback.
pub(crate) async fn for_student(
    s: &Services,
    student: &str,
    locale: &Locale,
    now: i64,
    limit: i64,
) -> Result<Vec<Summary>> {
    let rows =
        s.db.query(
            Statement::new(
                "SELECT a.id, a.require_ack, a.sent_at, \
                 (SELECT max(acked_at) FROM announcement_delivery x WHERE x.announcement_id = a.id \
                  AND x.student_id = ?) AS acked_at, \
                 coalesce(a.sent_at, a.send_at, a.created_at) AS at \
                 FROM announcement a WHERE a.status IN ('sending', 'sent') \
                 AND (a.expires_at IS NULL OR a.expires_at > ?) \
                 AND EXISTS (SELECT 1 FROM announcement_delivery d WHERE d.announcement_id = a.id \
                  AND d.student_id = ?) \
                 ORDER BY at DESC, a.id DESC LIMIT ?",
            )
            .bind(student)
            .bind(now)
            .bind(student)
            .bind(limit),
        )
        .await?;
    let mut out = Vec::with_capacity(rows.len());
    for i in 0..rows.len() {
        let id = text(&rows, i, "id");
        let t = texts(s, &id).await?;
        let (title, body) = t.pick(locale);
        out.push(Summary {
            title: title.to_string(),
            body: body.to_string(),
            require_ack: int(&rows, i, "require_ack") == Some(1),
            acked_at: int(&rows, i, "acked_at"),
            sent_at: int(&rows, i, "sent_at"),
            id,
        });
    }
    // Stable: newest first within each group.
    out.sort_by_key(|a| !a.needs_ack());
    Ok(out)
}

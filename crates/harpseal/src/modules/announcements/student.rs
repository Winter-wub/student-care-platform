//! Reads for the Mini App (#28): a Student's announcements and one of them,
//! always through the Student's delivery rows, so a page never shows (or
//! reveals the existence of) an announcement the Student was not sent.

use super::store::{self, int, text};
use crate::message::Locale;
use crate::port_types::Statement;
use crate::{Deployment, Result};

/// One announcement as the Mini App shows it to a Student.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct StudentAnnouncement {
    /// `announcement.id`.
    pub id: String,
    /// Title in the Student's Locale (Thai fallback).
    pub title: String,
    /// Body in the Student's Locale (Thai fallback).
    pub body: String,
    /// The announcement asks for an acknowledgement.
    pub require_ack: bool,
    /// When the Student acknowledged it (any round, any Channel).
    pub acked_at: Option<i64>,
    /// When it went out: `sent_at`, else `send_at`, else `created_at`.
    pub at: i64,
    /// Staff retracted it.
    pub retracted: bool,
}

impl StudentAnnouncement {
    /// Still waiting for this Student's acknowledgement (never once
    /// retracted).
    pub fn needs_ack(&self) -> bool {
        self.require_ack && self.acked_at.is_none() && !self.retracted
    }
}

/// `max(acked_at)` of the Student's rows; binds the Student once.
const ACKED: &str = "(SELECT max(acked_at) FROM announcement_delivery x \
     WHERE x.announcement_id = a.id AND x.student_id = ?) AS acked_at";
/// The Student has a delivery row; binds the Student once.
const DELIVERED: &str = "EXISTS (SELECT 1 FROM announcement_delivery d \
     WHERE d.announcement_id = a.id AND d.student_id = ?)";

/// Every announcement Student `student` has a delivery row for, not
/// retracted and not expired now: the ones waiting for an acknowledgement
/// first, then newest first; at most `limit`. Texts in `locale` (Thai
/// fallback).
pub async fn list_for_student(
    d: &Deployment,
    student: &str,
    locale: &Locale,
    limit: i64,
) -> Result<Vec<StudentAnnouncement>> {
    let s = d.services();
    let sql = format!(
        "SELECT a.id, a.status, a.require_ack, {ACKED}, \
         coalesce(a.sent_at, a.send_at, a.created_at) AS at \
         FROM announcement a WHERE a.status IN ('sending', 'sent') \
         AND (a.expires_at IS NULL OR a.expires_at > ?) AND {DELIVERED} \
         ORDER BY (a.require_ack = 1 AND acked_at IS NULL) DESC, at DESC, a.id DESC LIMIT ?"
    );
    let stmt = Statement::new(sql)
        .bind(student)
        .bind(s.now_ms())
        .bind(student)
        .bind(limit);
    let rows = s.db.query(stmt).await?;
    let mut out = Vec::with_capacity(rows.len());
    for i in 0..rows.len() {
        out.push(read(d, &rows, i, locale).await?);
    }
    Ok(out)
}

/// Announcement `id` as Student `student` sees it, `None` when the Student
/// has no delivery row for it (whether or not it exists). Expired ones are
/// returned (an acknowledgement is still accepted); a retracted one comes
/// back with [`StudentAnnouncement::retracted`] set and must not show its
/// text.
pub async fn detail_for_student(
    d: &Deployment,
    id: &str,
    student: &str,
    locale: &Locale,
) -> Result<Option<StudentAnnouncement>> {
    let s = d.services();
    let sql = format!(
        "SELECT a.id, a.status, a.require_ack, {ACKED}, \
         coalesce(a.sent_at, a.send_at, a.created_at) AS at \
         FROM announcement a WHERE a.id = ? AND {DELIVERED}"
    );
    let stmt = Statement::new(sql).bind(student).bind(id).bind(student);
    let rows = s.db.query(stmt).await?;
    if rows.is_empty() {
        return Ok(None);
    }
    read(d, &rows, 0, locale).await.map(Some)
}

async fn read(
    d: &Deployment,
    rows: &crate::port_types::Rows,
    i: usize,
    locale: &Locale,
) -> Result<StudentAnnouncement> {
    let id = text(rows, i, "id");
    let texts = store::texts(d.services(), &id).await?;
    let (title, body) = texts.pick(locale);
    Ok(StudentAnnouncement {
        title: title.to_string(),
        body: body.to_string(),
        require_ack: int(rows, i, "require_ack") == Some(1),
        acked_at: int(rows, i, "acked_at"),
        at: int(rows, i, "at").unwrap_or(0),
        retracted: text(rows, i, "status") == "retracted",
        id,
    })
}

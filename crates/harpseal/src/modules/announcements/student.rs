//! Reads for the Mini App (#28): a Student's announcements and one of them,
//! always through the Student's delivery rows, so a page never shows (or
//! reveals the existence of) an announcement the Student was not sent.

use super::store::{int, text};
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
    /// Its `expires_at` has passed (#77): a Student gets the "no longer
    /// available" view, without its text.
    pub expired: bool,
    /// Its files: images (#109) and PDFs (#113), shown by
    /// [`StudentAnnouncement::body_html`].
    pub(crate) images: Vec<super::files::Stored>,
}

impl StudentAnnouncement {
    /// Still waiting for this Student's acknowledgement (never once
    /// retracted or expired).
    pub fn needs_ack(&self) -> bool {
        self.require_ack && self.acked_at.is_none() && !self.unavailable()
    }

    /// The body as the Mini App shows it: formatted, each image in place
    /// on its public path, then the images not placed (alt `"{label} N"`),
    /// then the PDFs with their names and sizes.
    pub fn body_html(&self, label: &str) -> maud::Markup {
        super::files::body_html(&self.body, &self.images, false, label)
    }

    /// The page needs the "open outside" script: the body has a link, or
    /// a PDF is attached (#113).
    pub fn opens_outside(&self) -> bool {
        !self.unavailable()
            && (super::files::pdfs(&self.images).next().is_some()
                || !crate::rich_text::parse(&self.body).links().is_empty())
    }

    /// Retracted or expired: a Student no longer sees its text nor
    /// acknowledges it.
    pub fn unavailable(&self) -> bool {
        self.retracted || self.expired
    }
}

/// `max(acked_at)` of the Student's rows; binds the Student once.
const ACKED: &str = "(SELECT max(acked_at) FROM announcement_delivery x \
     WHERE x.announcement_id = a.id AND x.student_id = ?) AS acked_at";
/// Whether it expired; binds now once.
const EXPIRED: &str = "(a.expires_at IS NOT NULL AND a.expires_at <= ?) AS expired";
/// Title and body in the bound Locale, else Thai; binds the Locale once.
const TEXT: &str = "coalesce(tl.title, tt.title, '') AS title, \
     coalesce(tl.body, tt.body, '') AS body FROM announcement a \
     LEFT JOIN announcement_text tl ON tl.announcement_id = a.id AND tl.locale = ? \
     LEFT JOIN announcement_text tt ON tt.announcement_id = a.id AND tt.locale = 'th'";
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
        "SELECT a.id, a.status, a.require_ack, {ACKED}, {EXPIRED}, \
         coalesce(a.sent_at, a.send_at, a.created_at) AS at, {TEXT} \
         WHERE a.status IN ('sending', 'sent') \
         AND (a.expires_at IS NULL OR a.expires_at > ?) AND {DELIVERED} \
         ORDER BY (a.require_ack = 1 AND acked_at IS NULL) DESC, at DESC, a.id DESC LIMIT ?"
    );
    let now = s.now_ms();
    let stmt = Statement::new(sql)
        .bind(student)
        .bind(now)
        .bind(locale.as_str())
        .bind(now)
        .bind(student)
        .bind(limit);
    let rows = s.db.query(stmt).await?;
    let mut out: Vec<StudentAnnouncement> = (0..rows.len()).map(|i| read(&rows, i)).collect();
    // Their files in one query, not one per announcement.
    let ids: Vec<&str> = out.iter().map(|a| a.id.as_str()).collect();
    let mut images = super::files::list_many(s, &ids).await?;
    for a in &mut out {
        a.images = images.remove(&a.id).unwrap_or_default();
    }
    Ok(out)
}

/// Announcement `id` as Student `student` sees it, `None` when the Student
/// has no delivery row for it (whether or not it exists). A retracted or
/// expired one comes back with [`StudentAnnouncement::retracted`] or
/// [`StudentAnnouncement::expired`] set ([`StudentAnnouncement::unavailable`])
/// and must not show its text (#77: a deep link to an expired one shows
/// "no longer available").
pub async fn detail_for_student(
    d: &Deployment,
    id: &str,
    student: &str,
    locale: &Locale,
) -> Result<Option<StudentAnnouncement>> {
    let s = d.services();
    let sql = format!(
        "SELECT a.id, a.status, a.require_ack, {ACKED}, {EXPIRED}, \
         coalesce(a.sent_at, a.send_at, a.created_at) AS at, {TEXT} \
         WHERE a.id = ? AND {DELIVERED}"
    );
    let stmt = Statement::new(sql)
        .bind(student)
        .bind(s.now_ms())
        .bind(locale.as_str())
        .bind(id)
        .bind(student);
    let rows = s.db.query(stmt).await?;
    if rows.is_empty() {
        return Ok(None);
    }
    Ok(Some(with_images(s, read(&rows, 0)).await?))
}

/// `a` with its images, unless it is no longer shown.
async fn with_images(
    s: &crate::Services,
    mut a: StudentAnnouncement,
) -> Result<StudentAnnouncement> {
    if !a.unavailable() {
        a.images = super::files::list(s, &a.id).await?;
    }
    Ok(a)
}

fn read(rows: &crate::port_types::Rows, i: usize) -> StudentAnnouncement {
    StudentAnnouncement {
        id: text(rows, i, "id"),
        title: text(rows, i, "title"),
        body: text(rows, i, "body"),
        require_ack: int(rows, i, "require_ack") == Some(1),
        acked_at: int(rows, i, "acked_at"),
        at: int(rows, i, "at").unwrap_or(0),
        retracted: text(rows, i, "status") == "retracted",
        expired: int(rows, i, "expired") == Some(1),
        images: Vec::new(),
    }
}

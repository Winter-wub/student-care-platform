//! Reads behind the Students pages.

use serde_json::{Map, Value as Json};

use super::StudentFields;
use crate::message::Locale;
use crate::port_types::{Rows, Statement, Value};
use crate::{Result, Services};

/// Students per list page.
pub(crate) const PAGE_SIZE: i64 = 50;

/// Column `c` of row `i` as text.
pub(super) fn text(rows: &Rows, i: usize, c: &str) -> Option<String> {
    rows.get(i, c).and_then(Value::as_text).map(str::to_string)
}

/// Column `c` of row `i` as an integer.
pub(super) fn int(rows: &Rows, i: usize, c: &str) -> Option<i64> {
    rows.get(i, c).and_then(Value::as_i64)
}

/// A JSON object column; empty when it is not one.
fn object(raw: &str) -> Map<String, Json> {
    match serde_json::from_str(raw) {
        Ok(Json::Object(m)) => m,
        _ => Map::new(),
    }
}

/// A JSON array of strings column.
pub(super) fn string_list(raw: &str) -> Vec<String> {
    serde_json::from_str::<Vec<String>>(raw).unwrap_or_default()
}

/// One Student, as stored.
pub(crate) struct StudentRow {
    pub(crate) id: String,
    pub(crate) code: String,
    pub(crate) status: String,
    pub(crate) fields: StudentFields,
    pub(crate) overrides: Vec<String>,
    /// `attributes` and `manual_overrides` exactly as read, for the
    /// compare-and-set of a write.
    pub(crate) raw_attributes: String,
    pub(crate) raw_overrides: String,
}

/// The Student `id`, if any.
pub(crate) async fn student(s: &Services, id: &str) -> Result<Option<StudentRow>> {
    let stmt = Statement::new(
        "SELECT id, student_code, display_name, email, locale, status, attributes, \
         manual_overrides FROM student WHERE id = ?",
    )
    .bind(id);
    let rows = s.db.query(stmt).await?;
    let Some(id) = text(&rows, 0, "id") else {
        return Ok(None);
    };
    let t = |c: &str| text(&rows, 0, c).unwrap_or_default();
    let (raw_attributes, raw_overrides) = (t("attributes"), t("manual_overrides"));
    Ok(Some(StudentRow {
        id,
        code: t("student_code"),
        status: t("status"),
        fields: StudentFields {
            display_name: t("display_name"),
            email: text(&rows, 0, "email"),
            locale: Locale(t("locale")),
            attributes: object(&raw_attributes),
        },
        overrides: string_list(&raw_overrides),
        raw_attributes,
        raw_overrides,
    }))
}

/// One line of the list.
pub(crate) struct ListRow {
    pub(crate) id: String,
    pub(crate) code: String,
    pub(crate) name: String,
    pub(crate) status: String,
    pub(crate) attributes: Map<String, Json>,
    /// Live Links: (channel, status).
    pub(crate) links: Vec<(String, String)>,
}

/// `q` as a `LIKE` pattern matching it anywhere (`\` escapes).
fn like(q: &str) -> String {
    let mut p = String::from("%");
    for c in q.chars() {
        if matches!(c, '%' | '_' | '\\') {
            p.push('\\');
        }
        p.push(c);
    }
    p.push('%');
    p
}

/// Students whose name or code contains `q` (all when empty), page
/// `page` (from 1) by student code, and how many match in all.
pub(crate) async fn search(s: &Services, q: &str, page: i64) -> Result<(Vec<ListRow>, i64)> {
    let pattern = like(q.trim());
    let filter = "WHERE display_name LIKE ? ESCAPE '\\' OR student_code LIKE ? ESCAPE '\\'";
    let count = Statement::new(format!("SELECT count(*) AS n FROM student {filter}"))
        .bind(pattern.clone())
        .bind(pattern.clone());
    let total = int(&s.db.query(count).await?, 0, "n").unwrap_or(0);
    let stmt = Statement::new(format!(
        "SELECT id, student_code, display_name, status, attributes FROM student {filter} \
         ORDER BY student_code, id LIMIT ? OFFSET ?"
    ))
    .bind(pattern.clone())
    .bind(pattern)
    .bind(PAGE_SIZE)
    .bind((page.max(1) - 1) * PAGE_SIZE);
    let rows = s.db.query(stmt).await?;
    let mut out: Vec<ListRow> = (0..rows.len())
        .map(|i| {
            let t = |c: &str| text(&rows, i, c).unwrap_or_default();
            ListRow {
                id: t("id"),
                code: t("student_code"),
                name: t("display_name"),
                status: t("status"),
                attributes: object(&t("attributes")),
                links: Vec::new(),
            }
        })
        .collect();
    if out.is_empty() {
        return Ok((out, total));
    }
    let marks = vec!["?"; out.len()].join(", ");
    let mut stmt = Statement::new(format!(
        "SELECT student_id, channel, status FROM link \
         WHERE status <> 'revoked' AND student_id IN ({marks})"
    ));
    for r in &out {
        stmt = stmt.bind(r.id.clone());
    }
    let links = s.db.query(stmt).await?;
    for i in 0..links.len() {
        let sid = text(&links, i, "student_id").unwrap_or_default();
        if let Some(r) = out.iter_mut().find(|r| r.id == sid) {
            let ch = text(&links, i, "channel").unwrap_or_default();
            r.links
                .push((ch, text(&links, i, "status").unwrap_or_default()));
        }
    }
    Ok((out, total))
}

/// One Link, live or past.
pub(crate) struct LinkRow {
    pub(crate) id: String,
    pub(crate) channel: String,
    pub(crate) external_id: String,
    pub(crate) status: String,
    pub(crate) method: String,
    pub(crate) linked_at: i64,
    pub(crate) revoked_at: Option<i64>,
    pub(crate) verified_by: Option<String>,
}

/// Every Link of the Student, newest first.
pub(crate) async fn links(s: &Services, student_id: &str) -> Result<Vec<LinkRow>> {
    let stmt = Statement::new(
        "SELECT l.id, l.channel, ca.external_id, l.status, l.method, l.linked_at, \
         l.revoked_at, st.display_name AS verified_by FROM link l \
         JOIN channel_account ca ON ca.id = l.channel_account_id \
         LEFT JOIN staff st ON st.id = l.verified_by_staff_id \
         WHERE l.student_id = ? ORDER BY l.linked_at DESC, l.id DESC",
    )
    .bind(student_id);
    let rows = s.db.query(stmt).await?;
    Ok((0..rows.len())
        .map(|i| LinkRow {
            id: text(&rows, i, "id").unwrap_or_default(),
            channel: text(&rows, i, "channel").unwrap_or_default(),
            external_id: text(&rows, i, "external_id").unwrap_or_default(),
            status: text(&rows, i, "status").unwrap_or_default(),
            method: text(&rows, i, "method").unwrap_or_default(),
            linked_at: int(&rows, i, "linked_at").unwrap_or(0),
            revoked_at: int(&rows, i, "revoked_at"),
            verified_by: text(&rows, i, "verified_by"),
        })
        .collect())
}

/// A Tag on the Student: (tag id, name, source `roster` / `manual`).
pub(crate) async fn tags(s: &Services, student_id: &str) -> Result<Vec<(String, String, String)>> {
    let stmt = Statement::new(
        "SELECT t.id, t.name, st.source FROM student_tag st JOIN tag t ON t.id = st.tag_id \
         WHERE st.student_id = ? ORDER BY t.name, st.source",
    )
    .bind(student_id);
    let rows = s.db.query(stmt).await?;
    Ok((0..rows.len())
        .map(|i| {
            let t = |c: &str| text(&rows, i, c).unwrap_or_default();
            (t("id"), t("name"), t("source"))
        })
        .collect())
}

/// Names of the Student's manual Tags.
pub(crate) async fn manual_tag_names(s: &Services, student_id: &str) -> Result<Vec<String>> {
    Ok(tags(s, student_id)
        .await?
        .into_iter()
        .filter(|(_, _, src)| src == "manual")
        .map(|(_, name, _)| name)
        .collect())
}

/// Consent rows: (notice version, accepted at, Channel), newest first.
pub(crate) async fn consents(s: &Services, student_id: &str) -> Result<Vec<(String, i64, String)>> {
    let stmt = Statement::new(
        "SELECT c.notice_version, c.accepted_at, ca.channel FROM consent c \
         JOIN channel_account ca ON ca.id = c.channel_account_id \
         WHERE c.student_id = ? ORDER BY c.accepted_at DESC, c.id DESC",
    )
    .bind(student_id);
    let rows = s.db.query(stmt).await?;
    Ok((0..rows.len())
        .map(|i| {
            let t = |c: &str| text(&rows, i, c).unwrap_or_default();
            (
                t("notice_version"),
                int(&rows, i, "accepted_at").unwrap_or(0),
                t("channel"),
            )
        })
        .collect())
}

/// The 5 newest Security Events about the Student: (at, kind).
pub(crate) async fn security_events(s: &Services, student_id: &str) -> Result<Vec<(i64, String)>> {
    let stmt = Statement::new(
        "SELECT at, kind FROM security_event WHERE student_id = ? \
         ORDER BY at DESC, id DESC LIMIT 5",
    )
    .bind(student_id);
    let rows = s.db.query(stmt).await?;
    Ok((0..rows.len())
        .map(|i| {
            let kind = text(&rows, i, "kind").unwrap_or_default();
            (int(&rows, i, "at").unwrap_or(0), kind)
        })
        .collect())
}

#[cfg(test)]
mod tests {
    #[test]
    fn like_patterns_escape_wildcards() {
        assert_eq!(super::like("a%b_c\\"), "%a\\%b\\_c\\\\%");
        assert_eq!(super::like(""), "%%");
    }
}

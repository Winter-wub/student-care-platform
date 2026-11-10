//! The writes of the bulk actions (#145): a manual Tag added or removed,
//! and Staff unlink, on many Students. Each runs in batches of [`CHUNK`]
//! Students, one `Db::batch` each; inside a batch every Student gets their
//! own audit row, which gates that Student's change (the compare-and-set
//! of the single actions), so a Student who changed meanwhile is skipped,
//! never written twice.

use std::collections::HashMap;

use serde_json::json;

use super::bulk::json_ids;
use super::read;
use super::service::{gated_stmt, target};
use super::tags::staff_of;
use crate::audit::{self, Actor, RequestCtx};
use crate::deployment::Deployment;
use crate::port_types::Statement;
use crate::{Result, Services, Value, links};

/// Students per batch.
const CHUNK: usize = 50;

/// The manual Tag names of each Student of `ids`, sorted.
async fn manual_names(s: &Services, ids: &[String]) -> Result<HashMap<String, Vec<String>>> {
    let stmt = Statement::new(
        "SELECT st.student_id, t.name FROM student_tag st JOIN tag t ON t.id = st.tag_id \
         WHERE st.source = 'manual' AND st.student_id IN (SELECT value FROM json_each(?)) \
         ORDER BY t.name",
    )
    .bind(json_ids(ids));
    let rows = s.db.query(stmt).await?;
    let mut out: HashMap<String, Vec<String>> = HashMap::new();
    for i in 0..rows.len() {
        let sid = read::text(&rows, i, "student_id").unwrap_or_default();
        let name = read::text(&rows, i, "name").unwrap_or_default();
        out.entry(sid).or_default().push(name);
    }
    Ok(out)
}

/// The Students of `ids` holding the manual Tag `name` now.
pub(super) async fn holding(s: &Services, ids: &[String], name: &str) -> Result<Vec<String>> {
    let stmt = Statement::new(
        "SELECT DISTINCT st.student_id AS id FROM student_tag st \
         JOIN tag t ON t.id = st.tag_id WHERE st.source = 'manual' AND t.name = ? \
         AND st.student_id IN (SELECT value FROM json_each(?))",
    )
    .bind(name)
    .bind(json_ids(ids));
    ids_of(s, stmt).await
}

/// The Students of `ids` with a live (active or unreachable) Link.
pub(super) async fn linked(s: &Services, ids: &[String]) -> Result<Vec<String>> {
    let stmt = Statement::new(
        "SELECT DISTINCT student_id AS id FROM link WHERE status <> 'revoked' \
         AND student_id IN (SELECT value FROM json_each(?))",
    )
    .bind(json_ids(ids));
    ids_of(s, stmt).await
}

async fn ids_of(s: &Services, stmt: Statement) -> Result<Vec<String>> {
    let rows = s.db.query(stmt).await?;
    Ok((0..rows.len())
        .filter_map(|i| read::text(&rows, i, "id"))
        .collect())
}

/// Adds the manual Tag `name` (already checked with
/// [`super::tags::valid_name`]) to every Student of `ids` that lacks it:
/// the Tag is created when new, and each Student gets the audit row
/// `student.tag_add` of [`super::add_manual_tag`]. Returns how many
/// Students were tagged.
pub(super) async fn add_tag(
    d: &Deployment,
    ids: &[String],
    name: &str,
    actor: &Actor,
    ctx: &RequestCtx,
) -> Result<usize> {
    let s = d.services();
    let mut done = 0;
    for group in ids.chunks(CHUNK) {
        let names = manual_names(s, group).await?;
        let now = s.now_ms();
        let mut stmts = vec![
            Statement::new(
                "INSERT INTO tag (id, name, created_at) VALUES (?, ?, ?) \
                 ON CONFLICT (name) DO NOTHING",
            )
            .bind(s.new_id()?)
            .bind(name)
            .bind(now),
        ];
        let mut attaches = Vec::new();
        for id in group {
            let before = names.get(id).cloned().unwrap_or_default();
            if before.iter().any(|n| n == name) {
                continue;
            }
            let mut after = before.clone();
            after.push(name.to_string());
            after.sort();
            let absent = Statement::new(
                "NOT EXISTS (SELECT 1 FROM student_tag st JOIN tag t ON t.id = st.tag_id \
                 WHERE st.student_id = ? AND t.name = ? AND st.source = 'manual')",
            )
            .bind(id.as_str())
            .bind(name);
            let (row, gate) = audit::entry_if(
                s,
                actor,
                "student.tag_add",
                Some(&target(id)),
                Some(json!({ "manual_tags": before })),
                Some(json!({ "manual_tags": after })),
                ctx,
                absent,
            )?;
            stmts.push(row);
            attaches.push(stmts.len());
            stmts.push(gated_stmt(
                "INSERT INTO student_tag (student_id, tag_id, source, added_at, added_by) \
                 SELECT ?, id, 'manual', ?, ? FROM tag WHERE name = ?",
                &gate,
                vec![
                    Value::from(id.as_str()),
                    Value::from(now),
                    Value::from(staff_of(actor)),
                    Value::from(name),
                ],
            ));
        }
        if attaches.is_empty() {
            continue;
        }
        let results = s.db.batch(stmts).await?;
        done += count(&results, &attaches);
    }
    Ok(done)
}

/// Removes the manual Tag `name` from every Student of `ids` holding it;
/// roster Tags of the same name stay. Each Student gets the audit row
/// `student.tag_remove` of [`super::remove_manual_tag`]. Returns how many
/// Students lost it.
pub(super) async fn remove_tag(
    d: &Deployment,
    ids: &[String],
    name: &str,
    actor: &Actor,
    ctx: &RequestCtx,
) -> Result<usize> {
    let s = d.services();
    let mut done = 0;
    for group in ids.chunks(CHUNK) {
        let names = manual_names(s, group).await?;
        let mut stmts = Vec::new();
        let mut detaches = Vec::new();
        for id in group {
            let before = names.get(id).cloned().unwrap_or_default();
            if !before.iter().any(|n| n == name) {
                continue;
            }
            let after: Vec<&String> = before.iter().filter(|n| *n != name).collect();
            let present = Statement::new(
                "EXISTS (SELECT 1 FROM student_tag st JOIN tag t ON t.id = st.tag_id \
                 WHERE st.student_id = ? AND t.name = ? AND st.source = 'manual')",
            )
            .bind(id.as_str())
            .bind(name);
            let (row, gate) = audit::entry_if(
                s,
                actor,
                "student.tag_remove",
                Some(&target(id)),
                Some(json!({ "manual_tags": before })),
                Some(json!({ "manual_tags": after })),
                ctx,
                present,
            )?;
            stmts.push(row);
            detaches.push(stmts.len());
            stmts.push(gated_stmt(
                "DELETE FROM student_tag WHERE student_id = ? AND source = 'manual' \
                 AND tag_id = (SELECT id FROM tag WHERE name = ?)",
                &gate,
                vec![Value::from(id.as_str()), Value::from(name)],
            ));
        }
        if detaches.is_empty() {
            continue;
        }
        let results = s.db.batch(stmts).await?;
        done += count(&results, &detaches);
    }
    Ok(done)
}

/// What a bulk unlink did.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(super) struct Unlinked {
    /// Students who lost their Links.
    pub(super) students: usize,
    /// Links revoked.
    pub(super) links: usize,
    /// Revoked Links whose Student could not be told in the chat.
    pub(super) unnotified: usize,
    /// A later batch failed after an earlier one committed: only some of
    /// the Students were unlinked.
    pub(super) partial: bool,
}

/// Revokes every live Link of every Student of `ids` (Staff unlink). Per
/// Student, an audit row `student.unlink` (the live Link ids before, none
/// after) gates the revoke of each Link, which writes its own `link.revoke`
/// row as a single revoke does; after the commit each Student is told on
/// each Channel, as [`crate::links::revoke_by_staff`] does. A batch that
/// fails after an earlier one committed is not an error: the result says
/// `partial`, so Staff learn that some Students were unlinked.
pub(super) async fn unlink(
    d: &Deployment,
    ids: &[String],
    actor: &Actor,
    ctx: &RequestCtx,
) -> Result<Unlinked> {
    let mut out = Unlinked::default();
    for group in ids.chunks(CHUNK) {
        match unlink_batch(d, group, actor, ctx, &mut out).await {
            Ok(()) => {}
            Err(_) if out.students > 0 => {
                out.partial = true;
                return Ok(out);
            }
            Err(e) => return Err(e),
        }
    }
    Ok(out)
}

/// One batch of [`unlink`], counted into `out`.
async fn unlink_batch(
    d: &Deployment,
    group: &[String],
    actor: &Actor,
    ctx: &RequestCtx,
    out: &mut Unlinked,
) -> Result<()> {
    let s = d.services();
    let default = &d.config().default_locale;
    {
        let mut live = links::live_links_of_students(s, group, default).await?;
        let mut stmts = Vec::new();
        let mut gates = Vec::new();
        let mut revokes = Vec::new();
        for id in group {
            let mine: Vec<_> = live.extract_if(.., |(sid, _)| sid == id).collect();
            if mine.is_empty() {
                continue;
            }
            let before: Vec<&str> = mine.iter().map(|(_, l)| l.id.as_str()).collect();
            let any_live = Statement::new(
                "EXISTS (SELECT 1 FROM link WHERE student_id = ? AND status <> 'revoked')",
            )
            .bind(id.as_str());
            let (row, gate) = audit::entry_if(
                s,
                actor,
                "student.unlink",
                Some(&target(id)),
                Some(json!({ "links": before })),
                Some(json!({ "links": [] })),
                ctx,
                any_live,
            )?;
            gates.push(stmts.len());
            stmts.push(row);
            for (_, link) in mine {
                let more = links::staff_revoke_statements(s, &link, actor, ctx, Some(&gate))?;
                revokes.push((stmts.len() + 1, link));
                stmts.extend(more);
            }
        }
        if gates.is_empty() {
            return Ok(());
        }
        let results = s.db.batch(stmts).await?;
        out.students += count(&results, &gates);
        for (i, link) in revokes {
            if results.get(i).map_or(0, |r| r.rows_affected) == 0 {
                continue;
            }
            out.links += 1;
            if !matches!(links::notify_revoked(d, link).await, Ok(true)) {
                out.unnotified += 1;
            }
        }
    }
    Ok(())
}

/// How many of the statements at `at` changed a row.
fn count(results: &[crate::port_types::ExecResult], at: &[usize]) -> usize {
    at.iter()
        .filter(|i| results.get(**i).map_or(0, |r| r.rows_affected) > 0)
        .count()
}

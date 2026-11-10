//! Manual Tags on a Student (roster Tags are the import's, #24).

use serde_json::json;

use super::read;
use super::service::{affected, gated_stmt, load, target};
use crate::audit::{self, Actor, RequestCtx};
use crate::deployment::Deployment;
use crate::port_types::Statement;
use crate::{Error, Result};

/// Longest manual Tag name, in characters.
const MAX_TAG_CHARS: usize = 64;

/// Attaches the manual Tag `name` (trimmed, 1 to 64 characters; created
/// when new) to the Student. Roster Tags of the same name are separate
/// rows and stay as they are. Audit `student.tag_add` with the manual
/// Tag names before and after.
///
/// `Err(Error::Invalid)` for an unknown Student or a bad name,
/// `Err(Error::Conflict)` when the Student already has it as a manual Tag.
pub async fn add_manual_tag(
    d: &Deployment,
    student_id: &str,
    name: &str,
    actor: &Actor,
    ctx: &RequestCtx,
) -> Result<()> {
    let s = d.services();
    let name = valid_name(name).ok_or_else(|| Error::Invalid("tag name".into()))?;
    load(s, student_id).await?;
    let before = read::manual_tag_names(s, student_id).await?;
    let mut after = before.clone();
    after.push(name.to_string());
    after.sort();
    let now = s.now_ms();
    let create = Statement::new(
        "INSERT INTO tag (id, name, created_at) VALUES (?, ?, ?) ON CONFLICT (name) DO NOTHING",
    )
    .bind(s.new_id()?)
    .bind(name)
    .bind(now);
    let absent = Statement::new(
        "NOT EXISTS (SELECT 1 FROM student_tag st JOIN tag t ON t.id = st.tag_id \
         WHERE st.student_id = ? AND t.name = ? AND st.source = 'manual')",
    )
    .bind(student_id)
    .bind(name);
    let (audit, gate) = audit::entry_if(
        s,
        actor,
        "student.tag_add",
        Some(&target(student_id)),
        Some(json!({ "manual_tags": before })),
        Some(json!({ "manual_tags": after })),
        ctx,
        absent,
    )?;
    let attach = gated_stmt(
        "INSERT INTO student_tag (student_id, tag_id, source, added_at, added_by) \
         SELECT ?, id, 'manual', ?, ? FROM tag WHERE name = ?",
        &gate,
        vec![
            crate::Value::from(student_id),
            crate::Value::from(now),
            crate::Value::from(staff_of(actor)),
            crate::Value::from(name),
        ],
    );
    let done = s.db.batch(vec![create, audit, attach]).await?;
    if affected(&done, 2) != 1 {
        return Err(Error::Conflict("the Student already has this Tag".into()));
    }
    Ok(())
}

/// Detaches the manual Tag `tag_id` from the Student; a roster Tag with
/// the same id stays. Audit `student.tag_remove` with the manual Tag names
/// before and after.
///
/// `Err(Error::Invalid)` for an unknown Student, `Err(Error::Conflict)`
/// when the Student has no such manual Tag (any more).
pub async fn remove_manual_tag(
    d: &Deployment,
    student_id: &str,
    tag_id: &str,
    actor: &Actor,
    ctx: &RequestCtx,
) -> Result<()> {
    let s = d.services();
    load(s, student_id).await?;
    let tags = read::tags(s, student_id).await?;
    let Some((_, name, _)) = tags
        .iter()
        .find(|(id, _, src)| id == tag_id && src == "manual")
    else {
        return Err(Error::Conflict("no such manual Tag".into()));
    };
    let before = read::manual_tag_names(s, student_id).await?;
    let after: Vec<&String> = before.iter().filter(|n| *n != name).collect();
    let present = Statement::new(
        "EXISTS (SELECT 1 FROM student_tag WHERE student_id = ? AND tag_id = ? \
         AND source = 'manual')",
    )
    .bind(student_id)
    .bind(tag_id);
    let (audit, gate) = audit::entry_if(
        s,
        actor,
        "student.tag_remove",
        Some(&target(student_id)),
        Some(json!({ "manual_tags": before })),
        Some(json!({ "manual_tags": after })),
        ctx,
        present,
    )?;
    let detach = gated_stmt(
        "DELETE FROM student_tag WHERE student_id = ? AND tag_id = ? AND source = 'manual'",
        &gate,
        vec![crate::Value::from(student_id), crate::Value::from(tag_id)],
    );
    let done = s.db.batch(vec![audit, detach]).await?;
    if affected(&done, 1) != 1 {
        return Err(Error::Conflict("no such manual Tag".into()));
    }
    Ok(())
}

/// `name` trimmed, when it can name a manual Tag: 1 to 64 characters,
/// no control characters.
pub(super) fn valid_name(name: &str) -> Option<&str> {
    let name = name.trim();
    let n = name.chars().count();
    (n > 0 && n <= MAX_TAG_CHARS && !name.chars().any(char::is_control)).then_some(name)
}

pub(super) fn staff_of(actor: &Actor) -> Option<String> {
    match actor {
        Actor::Staff(id) => Some(id.clone()),
        _ => None,
    }
}

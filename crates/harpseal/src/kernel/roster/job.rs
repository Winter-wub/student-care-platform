//! The Job `kernel.roster.import` (payload `{import_id, chunk}`): applies
//! rows `[chunk, chunk + 500)` of the stored file in one `Db::batch`,
//! enqueues the next chunk as its own Job, and after the last one (in
//! `full` mode) marks every active Student the file did not list as `left`
//! and revokes their Links (see `finish.rs`).
//!
//! Idempotent: each run re-reads and re-parses the stored file, and the
//! batch advances `applied_rows` with a compare-and-set that every other
//! statement of the batch is gated on (through a per-run token in
//! `counts.run`), so a repeated run of a finished chunk writes nothing.
//! Every Student the import listed gets `roster_imported_at` = the
//! import's `started_at`, which the claim makes later than any earlier
//! import's, so "not listed" is `roster_imported_at < started_at`.

use std::collections::BTreeSet;

use serde::Deserialize;
use serde_json::json;

use super::service::{JOB_KIND, job_key};
use super::store::{self, Existing, Import, Tally};
use super::validate::Record;
use crate::manifest::Job;
use crate::module::JobContext;
use crate::port_types::{Statement, Value};
use crate::{Error, Result, Services};

/// Rows per chunk (one batch each).
pub(crate) const CHUNK: i64 = 500;
/// Tag names per `INSERT INTO tag` (3 parameters each, D1 allows 100).
const TAGS_PER_INSERT: usize = 30;

#[derive(Debug, Deserialize)]
struct Payload {
    import_id: String,
    #[serde(default)]
    chunk: i64,
}

/// The handler registered for [`JOB_KIND`]. After its last attempt fails,
/// the import is marked `failed` (the reason never quotes the file).
pub(crate) async fn run(cx: JobContext, job: Job) -> Result<()> {
    let p: Payload = serde_json::from_str(&job.payload)
        .map_err(|_| Error::Invalid("roster import job payload".into()))?;
    let outcome = step(&cx, &job, &p).await;
    if let Err(e) = &outcome
        && last_attempt(&cx.services, &job).await
    {
        let failure = store::Failure {
            job: Some(describe(e)),
            ..store::Failure::default()
        };
        let stmt = Statement::new(
            "UPDATE roster_import SET state = 'failed', error = ?, finished_at = ? \
             WHERE id = ? AND state IN ('queued', 'running')",
        )
        .bind(serde_json::to_string(&failure).unwrap_or_default())
        .bind(cx.services.now_ms())
        .bind(p.import_id.as_str());
        let _ = cx.services.db.execute(stmt).await;
    }
    outcome
}

async fn step(cx: &JobContext, job: &Job, p: &Payload) -> Result<()> {
    let s = &cx.services;
    let Some(imp) = store::load(s, &p.import_id).await? else {
        return Ok(());
    };
    if !matches!(imp.state.as_str(), "queued" | "running") {
        return Ok(());
    }
    if imp.applied_rows > p.chunk && imp.applied_rows < imp.total_rows {
        // This chunk is applied; make sure the next one is queued.
        let next = imp.applied_rows;
        let payload = json!({ "import_id": imp.id, "chunk": next });
        cx.jobs
            .enqueue_unique(&job_key(&imp.id, next), JOB_KIND, payload, None)
            .await?;
        return Ok(());
    }
    if imp.applied_rows < p.chunk.min(imp.total_rows) {
        return Err(Error::Invalid(
            "roster chunk ahead of the applied rows".into(),
        ));
    }
    let claim = Statement::new(
        "UPDATE roster_import SET state = 'running', started_at = COALESCE(started_at, \
         MAX(?, (SELECT COALESCE(MAX(roster_imported_at), 0) + 1 FROM student))) \
         WHERE id = ? AND state IN ('queued', 'running') AND applied_rows = ?",
    )
    .bind(s.now_ms())
    .bind(imp.id.as_str())
    .bind(imp.applied_rows);
    if s.db.execute(claim).await?.rows_affected == 0 {
        return Ok(());
    }
    let Some(imp) = store::load(s, &p.import_id).await? else {
        return Ok(());
    };
    if imp.applied_rows < imp.total_rows {
        if !apply_chunk(cx, &imp).await? {
            return Ok(());
        }
        let Some(after) = store::load(s, &imp.id).await? else {
            return Ok(());
        };
        if after.applied_rows < after.total_rows {
            let note = format!("{} / {}", after.applied_rows, after.total_rows);
            return cx.progress(&note).await;
        }
        return super::finish::finish(cx, job, &after).await;
    }
    super::finish::finish(cx, job, &imp).await
}

/// Applies the chunk starting at `applied_rows`; `false` when another run
/// applied it first.
async fn apply_chunk(cx: &JobContext, imp: &Import) -> Result<bool> {
    let s = &cx.services;
    let bytes = s
        .files
        .get(&imp.file_key)
        .await?
        .ok_or_else(|| Error::Invalid("the stored roster file is missing".into()))?;
    if crate::auth::sha256_hex(&bytes) != imp.counts.sha256 {
        return Err(Error::Invalid("the stored roster file changed".into()));
    }
    let rows = super::file::parse(&bytes, &imp.file_name, &cx.config.roster)
        .map_err(|_| Error::Invalid("the stored roster file no longer reads".into()))?;
    let valid = super::validate::validate(&rows, &cx.config)
        .map_err(|_| Error::Invalid("the stored roster file no longer validates".into()))?;
    let from = usize::try_from(imp.applied_rows).unwrap_or(usize::MAX);
    let to = (from + CHUNK as usize).min(valid.records.len());
    let records = valid.records.get(from..to).unwrap_or_default();
    let codes: Vec<&str> = records.iter().map(|r| r.code.as_str()).collect();
    let existing = store::students_by_code(s, &codes).await?;

    let token = s.random_hex(8)?;
    let new_applied = to as i64;
    let started = imp.started_at.unwrap_or_else(|| s.now_ms());
    let gate = Gate {
        id: imp.id.clone(),
        applied: new_applied,
        token: token.clone(),
    };
    let mut counts = imp.counts.clone();
    counts.run = token;
    let mut stmts = vec![
        Statement::new(
            "UPDATE roster_import SET applied_rows = ?, counts = ? \
             WHERE id = ? AND state = 'running' AND applied_rows = ?",
        )
        .bind(new_applied)
        .bind(String::new()) // replaced below, once the tally is known
        .bind(imp.id.as_str())
        .bind(imp.applied_rows),
    ];
    let mut tally = Tally::default();
    let mut tag_names = BTreeSet::new();
    for rec in records {
        let e = existing.get(&rec.code);
        let (t, merge) = store::classify(e, rec);
        tally.add(&t);
        match e {
            // Nothing to write but the mark that the import listed it.
            Some(e) if t.unchanged == 1 => stmts.push(
                gate.bind(
                    Statement::new(format!(
                        "UPDATE student SET roster_imported_at = ? WHERE id = ? AND {}",
                        Gate::SQL
                    ))
                    .bind(started)
                    .bind(e.id.as_str()),
                ),
            ),
            _ => stmts.push(upsert(s, rec, &merge.fields, e, started, &gate)?),
        }
        if t.new == 1 || e.is_some_and(|e| tags_differ(e, rec)) {
            stmts.extend(replace_tags(rec, s.now_ms(), &gate));
            tag_names.extend(rec.tags.iter().cloned());
        }
    }
    counts.result.add(&tally);
    let counts_json =
        serde_json::to_string(&counts).map_err(|_| Error::Invalid("roster counts".into()))?;
    stmts[0].params[1] = Value::Text(counts_json);
    let tags: Vec<String> = tag_names.into_iter().collect();
    for group in tags.chunks(TAGS_PER_INSERT) {
        stmts.insert(1, create_tags(s, group)?);
    }
    if new_applied < imp.total_rows {
        let payload = json!({ "import_id": imp.id, "chunk": new_applied });
        let (_, next) =
            cx.jobs
                .insert_unique(&job_key(&imp.id, new_applied), JOB_KIND, &payload, None)?;
        stmts.push(next);
    }
    let done = s.db.batch(stmts).await?;
    Ok(done.first().map(|r| r.rows_affected) == Some(1))
}

/// The batch gate: true only in the run whose compare-and-set went
/// through (its token is in `counts.run`).
struct Gate {
    id: String,
    applied: i64,
    token: String,
}

impl Gate {
    const SQL: &'static str = "EXISTS (SELECT 1 FROM roster_import WHERE id = ? \
        AND applied_rows = ? AND json_extract(counts, '$.run') = ?)";

    fn bind(&self, st: Statement) -> Statement {
        st.bind(self.id.as_str())
            .bind(self.applied)
            .bind(self.token.as_str())
    }
}

fn tags_differ(e: &Existing, rec: &Record) -> bool {
    let mut tags = rec.tags.clone();
    tags.sort();
    tags != e.roster_tags
}

/// Inserts or updates one Student. Fields are taken from the merge only
/// while `manual_overrides` still holds what the merge read (a Staff edit
/// that raced in wins); `roster_imported_at` is set either way.
fn upsert(
    s: &Services,
    rec: &Record,
    f: &crate::students::StudentFields,
    e: Option<&Existing>,
    started: i64,
    gate: &Gate,
) -> Result<Statement> {
    let now = s.now_ms();
    let held = e.map_or("[]", |e| e.raw_overrides.as_str()).to_string();
    let kept = |c: &str| {
        format!(
            "{c} = CASE WHEN student.manual_overrides = ? THEN excluded.{c} ELSE student.{c} END"
        )
    };
    let sets: Vec<String> = [
        "display_name",
        "email",
        "locale",
        "status",
        "attributes",
        "updated_at",
    ]
    .iter()
    .map(|c| kept(c))
    .collect();
    let sql = format!(
        "INSERT INTO student (id, student_code, display_name, email, locale, status, \
         attributes, roster_imported_at, created_at, updated_at) \
         SELECT ?, ?, ?, ?, ?, ?, ?, ?, ?, ? WHERE {} \
         ON CONFLICT (student_code) DO UPDATE SET {}, \
         roster_imported_at = excluded.roster_imported_at",
        Gate::SQL,
        sets.join(", ")
    );
    let attributes = serde_json::Value::Object(f.attributes.clone()).to_string();
    let st = Statement::new(sql)
        .bind(s.new_id()?)
        .bind(rec.code.as_str())
        .bind(f.display_name.as_str())
        .bind(f.email.clone())
        .bind(f.locale.0.as_str())
        .bind(f.status.as_str())
        .bind(attributes)
        .bind(started)
        .bind(now)
        .bind(now);
    Ok((0..sets.len()).fold(gate.bind(st), |st, _| st.bind(held.as_str())))
}

/// Deletes the Student's roster Tags and attaches the file's; manual Tags
/// stay.
fn replace_tags(rec: &Record, now: i64, gate: &Gate) -> Vec<Statement> {
    let clear = gate.bind(
        Statement::new(format!(
            "DELETE FROM student_tag WHERE source = 'roster' AND student_id = \
             (SELECT id FROM student WHERE student_code = ?) AND {}",
            Gate::SQL
        ))
        .bind(rec.code.as_str()),
    );
    if rec.tags.is_empty() {
        return vec![clear];
    }
    let marks = vec!["?"; rec.tags.len()].join(", ");
    let attach = Statement::new(format!(
        "INSERT INTO student_tag (student_id, tag_id, source, added_at) \
         SELECT s.id, t.id, 'roster', ? FROM student s JOIN tag t ON t.name IN ({marks}) \
         WHERE s.student_code = ? AND {} ON CONFLICT DO NOTHING",
        Gate::SQL
    ))
    .bind(now);
    let attach = rec
        .tags
        .iter()
        .fold(attach, |st, t| st.bind(t.as_str()))
        .bind(rec.code.as_str());
    vec![clear, gate.bind(attach)]
}

/// Creates the Tags the chunk names (existing names are left alone).
fn create_tags(s: &Services, names: &[String]) -> Result<Statement> {
    let now = s.now_ms();
    let rows = vec!["(?, ?, ?)"; names.len()].join(", ");
    let mut st = Statement::new(format!(
        "INSERT INTO tag (id, name, created_at) VALUES {rows} ON CONFLICT (name) DO NOTHING"
    ));
    for n in names {
        st = st.bind(s.new_id()?).bind(n.as_str()).bind(now);
    }
    Ok(st)
}

async fn last_attempt(s: &Services, job: &Job) -> bool {
    let stmt = Statement::new("SELECT max_attempts FROM job WHERE id = ?").bind(job.id.as_str());
    let max =
        s.db.query(stmt)
            .await
            .ok()
            .and_then(|r| r.get(0, "max_attempts").and_then(Value::as_i64));
    max.is_some_and(|m| i64::from(job.attempt) > m)
}

/// The failure as Staff see it: the Kernel's own messages as they are,
/// other errors by kind only (a database or platform message could quote
/// a value from the file).
pub(crate) fn describe(e: &Error) -> String {
    match e.root() {
        Error::Invalid(m) => m.clone(),
        Error::Db(_) => "database error".into(),
        Error::Conflict(_) => "conflicting change".into(),
        _ => "platform error".into(),
    }
}

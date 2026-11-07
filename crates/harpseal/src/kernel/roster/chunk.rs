//! One chunk of the import Job: rows `[applied_rows, applied_rows + 500)`
//! of the stored file (read from its parsed rows, `rows.rs`) in one
//! `Db::batch`. The batch's first statement
//! advances `applied_rows` with a compare-and-set and writes a per-run
//! token into `counts.run`; every other statement is gated on both, so a
//! repeated or concurrent run of the same chunk writes nothing. The batch
//! also queues what comes next: the next chunk, the first left batch
//! (`full`), or `done` (`upsert`, last chunk).

use std::collections::BTreeSet;

use serde_json::json;

use super::service::{JOB_KIND, job_key, left_key};
use super::store::{self, Existing, Import, Tally};
use super::validate::Record;
use crate::module::JobContext;
use crate::port_types::{Statement, Value};
use crate::students::StudentFields;
use crate::{Error, Result, Services};

/// Rows per chunk (one batch each).
pub(crate) const CHUNK: i64 = 500;
/// Tag names per `INSERT INTO tag` (3 parameters each, D1 allows 100).
const TAGS_PER_INSERT: usize = 30;

/// Applies the chunk starting at `applied_rows`; `false` when another run
/// applied it first.
pub(super) async fn apply(cx: &JobContext, imp: &Import) -> Result<bool> {
    let s = &cx.services;
    let records = super::rows::chunk(cx, imp, CHUNK as usize).await?;
    let to = imp.applied_rows + records.len() as i64;
    let codes: Vec<&str> = records.iter().map(|r| r.code.as_str()).collect();
    let existing = store::students_by_code(s, &codes).await?;

    let gate = Gate {
        id: imp.id.clone(),
        applied: to,
        token: s.random_hex(8)?,
    };
    let started = imp.started_at.unwrap_or_else(|| s.now_ms());
    let mut counts = imp.counts.clone();
    counts.run = gate.token.clone();
    let mut stmts = Vec::new();
    let mut tally = Tally::default();
    let mut tag_names = BTreeSet::new();
    for rec in &records {
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
    let mut head = vec![
        Statement::new(
            "UPDATE roster_import SET applied_rows = ?, counts = ? \
             WHERE id = ? AND state = 'running' AND applied_rows = ?",
        )
        .bind(gate.applied)
        .bind(counts_json)
        .bind(imp.id.as_str())
        .bind(imp.applied_rows),
    ];
    let tags: Vec<String> = tag_names.into_iter().collect();
    for group in tags.chunks(TAGS_PER_INSERT) {
        head.push(create_tags(s, group)?);
    }
    head.append(&mut stmts);
    head.extend(next_step(cx, imp, &gate)?);
    let done = s.db.batch(head).await?;
    Ok(done.first().map(|r| r.rows_affected) == Some(1))
}

/// What follows the chunk ending at `gate.applied`, in its batch.
fn next_step(cx: &JobContext, imp: &Import, gate: &Gate) -> Result<Vec<Statement>> {
    if gate.applied < imp.total_rows {
        let payload = json!({ "import_id": imp.id, "chunk": gate.applied });
        let key = job_key(&imp.id, gate.applied);
        return Ok(vec![
            cx.jobs.insert_unique(&key, JOB_KIND, &payload, None)?.1,
        ]);
    }
    complete(cx, imp, Some(gate))
}

/// After the last row: `full` queues the first left batch, `upsert` is
/// `done`. With `gate`, only if the chunk's compare-and-set went through.
pub(super) fn complete(
    cx: &JobContext,
    imp: &Import,
    gate: Option<&Gate>,
) -> Result<Vec<Statement>> {
    if imp.mode == "full" {
        let payload = json!({ "import_id": imp.id, "chunk": imp.total_rows, "left": 0 });
        let key = left_key(&imp.id, 0);
        return Ok(vec![
            cx.jobs.insert_unique(&key, JOB_KIND, &payload, None)?.1,
        ]);
    }
    let cond = match gate {
        Some(_) => format!("AND {}", Gate::SQL),
        None => "AND state = 'running'".to_string(),
    };
    let done = Statement::new(format!(
        "UPDATE roster_import SET state = 'done', finished_at = ? \
         WHERE id = ? AND applied_rows = total_rows {cond}"
    ))
    .bind(cx.services.now_ms())
    .bind(imp.id.as_str());
    Ok(vec![match gate {
        Some(g) => g.bind(done),
        None => done,
    }])
}

/// The batch gate: true only in the run whose compare-and-set went
/// through (its token is in `counts.run`).
pub(super) struct Gate {
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

/// Core columns a manual override can hold.
const CORE: [&str; 4] = ["display_name", "email", "locale", "status"];

/// Inserts or updates one Student. A field in `manual_overrides` keeps the
/// stored column (`col = student.col`, so a Staff edit made after the Job
/// read the row survives); attribute overrides keep their stored keys. The
/// other fields take the merge only while `manual_overrides` still holds
/// what the Job read (a field Staff overrode meanwhile wins).
/// `roster_imported_at` is set either way.
fn upsert(
    s: &Services,
    rec: &Record,
    f: &StudentFields,
    e: Option<&Existing>,
    started: i64,
    gate: &Gate,
) -> Result<Statement> {
    let now = s.now_ms();
    let held: &[String] = e.map_or(&[], |e| e.overrides.as_slice());
    let raw = e.map_or("[]", |e| e.raw_overrides.as_str()).to_string();
    let mut sets = Vec::new();
    let mut binds: Vec<Value> = Vec::new();
    let unless_raced = |c: &str| {
        format!(
            "{c} = CASE WHEN student.manual_overrides = ? THEN excluded.{c} ELSE student.{c} END"
        )
    };
    for c in CORE {
        if held.iter().any(|h| h == c) {
            sets.push(format!("{c} = student.{c}"));
        } else {
            sets.push(unless_raced(c));
            binds.push(Value::from(raw.as_str()));
        }
    }
    // Attributes: the merge, then each overridden key from the stored JSON
    // (a JSON null removes a key the stored row lacks).
    let mut attrs = "excluded.attributes".to_string();
    let mut attr_binds: Vec<Value> = Vec::new();
    for key in held.iter().filter_map(|h| h.strip_prefix("attributes.")) {
        attrs = format!("json_patch({attrs}, json_object(?, json_extract(student.attributes, ?)))");
        attr_binds.push(Value::from(key));
        attr_binds.push(Value::from(format!("$.\"{key}\"")));
    }
    sets.push(format!(
        "attributes = CASE WHEN student.manual_overrides = ? THEN {attrs} ELSE student.attributes END"
    ));
    binds.push(Value::from(raw.as_str()));
    binds.append(&mut attr_binds);
    sets.push(unless_raced("updated_at"));
    binds.push(Value::from(raw.as_str()));
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
    let mut st = gate.bind(
        Statement::new(sql)
            .bind(s.new_id()?)
            .bind(rec.code.as_str())
            .bind(f.display_name.as_str())
            .bind(crate::kernel::students::email::normalise(
                f.email.as_deref(),
            ))
            .bind(f.locale.0.as_str())
            .bind(f.status.as_str())
            .bind(attributes)
            .bind(started)
            .bind(now)
            .bind(now),
    );
    st.params.extend(binds);
    Ok(st)
}

/// Deletes the Student's roster Tags and attaches the file's (at most
/// `MAX_TAGS`, so the `IN` list stays within D1's parameter limit); manual
/// Tags stay.
fn replace_tags(rec: &Record, now: i64, gate: &Gate) -> Vec<Statement> {
    let clear = gate.bind(
        Statement::new(format!(
            "DELETE FROM student_tag WHERE source = 'roster' AND student_id = \
             (SELECT id FROM student WHERE student_code = ?) AND {}",
            Gate::SQL
        ))
        .bind(rec.code.as_str()),
    );
    let tags = &rec.tags[..rec.tags.len().min(super::validate::MAX_TAGS)];
    if tags.is_empty() {
        return vec![clear];
    }
    let marks = vec!["?"; tags.len()].join(", ");
    let attach = Statement::new(format!(
        "INSERT INTO student_tag (student_id, tag_id, source, added_at) \
         SELECT s.id, t.id, 'roster', ? FROM student s JOIN tag t ON t.name IN ({marks}) \
         WHERE s.student_code = ? AND {} ON CONFLICT DO NOTHING",
        Gate::SQL
    ))
    .bind(now);
    let attach = tags
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

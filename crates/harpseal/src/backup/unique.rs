//! Uniqueness of the rows a restore is about to insert, as SQLite will
//! judge it: the primary key and every `UNIQUE` index, partial ones (`…
//! WHERE status <> 'revoked'`) and ones on expressions included.
//!
//! A plain index over columns is checked in memory. A partial or
//! expression index is evaluated by SQLite itself over the decoded rows:
//! `WITH r(<the table's columns>) AS (VALUES (…), …) SELECT <its terms>,
//! <its WHERE> FROM r`, a few rows per query, so the expressions mean what
//! they mean in the index. Keys are compared under each term's collation
//! (`NOCASE`, `RTRIM`, `BINARY`). An index whose definition cannot be read
//! refuses the restore: unchecked, it could fail half-way.

use std::collections::HashSet;

use super::schema::{Table, quote};
use crate::port_types::{Statement, Value};
use crate::{Db, Error, Result};

/// Bound values per evaluating query (D1 accepts 100).
const PARAMS: usize = 99;

/// One `UNIQUE` index (or the primary key) as the restore checks it.
pub(super) struct Unique {
    /// Index name, `PRIMARY KEY` for the key of a table.
    pub(super) name: String,
    /// The key terms, as SQL over the table's columns.
    terms: Vec<String>,
    /// Each term's collation.
    colls: Vec<String>,
    /// The index's `WHERE`, if partial.
    filter: Option<String>,
    /// The terms when every one is a plain column and there is no `WHERE`.
    plain: Option<Vec<String>>,
}

fn text(v: Option<&Value>) -> String {
    v.and_then(Value::as_text).unwrap_or_default().to_string()
}

/// `(terms, WHERE)` of a `CREATE [UNIQUE] INDEX … ON t (terms) [WHERE x]`.
fn parse_index(sql: &str) -> Option<(Vec<String>, Option<String>)> {
    let upper = sql.to_ascii_uppercase();
    let on = upper.find(" ON ")?;
    let open = on + sql[on..].find('(')?;
    let (mut depth, mut close) = (0i32, None);
    let mut quote_char: Option<char> = None;
    for (i, c) in sql[open..].char_indices() {
        match (quote_char, c) {
            (Some(q), c) if c == q => quote_char = None,
            (Some(_), _) => {}
            (None, '\'' | '"' | '`') => quote_char = Some(c),
            (None, '(') => depth += 1,
            (None, ')') => {
                depth -= 1;
                if depth == 0 {
                    close = Some(open + i);
                    break;
                }
            }
            _ => {}
        }
    }
    let close = close?;
    let mut terms = Vec::new();
    let (mut depth, mut start) = (0i32, open + 1);
    for (i, c) in sql[open + 1..close].char_indices() {
        match c {
            '(' => depth += 1,
            ')' => depth -= 1,
            ',' if depth == 0 => {
                terms.push(sql[start..open + 1 + i].to_string());
                start = open + 2 + i;
            }
            _ => {}
        }
    }
    terms.push(sql[start..close].to_string());
    let terms = terms
        .into_iter()
        .map(|t| {
            let t = t.trim();
            let u = t.to_ascii_uppercase();
            let cut = [" ASC", " DESC"]
                .iter()
                .find_map(|s| u.ends_with(s).then(|| t.len() - s.len()));
            t[..cut.unwrap_or(t.len())].trim().to_string()
        })
        .collect();
    let rest = sql[close + 1..].trim();
    let filter = match rest.get(..5) {
        Some(w) if w.eq_ignore_ascii_case("WHERE") => Some(rest[5..].trim().to_string()),
        _ if rest.is_empty() => None,
        _ => return None,
    };
    Some((terms, filter))
}

/// The primary key and every `UNIQUE` index of `t`.
pub(super) async fn uniques(db: &dyn Db, t: &Table) -> Result<Vec<Unique>> {
    let mut out = Vec::new();
    let pk: Vec<String> = t.pk_columns().iter().map(|c| c.to_string()).collect();
    if !pk.is_empty() {
        out.push(Unique {
            name: "PRIMARY KEY".into(),
            terms: pk.clone(),
            colls: vec!["BINARY".into(); pk.len()],
            filter: None,
            plain: Some(pk),
        });
    }
    let list = db
        .query(Statement::new(format!(
            "PRAGMA index_list({})",
            quote(&t.name)
        )))
        .await?;
    for i in 0..list.len() {
        let unique = list.get(i, "unique").and_then(Value::as_i64) == Some(1);
        if !unique || text(list.get(i, "origin")) == "pk" {
            continue;
        }
        let name = text(list.get(i, "name"));
        let info = db
            .query(Statement::new(format!(
                "PRAGMA index_xinfo({})",
                quote(&name)
            )))
            .await?;
        let key: Vec<usize> = (0..info.len())
            .filter(|&r| info.get(r, "key").and_then(Value::as_i64) == Some(1))
            .collect();
        let colls = key.iter().map(|&r| text(info.get(r, "coll"))).collect();
        let named: Vec<String> = key.iter().map(|&r| text(info.get(r, "name"))).collect();
        let partial = list.get(i, "partial").and_then(Value::as_i64) == Some(1);
        let expression = named.iter().any(String::is_empty);
        if !partial && !expression {
            out.push(Unique {
                name,
                terms: named.clone(),
                colls,
                filter: None,
                plain: Some(named),
            });
            continue;
        }
        let sql = db
            .query(
                Statement::new("SELECT sql FROM sqlite_master WHERE type = 'index' AND name = ?")
                    .bind(name.as_str()),
            )
            .await?;
        let parsed = parse_index(&text(sql.get(0, "sql")))
            .filter(|(terms, filter)| terms.len() == key.len() && filter.is_some() == partial);
        let Some((terms, filter)) = parsed else {
            return Err(Error::Invalid(format!(
                "cannot check the unique index {name} on {}, so nothing was restored",
                t.name
            )));
        };
        out.push(Unique {
            name,
            terms,
            colls,
            filter,
            plain: None,
        });
    }
    Ok(out)
}

impl Unique {
    /// The SQL a dump selects beside a row to key it on this index: each
    /// term, then the index's `WHERE` when it is partial.
    pub(super) fn select(&self) -> Vec<String> {
        let mut out: Vec<String> = match &self.plain {
            Some(cols) => cols.iter().map(|c| quote(c)).collect(),
            None => self.terms.iter().map(|t| format!("({t})")).collect(),
        };
        if let Some(f) = &self.filter {
            out.push(format!("({f})"));
        }
        out
    }

    /// The row's key on this index from the values of [`Unique::select`],
    /// hashed (first 8 bytes of a SHA-256, never 0); 0 when the row has no
    /// key (a `NULL` term, or outside a partial index's `WHERE`).
    pub(super) fn hash(&self, values: &[Value]) -> u64 {
        let n = self.terms.len();
        if self.filter.is_some() {
            let kept = values
                .get(n)
                .and_then(Value::as_i64)
                .is_some_and(|k| k != 0);
            if !kept {
                return 0;
            }
        }
        let Some(k) = values.get(..n).and_then(|v| key(v, &self.colls)) else {
            return 0;
        };
        use sha2::{Digest, Sha256};
        let d = Sha256::digest(format!("{}\u{1f}{k}", self.name).as_bytes());
        let mut first = [0u8; 8];
        first.copy_from_slice(&d[..8]);
        u64::from_le_bytes(first).max(1)
    }
}

/// The `UNIQUE` indexes of `t` other than its primary key: what a dump
/// checks across the rows it read (a keyset read never repeats a key).
pub(super) async fn secondary(db: &dyn Db, t: &Table) -> Result<Vec<Unique>> {
    let mut all = uniques(db, t).await?;
    all.retain(|u| u.name != "PRIMARY KEY");
    Ok(all)
}

/// `v` as a key part under `coll`, `None` for `NULL` (never collides).
fn part(v: &Value, coll: &str) -> Option<String> {
    let s = match v {
        Value::Null => return None,
        Value::Integer(i) => format!("i{i}"),
        Value::Real(x) if x.fract() == 0.0 && x.abs() < 9e15 => format!("i{}", *x as i64),
        Value::Real(x) => format!("r{x}"),
        Value::Text(s) if coll.eq_ignore_ascii_case("NOCASE") => {
            format!("t{}", s.to_ascii_lowercase())
        }
        Value::Text(s) if coll.eq_ignore_ascii_case("RTRIM") => {
            format!("t{}", s.trim_end_matches(' '))
        }
        Value::Text(s) => format!("t{s}"),
        Value::Blob(b) => format!("b{}", crate::id::hex(b)),
    };
    Some(s)
}

fn key(values: &[Value], colls: &[String]) -> Option<String> {
    let mut out = String::new();
    for (v, c) in values.iter().zip(colls) {
        out.push_str(&part(v, c)?);
        out.push('\u{1f}');
    }
    Some(out)
}

/// How many rows of `rows` (named by `columns`) repeat a key of `u`.
pub(super) async fn repeats(
    db: &dyn Db,
    u: &Unique,
    columns: &[String],
    rows: &[Vec<Value>],
) -> Result<usize> {
    let mut seen = HashSet::new();
    let mut dup = 0;
    if let Some(cols) = &u.plain {
        let idx: Option<Vec<usize>> = cols
            .iter()
            .map(|c| columns.iter().position(|n| n == c))
            .collect();
        let Some(idx) = idx else {
            return Ok(0);
        };
        for r in rows {
            let vals: Vec<Value> = idx.iter().map(|&i| r[i].clone()).collect();
            if let Some(k) = key(&vals, &u.colls) {
                dup += usize::from(!seen.insert(k));
            }
        }
        return Ok(dup);
    }
    let names: Vec<String> = columns.iter().map(|c| quote(c)).collect();
    let marks = format!("({})", vec!["?"; columns.len()].join(", "));
    let filter = u.filter.as_deref().unwrap_or("1");
    let per = (PARAMS / columns.len().max(1)).max(1);
    for chunk in rows.chunks(per) {
        let values = vec![marks.as_str(); chunk.len()].join(", ");
        let sql = format!(
            "WITH r({}) AS (VALUES {values}) SELECT {}, ({filter}) AS __kept FROM r",
            names.join(", "),
            u.terms.join(", ")
        );
        let params = chunk.iter().flatten().cloned().collect();
        let got = db
            .query(Statement {
                sql: sql.into(),
                params,
            })
            .await?;
        for r in &got.rows {
            let kept = r.last().and_then(Value::as_i64).is_some_and(|k| k != 0);
            if !kept {
                continue;
            }
            if let Some(k) = key(&r[..u.terms.len()], &u.colls) {
                dup += usize::from(!seen.insert(k));
            }
        }
    }
    Ok(dup)
}

#[cfg(test)]
mod tests {
    use super::parse_index;

    #[test]
    fn index_definitions_are_read() {
        let (t, w) = parse_index(
            "CREATE UNIQUE INDEX link_live_student_channel ON link (student_id, channel)\n    \
             WHERE status <> 'revoked'",
        )
        .unwrap();
        assert_eq!(t, ["student_id", "channel"]);
        assert_eq!(w.as_deref(), Some("status <> 'revoked'"));
        let (t, w) = parse_index(
            "CREATE UNIQUE INDEX x ON t (lower(email) COLLATE NOCASE DESC, substr(a, 1, 2))",
        )
        .unwrap();
        assert_eq!(t, ["lower(email) COLLATE NOCASE", "substr(a, 1, 2)"]);
        assert!(w.is_none());
        assert!(parse_index("CREATE UNIQUE INDEX x ON t (a) junk").is_none());
    }
}

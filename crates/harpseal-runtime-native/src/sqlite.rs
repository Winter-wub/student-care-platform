//! `Db` over rusqlite. `batch` is one transaction, rolled back on any
//! failure. Foreign keys are on, as on D1. Every call runs on tokio's
//! blocking pool so a slow query never stalls the async workers; the
//! connection is shared behind a mutex (SQLite serialises writers anyway).

use std::sync::{Arc, Mutex, MutexGuard};

use async_trait::async_trait;
use harpseal::{Db, Error, ExecResult, Result, Rows, Statement, Value};
use rusqlite::types::Value as SqlValue;

/// A SQLite database file (or an in-memory database for tests).
///
/// Values come back typed as SQLite stored them (`INTEGER` -> `Integer`,
/// `REAL` -> `Real`), which is exact; D1 cannot tell whole `REAL`s from
/// `INTEGER`s, so code must read integers with [`Value::as_i64`].
#[derive(Clone)]
pub struct SqliteDb {
    conn: Arc<Mutex<rusqlite::Connection>>,
}

fn db_err(e: rusqlite::Error) -> Error {
    Error::Db(e.to_string())
}

fn to_sql(v: &Value) -> SqlValue {
    match v {
        Value::Integer(i) => SqlValue::Integer(*i),
        Value::Real(f) => SqlValue::Real(*f),
        Value::Text(s) => SqlValue::Text(s.clone()),
        Value::Blob(b) => SqlValue::Blob(b.clone()),
        _ => SqlValue::Null,
    }
}

fn from_sql(v: SqlValue) -> Value {
    match v {
        SqlValue::Null => Value::Null,
        SqlValue::Integer(i) => Value::Integer(i),
        SqlValue::Real(f) => Value::Real(f),
        SqlValue::Text(s) => Value::Text(s),
        SqlValue::Blob(b) => Value::Blob(b),
    }
}

impl SqliteDb {
    /// Opens (or creates) the database file at `path`.
    pub fn open(path: &str) -> Result<Self> {
        Self::with(rusqlite::Connection::open(path).map_err(db_err)?)
    }

    /// A private in-memory database, empty on every call.
    pub fn memory() -> Result<Self> {
        Self::with(rusqlite::Connection::open_in_memory().map_err(db_err)?)
    }

    fn with(conn: rusqlite::Connection) -> Result<Self> {
        conn.execute_batch("PRAGMA foreign_keys = ON;")
            .map_err(db_err)?;
        conn.busy_timeout(std::time::Duration::from_secs(5))
            .map_err(db_err)?;
        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
        })
    }

    fn lock(&self) -> MutexGuard<'_, rusqlite::Connection> {
        self.conn.lock().unwrap_or_else(|p| p.into_inner())
    }

    fn query_sync(&self, s: &Statement) -> Result<Rows> {
        let conn = self.lock();
        let mut st = conn.prepare(&s.sql).map_err(db_err)?;
        let columns: Vec<String> = st.column_names().map(|c| c.to_string()).collect();
        let n = columns.len();
        let mut out = Rows {
            columns,
            rows: Vec::new(),
        };
        let mut rows = st
            .query(rusqlite::params_from_iter(s.params.iter().map(to_sql)))
            .map_err(db_err)?;
        while let Some(row) = rows.next().map_err(db_err)? {
            let vals = (0..n)
                .map(|i| row.get::<_, SqlValue>(i).map(from_sql))
                .collect::<rusqlite::Result<_>>();
            out.rows.push(vals.map_err(db_err)?);
        }
        Ok(out)
    }

    fn exec_on(conn: &rusqlite::Connection, s: &Statement) -> Result<ExecResult> {
        let n = conn
            .execute(
                &s.sql,
                rusqlite::params_from_iter(s.params.iter().map(to_sql)),
            )
            .map_err(db_err)?;
        Ok(ExecResult {
            rows_affected: n as u64,
        })
    }

    fn batch_sync(&self, stmts: &[Statement]) -> Result<Vec<ExecResult>> {
        let mut conn = self.lock();
        // Dropping `tx` without commit (the `?` below) rolls everything back.
        let tx = conn.transaction().map_err(db_err)?;
        let out = stmts
            .iter()
            .map(|s| Self::exec_on(&tx, s))
            .collect::<Result<Vec<_>>>()?;
        tx.commit().map_err(db_err)?;
        Ok(out)
    }

    /// Runs `f` on tokio's blocking pool.
    async fn blocking<T: Send + 'static>(
        &self,
        f: impl FnOnce(&SqliteDb) -> Result<T> + Send + 'static,
    ) -> Result<T> {
        let db = self.clone();
        tokio::task::spawn_blocking(move || f(&db))
            .await
            .map_err(|e| Error::Db(format!("sqlite task failed: {e}")))?
    }
}

#[async_trait]
impl Db for SqliteDb {
    async fn query(&self, s: Statement) -> Result<Rows> {
        self.blocking(move |db| db.query_sync(&s)).await
    }
    async fn execute(&self, s: Statement) -> Result<ExecResult> {
        self.blocking(move |db| Self::exec_on(&db.lock(), &s)).await
    }
    async fn batch(&self, s: Vec<Statement>) -> Result<Vec<ExecResult>> {
        self.blocking(move |db| db.batch_sync(&s)).await
    }
}

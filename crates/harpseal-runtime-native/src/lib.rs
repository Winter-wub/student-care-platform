//! Native runtime: a single tokio binary on the customer's own server.
//! Implements the runtime ports : SQLite file via
//! rusqlite, a local directory as FileStore, the OS clock and RNG. SMTP and
//! outgoing HTTP are `todo!()` in this prototype.

use std::path::PathBuf;
use std::sync::Mutex;

use harpseal::delivery::Timestamp;
use harpseal::manifest::Migration;
use harpseal::port_types::{ExecResult, Rows, Statement, Value};
use harpseal::{Error, Result};
use rusqlite::types::Value as SqlValue;

pub struct SqliteDb {
    // Prototype: blocking calls inside async fns. Real one: spawn_blocking.
    conn: Mutex<rusqlite::Connection>,
}

fn db_err(e: rusqlite::Error) -> Error {
    Error::Db(e.to_string())
}

fn to_sql(v: &Value) -> SqlValue {
    match v {
        Value::Null => SqlValue::Null,
        Value::Integer(i) => SqlValue::Integer(*i),
        Value::Real(f) => SqlValue::Real(*f),
        Value::Text(s) => SqlValue::Text(s.clone()),
        Value::Blob(b) => SqlValue::Blob(b.clone()),
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
    pub fn open(path: &str) -> Result<Self> {
        let conn = rusqlite::Connection::open(path).map_err(db_err)?;
        Ok(Self { conn: Mutex::new(conn) })
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, rusqlite::Connection> {
        self.conn.lock().unwrap_or_else(|p| p.into_inner())
    }

    fn query_sync(&self, s: &Statement) -> Result<Rows> {
        let conn = self.lock();
        let mut st = conn.prepare(&s.sql).map_err(db_err)?;
        let columns: Vec<String> = st.column_names().map(|c| c.to_string()).collect();
        let n = columns.len();
        let mut out = Rows { columns, rows: Vec::new() };
        let mut rows = st.query(rusqlite::params_from_iter(s.params.iter().map(to_sql))).map_err(db_err)?;
        while let Some(row) = rows.next().map_err(db_err)? {
            let vals = (0..n).map(|i| row.get::<_, SqlValue>(i).map(from_sql)).collect::<rusqlite::Result<_>>();
            out.rows.push(vals.map_err(db_err)?);
        }
        Ok(out)
    }

    fn exec_on(conn: &rusqlite::Connection, s: &Statement) -> Result<ExecResult> {
        let n = conn
            .execute(&s.sql, rusqlite::params_from_iter(s.params.iter().map(to_sql)))
            .map_err(db_err)?;
        Ok(ExecResult { rows_affected: n as u64, last_insert_rowid: Some(conn.last_insert_rowid()) })
    }

    fn execute_sync(&self, s: &Statement) -> Result<ExecResult> {
        Self::exec_on(&self.lock(), s)
    }

    fn batch_sync(&self, stmts: &[Statement]) -> Result<Vec<ExecResult>> {
        let mut conn = self.lock();
        let tx = conn.transaction().map_err(db_err)?;
        let out = stmts.iter().map(|s| Self::exec_on(&tx, s)).collect::<Result<Vec<_>>>()?;
        tx.commit().map_err(db_err)?;
        Ok(out)
    }

    /// Applies migrations not yet recorded. Same for both designs.
    pub fn migrate(&self, migrations: &[Migration]) -> Result<()> {
        let conn = self.lock();
        conn.execute_batch("CREATE TABLE IF NOT EXISTS _migration (module TEXT, name TEXT, PRIMARY KEY (module, name))")
            .map_err(db_err)?;
        for m in migrations {
            let done: bool = conn
                .query_row("SELECT 1 FROM _migration WHERE module = ?1 AND name = ?2", [m.module, m.name], |_| Ok(true))
                .unwrap_or(false);
            if !done {
                conn.execute_batch(m.sql).map_err(db_err)?;
                conn.execute("INSERT INTO _migration VALUES (?1, ?2)", [m.module, m.name]).map_err(db_err)?;
            }
        }
        Ok(())
    }
}

pub struct LocalFiles {
    pub root: PathBuf,
}

impl LocalFiles {
    fn path(&self, key: &str) -> Result<PathBuf> {
        if key.contains("..") {
            return Err(Error::Invalid("bad file key".into()));
        }
        Ok(self.root.join(key))
    }
    fn put_sync(&self, key: &str, bytes: &[u8]) -> Result<()> {
        let p = self.path(key)?;
        if let Some(dir) = p.parent() {
            std::fs::create_dir_all(dir).map_err(|e| Error::Platform(e.to_string()))?;
        }
        std::fs::write(p, bytes).map_err(|e| Error::Platform(e.to_string()))
    }
    fn get_sync(&self, key: &str) -> Result<Option<Vec<u8>>> {
        match std::fs::read(self.path(key)?) {
            Ok(b) => Ok(Some(b)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(Error::Platform(e.to_string())),
        }
    }
    fn delete_sync(&self, key: &str) -> Result<()> {
        std::fs::remove_file(self.path(key)?).or_else(|e| match e.kind() {
            std::io::ErrorKind::NotFound => Ok(()),
            _ => Err(Error::Platform(e.to_string())),
        })
    }
}

pub struct Smtp {
    pub url: String,
}
#[derive(Clone, Copy)]
pub struct NativeHttp;
pub struct SystemClock;
pub struct OsRandom;

impl SystemClock {
    fn now_sync() -> Timestamp {
        let d = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default();
        Timestamp(d.as_millis() as i64)
    }
}

fn fill_random(buf: &mut [u8]) -> Result<()> {
    getrandom::fill(buf).map_err(|e| Error::Platform(e.to_string()))
}

/// Settings read from the environment / CLI flags.
pub struct NativeSettings {
    pub sqlite_path: String,
    pub files_root: PathBuf,
    pub smtp_url: String,
}

mod a;

pub use a::services;

/// `axum::serve` on a TCP listener.
pub async fn serve(router: axum::Router, addr: &str) -> std::io::Result<()> {
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, router).await
}

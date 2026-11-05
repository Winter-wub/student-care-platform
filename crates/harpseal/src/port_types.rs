//! Plain data crossing the runtime ports (Db, FileStore, EmailSender).

/// A SQLite value. Integers beyond ±2^53−1 are unsafe on D1 (they cross as
/// JS numbers): use text ids.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Null,
    Integer(i64),
    Real(f64),
    Text(String),
    Blob(Vec<u8>),
}

impl From<&str> for Value {
    fn from(s: &str) -> Self {
        Value::Text(s.into())
    }
}
impl From<String> for Value {
    fn from(s: String) -> Self {
        Value::Text(s)
    }
}
impl From<i64> for Value {
    fn from(i: i64) -> Self {
        Value::Integer(i)
    }
}

/// One SQL statement in the SQLite dialect with positional `?` params.
#[derive(Debug, Clone)]
pub struct Statement {
    pub sql: std::borrow::Cow<'static, str>,
    pub params: Vec<Value>,
}

impl Statement {
    pub fn new(sql: &'static str) -> Self {
        Self {
            sql: sql.into(),
            params: Vec::new(),
        }
    }
    pub fn bind(mut self, v: impl Into<Value>) -> Self {
        self.params.push(v.into());
        self
    }
}

/// Result of a query: column names plus rows in the same order.
#[derive(Debug, Clone, Default)]
pub struct Rows {
    pub columns: Vec<String>,
    pub rows: Vec<Vec<Value>>,
}

/// Result of an execute.
#[derive(Debug, Clone, Copy, Default)]
pub struct ExecResult {
    pub rows_affected: u64,
    pub last_insert_rowid: Option<i64>,
}

/// One email (the email-OTP Verification Method sends these).
#[derive(Debug, Clone)]
pub struct Email {
    pub to: String,
    pub subject: String,
    pub text: String,
    pub html: Option<String>,
}

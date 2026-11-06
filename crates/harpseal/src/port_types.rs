//! Plain data crossing the runtime ports (Db, EmailSender).

use std::borrow::Cow;

/// A SQLite value. Integers beyond ±2^53−1 are unsafe on D1 (they cross as
/// JS numbers): ids are text (ADR-0007).
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum Value {
    /// SQL `NULL`.
    Null,
    /// 64-bit integer (timestamps are milliseconds, well inside ±2^53).
    Integer(i64),
    /// 64-bit float.
    Real(f64),
    /// UTF-8 text.
    Text(String),
    /// Bytes.
    Blob(Vec<u8>),
}

impl Value {
    /// The text, if this is `Text`.
    pub fn as_text(&self) -> Option<&str> {
        match self {
            Value::Text(s) => Some(s),
            _ => None,
        }
    }
    /// The integer, if this is `Integer` (D1 may return whole `Real`s).
    pub fn as_i64(&self) -> Option<i64> {
        match self {
            Value::Integer(i) => Some(*i),
            Value::Real(f) if f.fract() == 0.0 => Some(*f as i64),
            _ => None,
        }
    }
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
impl From<bool> for Value {
    fn from(b: bool) -> Self {
        Value::Integer(i64::from(b))
    }
}
impl<T: Into<Value>> From<Option<T>> for Value {
    fn from(v: Option<T>) -> Self {
        v.map_or(Value::Null, Into::into)
    }
}

/// One SQL statement in the SQLite dialect with positional `?` params.
#[derive(Debug, Clone)]
pub struct Statement {
    /// The SQL text.
    pub sql: Cow<'static, str>,
    /// Values bound to `?` in order.
    pub params: Vec<Value>,
}

impl Statement {
    /// A statement with no params yet.
    pub fn new(sql: impl Into<Cow<'static, str>>) -> Self {
        Self {
            sql: sql.into(),
            params: Vec::new(),
        }
    }
    /// Binds the next `?`.
    pub fn bind(mut self, v: impl Into<Value>) -> Self {
        self.params.push(v.into());
        self
    }
}

/// Result of a query: column names plus rows in the same order.
#[derive(Debug, Clone, Default)]
pub struct Rows {
    /// Column names.
    pub columns: Vec<String>,
    /// Rows, each with one value per column.
    pub rows: Vec<Vec<Value>>,
}

impl Rows {
    /// The value of `column` in row `row`.
    pub fn get(&self, row: usize, column: &str) -> Option<&Value> {
        let i = self.columns.iter().position(|c| c == column)?;
        self.rows.get(row)?.get(i)
    }
    /// Number of rows.
    pub fn len(&self) -> usize {
        self.rows.len()
    }
    /// No rows.
    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }
}

/// Result of an execute.
#[derive(Debug, Clone, Copy, Default)]
pub struct ExecResult {
    /// Rows inserted, updated or deleted.
    pub rows_affected: u64,
}

/// One email (the email-OTP Verification Method sends these).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Email {
    /// Recipient address.
    pub to: String,
    /// Subject line.
    pub subject: String,
    /// Plain text body.
    pub text: String,
    /// Optional HTML body.
    pub html: Option<String>,
}

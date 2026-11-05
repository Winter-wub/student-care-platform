//! `Db` over a D1 binding. Every JS-backed future is wrapped in
//! `SendFuture`: the Core's traits require `Send`, and a Worker isolate runs
//! one thread, so nothing is ever moved across threads.

use async_trait::async_trait;
use harpseal::port_types::{ExecResult, Rows, Statement, Value};
use harpseal::{Error, Result};
use worker::js_sys::{self, Array, Function, Object, Reflect, Uint8Array};
use worker::send::SendFuture;
use worker::wasm_bindgen::{JsCast, JsValue};
use worker::wasm_bindgen_futures::JsFuture;
use worker::{D1Database, D1PreparedStatement, D1Result};

/// D1 has no interactive transactions; `batch` is D1's own atomic batch.
pub struct D1Db {
    db: D1Database,
}

impl D1Db {
    pub fn new(db: D1Database) -> Self {
        Self { db }
    }
}

fn db_err(e: impl core::fmt::Display) -> Error {
    Error::Db(e.to_string())
}

fn js_err(e: JsValue) -> Error {
    let msg = e
        .dyn_ref::<js_sys::Error>()
        .map(|e| String::from(e.message()))
        .or_else(|| e.as_string())
        .unwrap_or_else(|| format!("{e:?}"));
    Error::Db(msg)
}

/// Integers cross as JS numbers: exact up to ±2^53−1 only.
fn to_js(v: &Value) -> JsValue {
    match v {
        Value::Null => JsValue::NULL,
        Value::Integer(i) => JsValue::from_f64(*i as f64),
        Value::Real(f) => JsValue::from_f64(*f),
        Value::Text(s) => JsValue::from_str(s),
        Value::Blob(b) => Uint8Array::from(b.as_slice()).into(),
    }
}

const MAX_SAFE: f64 = 9_007_199_254_740_991.0;

fn from_js(v: JsValue) -> Value {
    if v.is_null() || v.is_undefined() {
        return Value::Null;
    }
    if let Some(f) = v.as_f64() {
        // D1 does not tell INTEGER from REAL; whole numbers read as Integer.
        return if f.fract() == 0.0 && f.abs() <= MAX_SAFE {
            Value::Integer(f as i64)
        } else {
            Value::Real(f)
        };
    }
    if let Some(s) = v.as_string() {
        return Value::Text(s);
    }
    if v.is_instance_of::<js_sys::ArrayBuffer>() || v.is_instance_of::<Uint8Array>() {
        return Value::Blob(Uint8Array::new(&v).to_vec());
    }
    if let Some(a) = v.dyn_ref::<Array>() {
        let bytes = a.iter().map(|b| b.as_f64().unwrap_or(0.0) as u8);
        return Value::Blob(bytes.collect());
    }
    Value::Text(format!("{v:?}"))
}

fn prepare(db: &D1Database, s: &Statement) -> Result<D1PreparedStatement> {
    let params: Vec<JsValue> = s.params.iter().map(to_js).collect();
    db.prepare(s.sql.as_ref()).bind(&params).map_err(db_err)
}

fn exec_result(r: &D1Result) -> Result<ExecResult> {
    if let Some(e) = r.error() {
        return Err(Error::Db(e));
    }
    let meta = r.meta().map_err(db_err)?;
    Ok(ExecResult {
        rows_affected: meta.as_ref().and_then(|m| m.changes).unwrap_or(0) as u64,
        last_insert_rowid: meta.and_then(|m| m.last_row_id),
    })
}

/// `stmt.raw({ columnNames: true })`: the first row is the column names.
/// workers-rs 0.8 only exposes `raw()` without options.
async fn raw_with_columns(stmt: &D1PreparedStatement) -> Result<Rows> {
    let inner: &JsValue = stmt.inner().as_ref();
    let raw: Function = Reflect::get(inner, &"raw".into())
        .map_err(js_err)?
        .dyn_into()
        .map_err(js_err)?;
    let opts = Object::new();
    Reflect::set(&opts, &"columnNames".into(), &JsValue::TRUE).map_err(js_err)?;
    let promise: js_sys::Promise = raw
        .call1(inner, &opts)
        .map_err(js_err)?
        .dyn_into()
        .map_err(js_err)?;
    let out: Array = JsFuture::from(promise)
        .await
        .map_err(js_err)?
        .dyn_into()
        .map_err(js_err)?;
    let mut it = out.iter();
    let columns = match it.next() {
        Some(cols) => Array::from(&cols)
            .iter()
            .map(|c| c.as_string().unwrap_or_default())
            .collect(),
        None => Vec::new(),
    };
    let rows = it
        .map(|r| Array::from(&r).iter().map(from_js).collect())
        .collect();
    Ok(Rows { columns, rows })
}

#[async_trait]
impl harpseal::Db for D1Db {
    async fn query(&self, s: Statement) -> Result<Rows> {
        SendFuture::new(async move {
            let stmt = prepare(&self.db, &s)?;
            raw_with_columns(&stmt).await
        })
        .await
    }

    async fn execute(&self, s: Statement) -> Result<ExecResult> {
        SendFuture::new(async move {
            let stmt = prepare(&self.db, &s)?;
            let r = stmt.run().await.map_err(db_err)?;
            exec_result(&r)
        })
        .await
    }

    async fn batch(&self, stmts: Vec<Statement>) -> Result<Vec<ExecResult>> {
        SendFuture::new(async move {
            let prepared = stmts
                .iter()
                .map(|s| prepare(&self.db, s))
                .collect::<Result<Vec<_>>>()?;
            let results = self.db.batch(prepared).await.map_err(db_err)?;
            results.iter().map(exec_result).collect()
        })
        .await
    }
}

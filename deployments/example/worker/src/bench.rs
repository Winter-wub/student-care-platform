//! `/bench/*`: issue #5 measurements, run on the deployed Worker.
//!
//! Timing: Workers freeze `Date.now()` during CPU work and advance it only
//! at I/O (Spectre mitigation). So `cpu_loop_ms` below is usually 0; the
//! routes then await two cheap D1 queries and report
//! `est_ms = (after first I/O − start) − (second I/O round trip)`.
//! The authoritative number is `cpuTime` from `wrangler tail`.

use axum::Router;
use axum::extract::{Query, State};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use harpseal::Services;
use harpseal::password::{Scheme, hash};
use harpseal::port_types::Statement;
use serde::Deserialize;
use worker::js_sys::{self, Array, Reflect, Uint8Array};
use worker::send::SendFuture;
use worker::wasm_bindgen::{JsCast, JsValue};
use worker::wasm_bindgen_futures::JsFuture;

const PASSWORD: &str = "seal-bay-proto-2026";
const SALT: &[u8] = b"bench-salt-16byt";

pub fn router(services: Services) -> Router {
    let r = Router::new()
        .route("/bench/hash", get(hash_bench))
        .route("/bench/batch", post(batch_bench));
    #[cfg(feature = "xlsx")]
    let r = r.route("/bench/xlsx", post(xlsx_bench));
    r.with_state(services)
}

fn now() -> f64 {
    js_sys::Date::now()
}

fn json(body: String) -> Response {
    ([("content-type", "application/json")], body).into_response()
}

/// Two `SELECT 1` round trips: returns (time after the first, its length).
async fn io_fence(s: &Services) -> harpseal::Result<(f64, f64)> {
    s.db.query(Statement::new("SELECT 1")).await?;
    let t1 = now();
    s.db.query(Statement::new("SELECT 1")).await?;
    Ok((t1, now() - t1))
}

#[derive(Deserialize)]
pub struct HashQ {
    algo: String,
    #[serde(default = "one")]
    n: u32,
    /// Argon2id memory in MiB (default 19).
    m: Option<u32>,
    /// PBKDF2 iterations (default 600 000).
    i: Option<u32>,
}
fn one() -> u32 {
    1
}

/// PBKDF2-HMAC-SHA256 via `crypto.subtle.deriveBits` (runs in the
/// runtime's C++, not in wasm).
async fn webcrypto_pbkdf2(iterations: u32) -> Result<Vec<u8>, String> {
    let fmt = |e: JsValue| {
        e.dyn_ref::<js_sys::Error>()
            .map(|e| String::from(e.message()))
            .unwrap_or_else(|| format!("{e:?}"))
    };
    let crypto: web_sys::Crypto = Reflect::get(&js_sys::global(), &"crypto".into())
        .map_err(fmt)?
        .unchecked_into();
    let subtle = crypto.subtle();
    let key_data = Uint8Array::from(PASSWORD.as_bytes());
    let usages = Array::of1(&"deriveBits".into());
    let key = subtle
        .import_key_with_str("raw", &key_data, "PBKDF2", false, &usages)
        .map_err(fmt)?;
    let key: web_sys::CryptoKey = JsFuture::from(key).await.map_err(fmt)?.unchecked_into();
    let salt = Uint8Array::from(SALT);
    let params = web_sys::Pbkdf2Params::new("PBKDF2", &"SHA-256".into(), iterations, &salt);
    let bits = subtle
        .derive_bits_with_object(&params, &key, 256)
        .map_err(fmt)?;
    let bits = JsFuture::from(bits).await.map_err(fmt)?;
    Ok(Uint8Array::new(&bits).to_vec())
}

/// `GET /bench/hash?algo=argon2id|pbkdf2|pbkdf2-webcrypto&n=&m=&i=`
async fn hash_bench(State(s): State<Services>, Query(q): Query<HashQ>) -> Response {
    let n = q.n.clamp(1, 20);
    let iterations = q.i.unwrap_or(600_000);
    let scheme = match q.algo.as_str() {
        "argon2id" => Scheme::Argon2id {
            m_kib: q.m.unwrap_or(19).clamp(1, 96) * 1024,
            t: 2,
            p: 1,
        },
        "pbkdf2" => Scheme::Pbkdf2Sha256 { rounds: iterations },
        "pbkdf2-webcrypto" => {
            let t0 = now();
            let mut last = Ok(Vec::new());
            for _ in 0..n {
                last = SendFuture::new(webcrypto_pbkdf2(iterations)).await;
                if last.is_err() {
                    break;
                }
            }
            let ms = now() - t0;
            let body = match last {
                Ok(bits) => format!(
                    r#"{{"algo":"pbkdf2-webcrypto","iterations":{iterations},"n":{n},"total_ms":{ms},"ms_per_hash":{},"bytes":{}}}"#,
                    ms / f64::from(n),
                    bits.len()
                ),
                Err(e) => format!(
                    r#"{{"algo":"pbkdf2-webcrypto","iterations":{iterations},"error":{:?}}}"#,
                    e
                ),
            };
            return json(body);
        }
        other => return json(format!(r#"{{"error":"unknown algo {other}"}}"#)),
    };
    let t0 = now();
    let mut phc = String::new();
    for _ in 0..n {
        match hash(scheme, PASSWORD, SALT) {
            Ok(h) => phc = h,
            Err(e) => return json(format!(r#"{{"error":{:?}}}"#, e.to_string())),
        }
    }
    let cpu_loop_ms = now() - t0;
    let (t1, io_ms) = match io_fence(&s).await {
        Ok(v) => v,
        Err(e) => return json(format!(r#"{{"error":{:?}}}"#, e.to_string())),
    };
    let est = ((t1 - t0) - io_ms).max(0.0);
    json(format!(
        r#"{{"algo":"{}","n":{n},"cpu_loop_ms":{cpu_loop_ms},"io_ms":{io_ms},"est_total_ms":{est},"est_ms_per_hash":{},"phc":{:?}}}"#,
        q.algo,
        est / f64::from(n),
        phc
    ))
}

#[derive(Deserialize)]
pub struct BatchQ {
    #[serde(default = "five_hundred")]
    n: usize,
    #[serde(default)]
    fail: u8,
}
fn five_hundred() -> usize {
    500
}

fn upsert(code: String, name: Option<String>, run: &str, at: i64) -> Statement {
    let mut st = Statement::new(
        "INSERT INTO bench_roster (student_code, display_name, email, run, updated_at) \
         VALUES (?, ?, ?, ?, ?) ON CONFLICT (student_code) DO UPDATE SET \
         display_name = excluded.display_name, email = excluded.email, \
         run = excluded.run, updated_at = excluded.updated_at",
    )
    .bind(code.clone());
    st.params.push(match name {
        Some(n) => n.into(),
        None => harpseal::port_types::Value::Null, // NOT NULL -> the batch fails
    });
    st.bind(format!("{code}@student.sealbay.example"))
        .bind(run)
        .bind(at)
}

/// `POST /bench/batch?n=500&fail=0|1`: one `Db::batch` of `n` upserts. With
/// `fail=1` the last statement violates NOT NULL; the response then counts
/// rows of that run (0 = the batch rolled back as a whole).
async fn batch_bench(State(s): State<Services>, Query(q): Query<BatchQ>) -> Response {
    let n = q.n.clamp(1, 1000);
    let fail = q.fail == 1;
    let at = s.clock.now().0;
    let run = format!("run-{at}");
    let prefix = if fail { "F" } else { "B" };
    let stmts = (0..n)
        .map(|i| {
            let name = (!fail || i + 1 < n).then(|| format!("นักศึกษาทดสอบ {i}"));
            upsert(format!("{prefix}{i:05}"), name, &run, at)
        })
        .collect();
    let t0 = now();
    let outcome = s.db.batch(stmts).await;
    let ms = now() - t0;
    let count = Statement::new("SELECT count(*) FROM bench_roster WHERE run = ?").bind(run.clone());
    let written = match s.db.query(count).await {
        Ok(r) => match r.rows.first().and_then(|r| r.first()) {
            Some(harpseal::port_types::Value::Integer(c)) => *c,
            _ => -1,
        },
        Err(_) => -1,
    };
    let (ok, err) = match &outcome {
        Ok(rs) => (rs.len(), String::new()),
        Err(e) => (0, e.to_string()),
    };
    json(format!(
        r#"{{"n":{n},"fail":{fail},"batch_ms":{ms},"statements_ok":{ok},"rows_with_this_run":{written},"error":{err:?}}}"#
    ))
}

/// `POST /bench/xlsx` (feature `xlsx`): body = an .xlsx file; returns the
/// first sheet's size and header row.
#[cfg(feature = "xlsx")]
async fn xlsx_bench(body: axum::body::Bytes) -> Response {
    use calamine::{Reader, Xlsx};
    let t0 = now();
    let mut wb = match Xlsx::new(std::io::Cursor::new(body.to_vec())) {
        Ok(wb) => wb,
        Err(e) => return json(format!(r#"{{"error":{:?}}}"#, e.to_string())),
    };
    let range = match wb.worksheet_range_at(0) {
        Some(Ok(r)) => r,
        Some(Err(e)) => return json(format!(r#"{{"error":{:?}}}"#, e.to_string())),
        None => return json(r#"{"error":"no sheet"}"#.into()),
    };
    let header: Vec<String> = range
        .rows()
        .next()
        .map(|r| r.iter().map(|c| c.to_string()).collect())
        .unwrap_or_default();
    let (h, w) = range.get_size();
    json(format!(
        r#"{{"rows":{h},"cols":{w},"header":{header:?},"cpu_ms":{}}}"#,
        now() - t0
    ))
}

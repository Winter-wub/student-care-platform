//! `GET /admin/jobs.json`: the Job runner's state for debugging, guarded
//! by `kernel.audit.view` (the Kernel registers it as a back-office route,
//! so the Staff guard of #21 applies; no navigation entry, no HTML).
//!
//! ```json
//! { "now": 1793836800000,
//!   "counts": { "queued": 2, "running": 1, "done": 40, "failed": 1 },
//!   "recent": [ { "id": "…", "kind": "demo.x", "state": "failed",
//!                 "attempts": 4, "max_attempts": 3, "last_error": "…",
//!                 "progress": null, "run_at": 1793836800000,
//!                 "locked_until": null, "finished_at": 1793836860000 } ] }
//! ```
//!
//! `recent` holds the 50 newest `running` and `failed` Jobs, by `run_at`.
//! Payloads are never shown (they may hold personal data).

use axum::extract::State;
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};
use serde_json::{Map, Value as Json, json};

use crate::port_types::{Rows, Statement, Value};
use crate::{AppState, Result};

/// Path under `/admin`.
pub(crate) const PATH: &str = "/jobs.json";
/// Rows listed in `recent`.
const RECENT: i64 = 50;

fn cell(rows: &Rows, i: usize, column: &str) -> Json {
    match rows.get(i, column) {
        Some(Value::Integer(n)) => json!(n),
        Some(Value::Real(f)) => json!(f),
        Some(Value::Text(s)) => json!(s),
        _ => Json::Null,
    }
}

async fn body(s: &AppState) -> Result<Json> {
    let db = s.services.db.as_ref();
    let counts = db
        .query(Statement::new(
            "SELECT state, count(*) AS n FROM job GROUP BY state",
        ))
        .await?;
    let mut by_state = Map::new();
    for state in ["queued", "running", "done", "failed"] {
        by_state.insert(state.into(), json!(0));
    }
    for i in 0..counts.len() {
        if let Json::String(state) = cell(&counts, i, "state") {
            by_state.insert(state, cell(&counts, i, "n"));
        }
    }
    let columns = [
        "id",
        "kind",
        "state",
        "attempts",
        "max_attempts",
        "last_error",
        "progress",
        "run_at",
        "locked_until",
        "finished_at",
    ];
    let recent = db
        .query(
            Statement::new(
                "SELECT id, kind, state, attempts, max_attempts, last_error, progress, \
                 run_at, locked_until, finished_at FROM job \
                 WHERE state IN ('running', 'failed') ORDER BY run_at DESC, id DESC LIMIT ?",
            )
            .bind(RECENT),
        )
        .await?;
    let recent: Vec<Json> = (0..recent.len())
        .map(|i| {
            let row: Map<String, Json> = columns
                .iter()
                .map(|c| ((*c).to_string(), cell(&recent, i, c)))
                .collect();
            Json::Object(row)
        })
        .collect();
    Ok(json!({
        "now": s.services.now_ms(),
        "counts": by_state,
        "recent": recent,
    }))
}

/// The handler. A database failure is a 500 with a JSON error, never the
/// error text (it could name tables or values).
pub(crate) async fn page(State(s): State<AppState>) -> Response {
    let (status, json) = match body(&s).await {
        Ok(j) => (StatusCode::OK, j),
        Err(_) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            json!({ "error": "jobs status unavailable" }),
        ),
    };
    (
        status,
        [
            (header::CONTENT_TYPE, "application/json"),
            (header::CACHE_CONTROL, "no-store"),
        ],
        json.to_string(),
    )
        .into_response()
}

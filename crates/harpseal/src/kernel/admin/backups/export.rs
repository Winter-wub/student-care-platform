//! `GET /admin/backups/export.xlsx`: every table as one workbook
//! ([`crate::backup::xlsx`]), built in this request, audited `data.export`
//! (tables, rows, the columns left out, tables cut at Excel's limit)
//! before it is sent.

use axum::extract::State;
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use serde_json::json;

use super::stamp;
use crate::audit::{self, Actor, RequestCtx};
use crate::backup::xlsx;
use crate::kernel::admin::view::View;
use crate::{AppState, CurrentStaff};

const XLSX: &str = "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet";

pub(super) async fn xlsx(
    State(s): State<AppState>,
    staff: CurrentStaff,
    ctx: RequestCtx,
) -> Response {
    let v = View::new(&s, &staff);
    let built = async {
        let e = xlsx::export(&s.deployment).await?;
        let after = json!({
            "tables": e.tables, "rows": e.rows, "bytes": e.bytes.len(),
            "excluded_columns": e.excluded, "cut_tables": e.cut,
        });
        let actor = Actor::staff(&staff);
        audit::record(
            &s.services,
            &actor,
            "data.export",
            None,
            None,
            Some(after),
            &ctx,
        )
        .await?;
        Ok::<_, crate::Error>(e.bytes)
    };
    let bytes = match built.await {
        Ok(b) => b,
        Err(e) => return v.error(&e),
    };
    let name = format!("data-{}.xlsx", stamp(s.services.now_ms()));
    let disposition = format!("attachment; filename=\"{name}\"");
    let mut resp = (StatusCode::OK, bytes).into_response();
    let h = resp.headers_mut();
    h.insert(header::CONTENT_TYPE, HeaderValue::from_static(XLSX));
    h.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    if let Ok(d) = HeaderValue::from_str(&disposition) {
        h.insert(header::CONTENT_DISPOSITION, d);
    }
    resp
}

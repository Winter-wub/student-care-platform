//! The Excel downloads ([`crate::backup::xlsx`]), built in the request and
//! audited `data.export` (tables, rows, the columns left out, tables cut at
//! Excel's limit) before they are sent:
//!
//! - `GET /admin/backups/export.xlsx`: "Download all data", every table but
//!   the operational ones;
//! - `GET /admin/backups/export/{table}`: one table alone, any table.
//!
//! Both first count the cells and answer 413 above `[backup]
//! max_export_cells`, naming the largest table to download alone.

use axum::extract::{Path, State};
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use maud::html;
use serde_json::json;

use super::stamp;
use crate::audit::{self, Actor, RequestCtx};
use crate::backup::xlsx::{self, Selection};
use crate::kernel::admin::view::View;
use crate::kernel::text::fill;
use crate::{AppState, CurrentStaff};

const XLSX: &str = "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet";

pub(super) async fn all(
    State(s): State<AppState>,
    staff: CurrentStaff,
    ctx: RequestCtx,
) -> Response {
    let name = format!("data-{}.xlsx", stamp(s.services.now_ms()));
    send(&s, &staff, &ctx, Selection::Default, &name).await
}

pub(super) async fn one(
    State(s): State<AppState>,
    staff: CurrentStaff,
    ctx: RequestCtx,
    Path(table): Path<String>,
) -> Response {
    let name = format!("{table}-{}.xlsx", stamp(s.services.now_ms()));
    send(&s, &staff, &ctx, Selection::One(&table), &name).await
}

async fn send(
    s: &AppState,
    staff: &CurrentStaff,
    ctx: &RequestCtx,
    sel: Selection<'_>,
    file: &str,
) -> Response {
    let v = View::new(s, staff);
    let limit = s.config.backup.max_export_cells;
    let sizes = match xlsx::sizes(&s.deployment, sel).await {
        Ok(sizes) if sizes.is_empty() => return v.not_found("/admin/backups"),
        Ok(sizes) => sizes,
        Err(e) => return v.error(&e),
    };
    let cells: u64 = sizes.iter().map(|(_, _, c)| c).sum();
    if cells > limit {
        let (largest, _, most) = sizes.iter().max_by_key(|(_, _, c)| *c).expect("not empty");
        let key = if matches!(sel, Selection::One(_)) {
            "kernel.backup.export_table_too_big"
        } else {
            "kernel.backup.export_too_big"
        };
        let (cells, limit, most) = (cells.to_string(), limit.to_string(), most.to_string());
        let args = [
            ("cells", cells.as_str()),
            ("limit", limit.as_str()),
            ("table", largest.as_str()),
            ("table_cells", most.as_str()),
        ];
        let text = fill(&v.t(key), &args);
        let body =
            html! { div class="card" id="export-too-big" data-table=(largest) { p { (text) } } };
        return v.page(StatusCode::PAYLOAD_TOO_LARGE, &text, body);
    }
    let built = async {
        let e = xlsx::export(&s.deployment, sel).await?;
        let names: Vec<&str> = sizes.iter().map(|(n, _, _)| n.as_str()).collect();
        let after = json!({
            "tables": names, "rows": e.rows, "bytes": e.bytes.len(),
            "excluded_columns": e.excluded, "cut_tables": e.cut,
        });
        let actor = Actor::staff(staff);
        audit::record(
            &s.services,
            &actor,
            "data.export",
            None,
            None,
            Some(after),
            ctx,
        )
        .await?;
        Ok::<_, crate::Error>(e.bytes)
    };
    let bytes = match built.await {
        Ok(b) => b,
        Err(e) => return v.error(&e),
    };
    let disposition = format!("attachment; filename=\"{file}\"");
    let mut resp = (StatusCode::OK, bytes).into_response();
    let h = resp.headers_mut();
    h.insert(header::CONTENT_TYPE, HeaderValue::from_static(XLSX));
    h.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    if let Ok(d) = HeaderValue::from_str(&disposition) {
        h.insert(header::CONTENT_DISPOSITION, d);
    }
    resp
}

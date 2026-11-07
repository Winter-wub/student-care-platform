//! Setup for the Module upload tests (#74): a probe Module with a
//! [`ModuleRegistrar::sheet_upload`] route, confirm and cancel routes that
//! use [`KeptUpload`], signed-in browsers and a multipart body builder.

#![allow(dead_code)]

use axum::body::Body;
use axum::extract::Path;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use harpseal::{
    DeploymentConfig, KeptUpload, Module, ModuleRegistrar, Permission, RequestCtx, SheetUpload,
    Statement, Timestamp, UploadRejection, Uploads,
};
use harpseal_testing::{Browser, Harness, TEST_CONFIG, TestApp, TestResponse};
use http::{Request, header};

pub const PASSWORD: &str = "kelp forest at dawn";
pub const UPLOAD: &str = "/admin/probe/upload";
pub const NAME: &str = "probe";

pub const FEES_XLSX: &[u8] =
    include_bytes!("../../../harpseal/tests/fixtures/roster/fees_terms.xlsx");
pub const FEES_CSV: &str = "ค่าธรรมเนียม 2/2569\n\
    รหัสนักศึกษา,ชื่อ-สกุล,ยอดค้างชำระ\n\
    68010001,นกน้อย ทะเลใส,\"12,500\"\n\
    68010002,ปลาทอง อ่าวคราม,800.50\n";

/// A Module that keeps what it is sent and reports what it read.
pub struct Probe;

impl Module for Probe {
    fn name(&self) -> &'static str {
        NAME
    }

    fn register(&self, r: &mut ModuleRegistrar) {
        r.permission(Permission::new("probe.upload", "Upload probe files"))
            .permission(Permission::new("probe.all", "Every probe upload"))
            .sheet_upload("probe.upload", "/probe/upload", post(upload))
            .back_office("probe.upload", "/probe/{id}/confirm", post(confirm))
            .back_office("probe.upload", "/probe/{id}/cancel", post(cancel));
    }
}

/// `kept <id> <kind> sheets=<names> rows=<data rows per sheet> note=<field>`.
async fn upload(
    uploads: Uploads,
    ctx: RequestCtx,
    up: Result<SheetUpload, UploadRejection>,
) -> Response {
    let up = match up {
        Ok(u) => u,
        Err(r) => return r.into_response(),
    };
    let Ok(book) = up.workbook() else {
        return (StatusCode::UNPROCESSABLE_ENTITY, "unreadable").into_response();
    };
    let names: Vec<String> = book
        .sheets()
        .iter()
        .map(|s| {
            format!(
                "{}{}",
                s.name(),
                if s.is_hidden() { "(hidden)" } else { "" }
            )
        })
        .collect();
    let spec = harpseal::sheet::TableSpec::new(["รหัสนักศึกษา", "ชื่อ-สกุล", "ยอดค้างชำระ"])
        .skip_footer(["ยอดค้างชำระ"]);
    let rows: Vec<String> = book
        .sheets()
        .iter()
        .filter(|s| !s.is_hidden())
        .map(|s| s.table(&spec).map_or(0, |t| t.rows().len()).to_string())
        .collect();
    let note = up.fields.get("note").cloned().unwrap_or_default();
    let kind = up.kind.as_str();
    let id = match uploads.keep(up, &ctx).await {
        Ok(id) => id,
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    };
    let body = format!(
        "kept {id} {kind} sheets={} rows={} note={note}",
        names.join("|"),
        rows.join("|")
    );
    (StatusCode::OK, body).into_response()
}

/// Opens (hash check), then pins: `confirmed <pinned now?>`. Staff
/// holding `probe.all` see every upload.
async fn confirm(uploads: Uploads, ctx: RequestCtx, Path(id): Path<String>) -> Response {
    let uploads = uploads.shared_with("probe.all");
    match uploads.open(&id).await {
        Ok(Some(_)) => {}
        Ok(None) => return StatusCode::NOT_FOUND.into_response(),
        Err(_) => return StatusCode::CONFLICT.into_response(),
    }
    match uploads.confirm(&id, &ctx).await {
        Ok(pinned) => (StatusCode::OK, format!("confirmed {pinned}")).into_response(),
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

async fn cancel(uploads: Uploads, ctx: RequestCtx, Path(id): Path<String>) -> Response {
    match uploads.shared_with("probe.all").discard(&id, &ctx).await {
        Ok(true) => StatusCode::OK.into_response(),
        Ok(false) => StatusCode::NOT_FOUND.into_response(),
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

/// A Deployment with the probe Module; `Uploader` holds its Permission.
pub async fn boot() -> (Harness, TestApp) {
    let h = Harness::new();
    let toml = TEST_CONFIG.replace(
        "Viewer = [\"kernel.student.view\"]",
        "Viewer = [\"kernel.student.view\"]\nUploader = [\"probe.upload\"]\n\
         Clerk = [\"probe.upload\"]\nSupervisor = [\"probe.upload\", \"probe.all\"]",
    );
    let config = DeploymentConfig::from_toml(&toml).unwrap();
    let d = h.builder().config(config).module(Probe).build().unwrap();
    let app = h.boot(d).await;
    (h, app)
}

/// A signed-in browser for a new Staff member holding `role`.
pub async fn signed_in<'a>(h: &Harness, app: &'a TestApp, role: &str) -> Browser<'a> {
    let email = format!("{}@sealbay.example", role.to_lowercase());
    h.create_staff(&email, PASSWORD, &[role]).await;
    let b = app.browser();
    let r = b.login(&email, PASSWORD).await;
    assert_eq!(r.status, 303, "{}", r.body);
    assert_eq!(b.get("/admin").await.status, 200);
    b
}

/// How the CSRF token travels.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Csrf {
    Part,
    Header,
    Missing,
}

/// `POST path` with a multipart body: `_csrf` (per `csrf`), `note`, and
/// `name` / `bytes` as the `file` part.
pub async fn send(
    b: &Browser<'_>,
    path: &str,
    name: &str,
    bytes: &[u8],
    csrf: Csrf,
) -> TestResponse {
    let boundary = "----sealbay-74";
    let token = b.csrf().unwrap_or_default();
    let mut body = Vec::new();
    let mut part = |head: String, value: &[u8]| {
        body.extend_from_slice(format!("--{boundary}\r\n{head}\r\n\r\n").as_bytes());
        body.extend_from_slice(value);
        body.extend_from_slice(b"\r\n");
    };
    if csrf == Csrf::Part {
        part(
            "Content-Disposition: form-data; name=\"_csrf\"".into(),
            token.as_bytes(),
        );
    }
    part(
        "Content-Disposition: form-data; name=\"note\"".into(),
        "ภาคเรียน 2".as_bytes(),
    );
    part(
        format!(
            "Content-Disposition: form-data; name=\"file\"; filename=\"{name}\"\r\n\
             Content-Type: application/octet-stream"
        ),
        bytes,
    );
    body.extend_from_slice(format!("--{boundary}--\r\n").as_bytes());
    let mut req = Request::post(path).header(
        header::CONTENT_TYPE,
        format!("multipart/form-data; boundary={boundary}"),
    );
    if csrf == Csrf::Header {
        req = req.header("x-csrf-token", token);
    }
    b.send(req.body(Body::from(body)).unwrap()).await
}

/// Uploads to the probe route with the token as a part.
pub async fn upload_file(b: &Browser<'_>, name: &str, bytes: &[u8]) -> TestResponse {
    send(b, UPLOAD, name, bytes, Csrf::Part).await
}

/// The upload id in the probe's answer.
pub fn kept_id(r: &TestResponse) -> String {
    assert_eq!(r.status, 200, "{}", r.body);
    r.body
        .split_whitespace()
        .nth(1)
        .expect("kept <id>")
        .to_string()
}

/// The confirmed upload `id` as the import Job reads it.
pub async fn job_sees(h: &Harness, id: &str) -> bool {
    KeptUpload::open_confirmed(&h.services(), NAME, id)
        .await
        .unwrap()
        .is_some()
}

/// Audit rows with `action`.
pub async fn audited(h: &Harness, action: &str) -> i64 {
    let q = Statement::new("SELECT COUNT(*) AS n FROM audit_log WHERE action = ?").bind(action);
    let rows = h.db().query(q).await.unwrap();
    rows.get(0, "n").and_then(|v| v.as_i64()).unwrap_or(-1)
}

/// `module_upload` rows.
pub async fn uploads(h: &Harness) -> i64 {
    let rows = h
        .db()
        .query(Statement::new("SELECT COUNT(*) AS n FROM module_upload"))
        .await
        .unwrap();
    rows.get(0, "n").and_then(|v| v.as_i64()).unwrap_or(-1)
}

/// FileStore keys under `module-upload/`.
pub fn kept_keys(h: &Harness) -> Vec<String> {
    h.files()
        .keys()
        .into_iter()
        .filter(|k| k.starts_with("module-upload/"))
        .collect()
}

/// One tick at the harness clock.
pub async fn tick(h: &Harness, app: &TestApp) -> harpseal::TickReport {
    app.deployment()
        .tick(Timestamp(h.clock().now_ms()))
        .await
        .unwrap()
}

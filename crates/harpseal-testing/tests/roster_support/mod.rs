//! Setup for the roster import tests (#24): a Deployment with the example
//! `[roster]` mapping, signed-in browsers, a multipart upload, ticks that
//! run the import Jobs, and generated roster files.

#![allow(dead_code)]

use axum::body::Body;
use harpseal::{DeploymentConfig, Statement, Timestamp};
use harpseal_testing::{Browser, Harness, TestApp, TestResponse};
use http::{Request, header};

pub const PASSWORD: &str = "kelp forest at dawn";

pub const CSV_50: &[u8] = include_bytes!("../../../harpseal/tests/fixtures/roster/seal_bay_50.csv");
pub const XLSX_50: &[u8] =
    include_bytes!("../../../harpseal/tests/fixtures/roster/seal_bay_50.xlsx");

pub const HEADERS: &str = "รหัสนักศึกษา,ชื่อ-สกุล,อีเมล,ภาษา,กลุ่ม,หลักสูตร,ชั้นปี,เกรดเฉลี่ย";

/// Seal Bay University with the example Deployment's `[roster]` mapping.
pub const CONFIG: &str = r#"
name = "Seal Bay University (roster)"
default_locale = "th"
utc_offset = "+07:00"

[student]
id_pattern = '########'

[roles]
Officer = ["kernel.student.view", "kernel.student.edit", "kernel.link.manage",
           "kernel.roster.view", "kernel.roster.import"]
Viewer = ["kernel.roster.view"]

[roster]
student_code = "รหัสนักศึกษา"
display_name = "ชื่อ-สกุล"
email = "อีเมล"
locale = { column = "ภาษา", default = "th" }
tags = { column = "กลุ่ม", separator = ";" }

[roster.attributes.programme]
column = "หลักสูตร"
type = "enum"
values = ["MSC", "MSC-Intl"]
order = 1

[roster.attributes.year]
column = "ชั้นปี"
type = "integer"
order = 2

[roster.attributes.gpa]
column = "เกรดเฉลี่ย"
type = "number"
optional = true
order = 3
"#;

/// A migrated Deployment with [`CONFIG`].
pub async fn boot() -> (Harness, TestApp) {
    let h = Harness::new();
    let config = DeploymentConfig::from_toml(CONFIG).unwrap();
    let app = h.boot(h.builder().config(config).build().unwrap()).await;
    (h, app)
}

/// Runs one statement on the harness database.
pub async fn run(h: &Harness, sql: &str) {
    h.db()
        .execute(Statement::new(sql.to_string()))
        .await
        .unwrap();
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

/// `POST /admin/roster/upload` as the form sends it; `csrf` defaults to
/// the browser's token.
pub async fn upload_with(
    b: &Browser<'_>,
    name: &str,
    bytes: &[u8],
    mode: &str,
    csrf: Option<&str>,
) -> TestResponse {
    upload_ordered(b, name, bytes, mode, csrf, false).await
}

/// [`upload_with`], with the `_csrf` part after the file when
/// `csrf_last`. `Some("")` sends no `_csrf` part at all.
pub async fn upload_ordered(
    b: &Browser<'_>,
    name: &str,
    bytes: &[u8],
    mode: &str,
    csrf: Option<&str>,
    csrf_last: bool,
) -> TestResponse {
    let boundary = "----sealbay-boundary";
    let mut body = Vec::new();
    let token = b.csrf().unwrap_or_default();
    let token = csrf.unwrap_or(token.as_str()).to_string();
    let mut part = |head: String, value: &[u8]| {
        body.extend_from_slice(format!("--{boundary}\r\n{head}\r\n\r\n").as_bytes());
        body.extend_from_slice(value);
        body.extend_from_slice(b"\r\n");
    };
    let csrf_head = || "Content-Disposition: form-data; name=\"_csrf\"".to_string();
    if !csrf_last && !token.is_empty() {
        part(csrf_head(), token.as_bytes());
    }
    part(
        "Content-Disposition: form-data; name=\"mode\"".into(),
        mode.as_bytes(),
    );
    part(
        format!(
            "Content-Disposition: form-data; name=\"file\"; filename=\"{name}\"\r\n\
             Content-Type: application/octet-stream"
        ),
        bytes,
    );
    if csrf_last && !token.is_empty() {
        part(csrf_head(), token.as_bytes());
    }
    body.extend_from_slice(format!("--{boundary}--\r\n").as_bytes());
    let req = Request::post("/admin/roster/upload")
        .header(
            header::CONTENT_TYPE,
            format!("multipart/form-data; boundary={boundary}"),
        )
        .body(Body::from(body))
        .unwrap();
    b.send(req).await
}

/// Uploads with the browser's token; returns the response.
pub async fn upload(b: &Browser<'_>, name: &str, bytes: &[u8], mode: &str) -> TestResponse {
    upload_with(b, name, bytes, mode, None).await
}

/// The import id a 303 points at (`/admin/roster/<id>`).
pub fn import_id(r: &TestResponse) -> String {
    let at = r.header("location").expect("a redirect");
    at.strip_prefix("/admin/roster/")
        .expect("to an import")
        .to_string()
}

/// Uploads and checks it was previewed; returns the import id.
pub async fn previewed(b: &Browser<'_>, name: &str, bytes: &[u8], mode: &str) -> String {
    let r = upload(b, name, bytes, mode).await;
    assert_eq!(r.status, 303, "{}", r.body);
    assert_eq!(flash(b), "ok:kernel.roster.previewed");
    import_id(&r)
}

/// Confirms import `id` (after loading its page for the token), typing
/// "ยืนยัน" in case its reach asks for it (#120).
pub async fn confirm(b: &Browser<'_>, id: &str) -> TestResponse {
    confirm_typed(b, id, Some(TYPED_WORD)).await
}

/// "ยืนยัน", form-encoded.
pub const TYPED_WORD: &str = "%E0%B8%A2%E0%B8%B7%E0%B8%99%E0%B8%A2%E0%B8%B1%E0%B8%99";

/// Confirms import `id` with `typed` (form-encoded) as the typed
/// confirmation, or without the field.
pub async fn confirm_typed(b: &Browser<'_>, id: &str, typed: Option<&str>) -> TestResponse {
    b.get(&format!("/admin/roster/{id}")).await;
    let form = typed.map(|t| format!("typed={t}")).unwrap_or_default();
    b.post(&format!("/admin/roster/{id}/confirm"), &form).await
}

/// Ticks (moving the clock a minute each time) until no Job is due or
/// running; returns how many Jobs ran.
pub async fn run_jobs(h: &Harness, app: &TestApp) -> usize {
    let mut ran = 0;
    for _ in 0..50 {
        let report = app
            .deployment()
            .tick(Timestamp(h.clock().now_ms()))
            .await
            .unwrap();
        ran += report.claimed;
        h.clock().advance_minutes(1);
        if report.claimed == 0 {
            break;
        }
    }
    ran
}

/// The pending flash cookie (`ok:<key>` / `err:<key>`).
pub fn flash(b: &Browser<'_>) -> String {
    b.cookie("hs_flash").unwrap_or_default()
}

/// A text column of the import.
pub async fn import_text(h: &Harness, id: &str, col: &str) -> String {
    let rows = h
        .db()
        .query(Statement::new(format!("SELECT {col} FROM roster_import WHERE id = ?")).bind(id))
        .await
        .unwrap();
    rows.get(0, col)
        .and_then(|v| v.as_text())
        .unwrap_or_default()
        .to_string()
}

/// An integer from `sql` (bound to `params`).
pub async fn int(h: &Harness, sql: &str, params: &[&str]) -> i64 {
    let st = params
        .iter()
        .fold(Statement::new(sql.to_string()), |st, p| st.bind(*p));
    let rows = h.db().query(st).await.unwrap();
    rows.rows
        .first()
        .and_then(|r| r.first())
        .and_then(|v| v.as_i64())
        .unwrap_or(0)
}

/// The import's `counts` JSON.
pub async fn counts(h: &Harness, id: &str) -> serde_json::Value {
    serde_json::from_str(&import_text(h, id, "counts").await).unwrap()
}

/// One valid row for student code `68000000 + n`.
pub fn row(n: usize) -> String {
    let code = 68_000_000 + n;
    format!("{code},นักศึกษา {n},s{code}@student.sealbay.example,th,ปี1;MSC,MSC,1,3.0\r\n")
}

/// A CSV of rows `from..to` (see [`row`]).
pub fn csv(rows: std::ops::Range<usize>) -> Vec<u8> {
    let mut s = format!("{HEADERS}\r\n");
    for n in rows {
        s.push_str(&row(n));
    }
    s.into_bytes()
}

/// A CSV with the given data lines after the headers.
pub fn csv_lines(lines: &[&str]) -> Vec<u8> {
    let mut s = format!("{HEADERS}\r\n");
    for l in lines {
        s.push_str(l);
        s.push_str("\r\n");
    }
    s.into_bytes()
}

/// Adds Student `id` (`code`, `status`) with a live Link on the Fake
/// Channel from account `u-<id>`.
pub async fn linked_student(h: &Harness, id: &str, code: &str, status: &str) {
    run(
        h,
        &format!(
            "INSERT INTO student (id, student_code, display_name, email, locale, status, \
             created_at, updated_at) VALUES ('{id}', '{code}', 'Old {id}', NULL, 'th', \
             '{status}', 1, 1)"
        ),
    )
    .await;
    run(
        h,
        &format!(
            "INSERT INTO channel_account (id, channel, external_id, first_seen_at, flow_state, \
             updated_at) VALUES ('a-{id}', 'fake', 'u-{id}', 1, 'linked', 1)"
        ),
    )
    .await;
    run(
        h,
        &format!(
            "INSERT INTO consent (id, student_id, channel_account_id, notice_version, locale, \
             accepted_at) VALUES ('c-{id}', '{id}', 'a-{id}', '1', 'th', 1)"
        ),
    )
    .await;
    run(
        h,
        &format!(
            "INSERT INTO link (id, student_id, channel_account_id, channel, status, method, \
             linked_at, updated_at) VALUES ('l-{id}', '{id}', 'a-{id}', 'fake', 'active', \
             'email_otp', 1, 1)"
        ),
    )
    .await;
}

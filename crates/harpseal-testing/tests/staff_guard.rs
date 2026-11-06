//! The back-office guard over HTTP (#21): Permissions per route, the
//! navigation, CSRF, and the audit writer called from a Module page.

use std::collections::HashMap;

use axum::Form;
use axum::extract::State;
use axum::routing::{get, post};
use harpseal::audit::{self, Actor, Target};
use harpseal::{
    AppState, CatalogEntry, CurrentStaff, Module, ModuleRegistrar, Permission, RequestCtx,
};
use harpseal_testing::{Harness, TestApp};
use serde_json::json;

const PASSWORD: &str = "kelp forest at dawn";

/// Stands in for the announcements Module and a Staff & Roles page.
struct Pages;

const TEXTS: &[CatalogEntry] = &[
    CatalogEntry::new("pages.send", "th", "ส่งประกาศ"),
    CatalogEntry::new("pages.send", "en", "Send"),
    CatalogEntry::new("pages.students", "th", "นักศึกษา"),
    CatalogEntry::new("pages.students", "en", "Students"),
];

async fn set_roles(
    State(s): State<AppState>,
    staff: CurrentStaff,
    ctx: RequestCtx,
    Form(f): Form<HashMap<String, String>>,
) -> &'static str {
    let roles: Vec<String> = f["roles"].split(',').map(str::to_string).collect();
    harpseal::set_roles(
        &s.services,
        &Actor::staff(&staff),
        &f["staff"],
        &roles,
        &ctx,
    )
    .await
    .unwrap();
    let after = json!({ "password_hash": "$argon2id$leak", "api_token": "t", "note": "kept" });
    let target = Target::new("staff", f["staff"].clone());
    audit::record(
        &s.services,
        &Actor::staff(&staff),
        "test.redact",
        Some(&target),
        None,
        Some(after),
        &ctx,
    )
    .await
    .unwrap();
    "saved"
}

impl Module for Pages {
    fn name(&self) -> &'static str {
        "pages"
    }
    fn register(&self, r: &mut ModuleRegistrar) {
        r.permission(Permission::new("announcements.send", "Send announcements"))
            .messages(TEXTS)
            .back_office_page(
                "announcements.send",
                "/send",
                "pages.send",
                20,
                get(|| async { "send form" }),
            )
            .back_office_page(
                "kernel.student.view",
                "/students",
                "pages.students",
                10,
                get(|| async { "students" }),
            )
            .back_office("kernel.staff.manage", "/staff-roles", post(set_roles));
    }
}

async fn start() -> (Harness, TestApp) {
    let h = Harness::new();
    let app = h.boot(h.builder().module(Pages).build().unwrap()).await;
    (h, app)
}

#[tokio::test]
async fn a_viewer_gets_a_403_page_on_a_send_only_route() {
    let (h, app) = start().await;
    h.create_staff("viewer@sealbay.example", PASSWORD, &["Viewer"])
        .await;
    let b = app.browser();
    b.login("viewer@sealbay.example", PASSWORD).await;
    let r = b.get("/admin/send").await;
    assert_eq!(r.status, 403);
    assert!(r.header("content-type").unwrap().starts_with("text/html"));
    assert!(r.body.contains("ไม่มีสิทธิ์เข้าหน้านี้"));
    assert!(!r.body.contains("send form"));
    assert_eq!(b.get("/admin/students").await.body, "students");

    // Navigation and home list only what the Viewer may open.
    let home = b.get("/admin").await;
    assert!(home.body.contains("href=\"/admin/students\""));
    assert!(!home.body.contains("href=\"/admin/send\""));
}

#[tokio::test]
async fn a_database_role_grants_the_route_and_orders_the_nav() {
    let (h, app) = start().await;
    let services = h.services();
    let ctx = RequestCtx::default();
    let perms = ["announcements.send", "kernel.student.view"];
    let made = harpseal::ensure_role(&services, &Actor::System, "Sender", &perms, &ctx);
    assert!(made.await.unwrap());
    h.create_staff("sender@sealbay.example", PASSWORD, &["Sender"])
        .await;
    let b = app.browser();
    b.login("sender@sealbay.example", PASSWORD).await;
    assert_eq!(b.get("/admin/send").await.body, "send form");
    let home = b.get("/admin").await.body;
    let students = home.find("href=\"/admin/students\"").unwrap();
    let send = home.find("href=\"/admin/send\"").unwrap();
    assert!(students < send, "nav_order 10 before 20");
}

#[tokio::test]
async fn without_a_session_every_back_office_page_redirects_to_login() {
    let (_h, app) = start().await;
    for path in ["/admin", "/admin/send", "/admin/password"] {
        let r = app.get(path).await;
        assert_eq!(r.status, 303, "{path}");
        assert_eq!(r.header("location"), Some("/admin/login"));
    }
    let login = app.get("/admin/login").await;
    assert_eq!(login.status, 200);
    assert!(
        login.body.contains("integrity=\"sha384-"),
        "HTMX pinned with SRI"
    );
}

#[tokio::test]
async fn a_role_change_is_audited_with_before_and_after_and_no_secret() {
    let (h, app) = start().await;
    let admin = h
        .create_staff("admin@sealbay.example", PASSWORD, &["Administrator"])
        .await;
    let viewer = h
        .create_staff("viewer@sealbay.example", PASSWORD, &["Viewer"])
        .await;
    let b = app.browser();
    b.login("admin@sealbay.example", PASSWORD).await;
    b.get("/admin").await;
    let form = format!("staff={viewer}&roles=Viewer%2CAdministrator");
    let r = b.post("/admin/staff-roles", &form).await;
    assert_eq!(r.status, 200, "{}", r.body);

    let rows = app
        .query(
            "SELECT actor_kind, actor_id, target_kind, target_id, before, after, request_id, \
             user_agent FROM audit_log WHERE action = 'staff.roles'",
        )
        .await;
    assert_eq!(rows.len(), 1);
    let text = |c: &str| rows.get(0, c).and_then(|v| v.as_text()).map(str::to_string);
    assert_eq!(text("actor_kind").as_deref(), Some("staff"));
    assert_eq!(text("actor_id"), Some(admin));
    assert_eq!(text("target_kind").as_deref(), Some("staff"));
    assert_eq!(text("target_id"), Some(viewer));
    assert_eq!(text("before").as_deref(), Some(r#"{"roles":["Viewer"]}"#));
    assert_eq!(
        text("after").as_deref(),
        Some(r#"{"roles":["Administrator","Viewer"]}"#)
    );
    assert_eq!(text("request_id").map(|s| s.len()), Some(36));

    let all = app
        .query("SELECT coalesce(before, '') || coalesce(after, '') FROM audit_log")
        .await;
    let dump: String = all.rows.iter().filter_map(|r| r[0].as_text()).collect();
    assert!(dump.contains("[redacted]") && dump.contains("kept"));
    assert!(!dump.contains("argon2"), "{dump}");
    assert!(!dump.contains("kelp forest"), "{dump}");
    assert!(!dump.contains("\"t\""), "{dump}");
}

#[tokio::test]
async fn a_post_without_the_csrf_token_is_rejected() {
    let (h, app) = start().await;
    h.create_staff("admin@sealbay.example", PASSWORD, &["Administrator"])
        .await;
    let viewer = h
        .create_staff("viewer@sealbay.example", PASSWORD, &["Viewer"])
        .await;
    let b = app.browser();
    b.login("admin@sealbay.example", PASSWORD).await;
    b.get("/admin").await;
    let form = format!("staff={viewer}&roles=Administrator");
    assert_eq!(b.post_raw("/admin/staff-roles", &form).await.status, 403);
    let forged = format!("{form}&_csrf=00ff");
    assert_eq!(b.post_raw("/admin/staff-roles", &forged).await.status, 403);
    let header = http::Request::post("/admin/staff-roles")
        .header("content-type", "application/x-www-form-urlencoded")
        .header("x-csrf-token", b.csrf().unwrap())
        .body(axum::body::Body::from(form.clone()))
        .unwrap();
    assert_eq!(b.send(header).await.status, 200, "HTMX header form");
    assert_eq!(
        app.count("SELECT count(*) FROM audit_log WHERE action = 'staff.roles'")
            .await,
        1
    );

    // The login form has its own double-submit token.
    let fresh = app.browser();
    let r = fresh
        .post_raw(
            "/admin/login",
            "email=admin%40sealbay.example&password=kelp",
        )
        .await;
    assert_eq!(r.status, 403);
}

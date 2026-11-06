//! Staff sign-in over HTTP (#21): login, lockout, session expiry, logout
//! and the forced password change.

use harpseal::{Locale, NewStaff, Statement};
use harpseal_testing::{Browser, Harness, TestApp, TestResponse};

const EMAIL: &str = "officer@sealbay.example";
const PASSWORD: &str = "kelp forest at dawn";
const HOUR_MIN: i64 = 60;

async fn app_with_admin() -> (Harness, TestApp, String) {
    let h = Harness::new();
    let app = h.start().await;
    let id = h.create_staff(EMAIL, PASSWORD, &["Administrator"]).await;
    (h, app, id)
}

/// The text of the error flash on a page.
fn flash_text(r: &TestResponse) -> String {
    let start = r.body.find("flash err").expect("an error flash") + "flash err".len();
    let rest = &r.body[start..];
    let open = rest.find('>').unwrap() + 1;
    let close = rest.find("</div>").unwrap();
    rest[open..close].to_string()
}

async fn count(h: &Harness, sql: &'static str, bind: &str) -> i64 {
    let rows = h.db().query(Statement::new(sql).bind(bind)).await.unwrap();
    rows.rows[0][0].as_i64().unwrap()
}

#[tokio::test]
async fn login_sets_a_hardened_cookie_and_opens_the_home_page() {
    let (h, app, _) = app_with_admin().await;
    let b = app.browser();
    let r = b.login(EMAIL, PASSWORD).await;
    assert_eq!(r.status, 303);
    assert_eq!(r.header("location"), Some("/admin"));
    let cookie = r
        .headers
        .get_all("set-cookie")
        .iter()
        .map(|v| v.to_str().unwrap().to_string())
        .find(|v| v.starts_with("hs_session="))
        .expect("a session cookie");
    for attr in ["HttpOnly", "Secure", "SameSite=Lax", "Path=/admin"] {
        assert!(cookie.contains(attr), "{cookie} lacks {attr}");
    }
    // The database keeps only the token's hash.
    let token = b.cookie("hs_session").unwrap();
    assert_eq!(token.len(), 64);
    let sql = "SELECT count(*) FROM staff_session WHERE id = ?";
    assert_eq!(count(&h, sql, &token).await, 0);
    assert_eq!(app.count("SELECT count(*) FROM staff_session").await, 1);

    let home = b.get("/admin").await;
    assert_eq!(home.status, 200);
    assert!(home.body.contains(EMAIL));
    assert!(home.header("x-request-id").is_some());
    let audited = "SELECT count(*) FROM audit_log WHERE action = ?";
    assert_eq!(count(&h, audited, "staff.login").await, 1);
}

#[tokio::test]
async fn a_wrong_password_reads_exactly_like_an_unknown_email() {
    let (h, app, _) = app_with_admin().await;
    let b = app.browser();
    let wrong = b.login(EMAIL, "not the password at all").await;
    let unknown = b.login("nobody@sealbay.example", PASSWORD).await;
    assert_eq!(wrong.status, 401);
    assert_eq!(unknown.status, 401);
    assert_eq!(flash_text(&wrong), flash_text(&unknown));
    assert!(b.cookie("hs_session").is_none());
    let sql = "SELECT count(*) FROM audit_log WHERE action = ?";
    assert_eq!(count(&h, sql, "staff.login_failed").await, 2);
}

#[tokio::test]
async fn five_failures_lock_the_account_for_fifteen_minutes() {
    let (h, app, staff_id) = app_with_admin().await;
    let b = app.browser();
    for _ in 0..5 {
        assert_eq!(b.login(EMAIL, "guessing the password").await.status, 401);
    }
    let sql = "SELECT count(*) FROM security_event WHERE kind = 'staff.lockout' \
               AND staff_id = ?";
    assert_eq!(count(&h, sql, &staff_id).await, 1);

    // Locked: even the right password is refused, with the same text.
    h.clock().advance_minutes(14);
    let locked = b.login(EMAIL, PASSWORD).await;
    assert_eq!(locked.status, 401);
    let unknown = b.login("nobody@sealbay.example", PASSWORD).await;
    assert_eq!(flash_text(&locked), flash_text(&unknown));

    h.clock().advance_minutes(1);
    assert_eq!(b.login(EMAIL, PASSWORD).await.status, 303);
    assert_eq!(b.get("/admin").await.status, 200);
    assert_eq!(count(&h, sql, &staff_id).await, 1);
}

#[tokio::test]
async fn sessions_slide_for_twelve_idle_hours() {
    let (h, app, _) = app_with_admin().await;
    let b = app.browser();
    b.login(EMAIL, PASSWORD).await;
    h.clock().advance_minutes(11 * HOUR_MIN);
    assert_eq!(b.get("/admin").await.status, 200, "11 h idle is fine");
    h.clock().advance_minutes(11 * HOUR_MIN);
    assert_eq!(b.get("/admin").await.status, 200, "activity slid it");
    h.clock().advance_minutes(12 * HOUR_MIN);
    let r = b.get("/admin").await;
    assert_eq!(r.status, 303, "12 h idle ends it");
    assert_eq!(r.header("location"), Some("/admin/login"));
    assert!(b.cookie("hs_session").is_none());
    assert_eq!(app.count("SELECT count(*) FROM staff_session").await, 0);
}

#[tokio::test]
async fn sessions_end_after_seven_days_whatever_the_activity() {
    let (h, app, _) = app_with_admin().await;
    let b = app.browser();
    b.login(EMAIL, PASSWORD).await;
    for i in 1..=15 {
        h.clock().advance_minutes(11 * HOUR_MIN);
        assert_eq!(b.get("/admin").await.status, 200, "after {} h", i * 11);
    }
    h.clock().advance_minutes(3 * HOUR_MIN); // 168 h
    assert_eq!(b.get("/admin").await.status, 303);
}

#[tokio::test]
async fn logout_invalidates_the_session_row() {
    let (_h, app, _) = app_with_admin().await;
    let b = app.browser();
    b.login(EMAIL, PASSWORD).await;
    assert_eq!(b.get("/admin").await.status, 200);
    let token = b.cookie("hs_session").unwrap();
    let r = b.post("/admin/logout", "").await;
    assert_eq!(r.status, 303);
    assert_eq!(r.header("location"), Some("/admin/login"));
    assert!(b.cookie("hs_session").is_none());
    let page = b.get("/admin/login").await;
    assert!(page.body.contains("ออกจากระบบแล้ว"), "signed-out flash");

    // The old cookie, replayed, no longer opens anything.
    let replay = app.browser();
    let req = http::Request::get("/admin")
        .header("cookie", format!("hs_session={token}"))
        .body(axum::body::Body::empty())
        .unwrap();
    assert_eq!(replay.send(req).await.status, 303);
    assert_eq!(app.count("SELECT count(*) FROM staff_session").await, 0);
}

async fn forced(h: &Harness) -> String {
    let mut s = NewStaff::new(EMAIL, "Temp", "temporary pass 01", Locale::th());
    s.roles = vec!["Viewer".into()];
    s.must_change_password = true;
    h.create_staff_with(s).await
}

#[tokio::test]
async fn a_temporary_password_must_be_changed_before_anything_else() {
    let h = Harness::new();
    let app = h.start().await;
    forced(&h).await;
    let b = app.browser();
    let r = b.login(EMAIL, "temporary pass 01").await;
    assert_eq!(r.header("location"), Some("/admin/password"));
    let home = b.get("/admin").await;
    assert_eq!(home.status, 303);
    assert_eq!(home.header("location"), Some("/admin/password"));
    assert_eq!(b.get("/admin/password").await.status, 200);

    let short = b
        .post(
            "/admin/password",
            "current=temporary+pass+01&new=short&confirm=short",
        )
        .await;
    assert_eq!(short.status, 400);
    let wrong = "current=nope+nope+nope&new=a+brand+new+secret&confirm=a+brand+new+secret";
    assert_eq!(b.post("/admin/password", wrong).await.status, 400);
    let ok = "current=temporary+pass+01&new=a+brand+new+secret&confirm=a+brand+new+secret";
    let r = b.post("/admin/password", ok).await;
    assert_eq!(r.status, 303);
    assert_eq!(r.header("location"), Some("/admin"));
    let home = b.get("/admin").await;
    assert_eq!(home.status, 200);
    assert!(home.body.contains("เปลี่ยนรหัสผ่านแล้ว"), "flash shown once");
    assert!(!b.get("/admin").await.body.contains("เปลี่ยนรหัสผ่านแล้ว"));

    let fresh: Browser<'_> = app.browser();
    assert_eq!(fresh.login(EMAIL, "temporary pass 01").await.status, 401);
    assert_eq!(fresh.login(EMAIL, "a brand new secret").await.status, 303);
    let sql = "SELECT count(*) FROM audit_log WHERE action = ?";
    assert_eq!(count(&h, sql, "staff.password_change").await, 1);
}

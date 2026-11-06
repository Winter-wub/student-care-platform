//! Staff sessions and the lock under pressure (#21 review): parallel
//! guesses, counter reset, session rotation, resets, disabled accounts,
//! the tick sweep, CSRF and HTMX edges, security headers.

use harpseal::{Actor, RequestCtx, Timestamp};
use harpseal_testing::{Harness, TestApp};

const EMAIL: &str = "officer@sealbay.example";
const PASSWORD: &str = "kelp forest at dawn";
const WRONG: &str = "guessing the password";
const NEW: &str = "a brand new secret";

async fn app_with_admin() -> (Harness, TestApp, String) {
    let h = Harness::new();
    let app = h.start().await;
    let id = h.create_staff(EMAIL, PASSWORD, &["Administrator"]).await;
    (h, app, id)
}

async fn failed_logins(app: &TestApp) -> i64 {
    app.count("SELECT failed_logins FROM staff").await
}

async fn lockouts(app: &TestApp) -> i64 {
    app.count("SELECT count(*) FROM security_event WHERE kind = 'staff.lockout'")
        .await
}

fn form(password: &str) -> String {
    format!(
        "email=officer%40sealbay.example&password={}",
        harpseal_testing::urlencode(password)
    )
}

#[tokio::test]
async fn wrong_passwords_submitted_back_to_back_both_count() {
    let (_h, app, _) = app_with_admin().await;
    let (a, b) = (app.browser(), app.browser());
    a.get("/admin/login").await;
    b.get("/admin/login").await;
    let wrong = form(WRONG);
    let (ra, rb) = tokio::join!(
        a.post("/admin/login", &wrong),
        b.post("/admin/login", &wrong)
    );
    assert_eq!((ra.status.as_u16(), rb.status.as_u16()), (401, 401));
    assert_eq!(failed_logins(&app).await, 2);
}

#[tokio::test]
async fn a_successful_login_resets_the_counter() {
    let (_h, app, _) = app_with_admin().await;
    let b = app.browser();
    for _ in 0..4 {
        b.login(EMAIL, WRONG).await;
    }
    assert_eq!(b.login(EMAIL, PASSWORD).await.status, 303);
    for _ in 0..4 {
        b.login(EMAIL, WRONG).await;
    }
    assert_eq!(lockouts(&app).await, 0);
    assert_eq!(b.login(EMAIL, PASSWORD).await.status, 303, "not locked");
}

#[tokio::test]
async fn a_password_change_rotates_the_session_and_ends_all_others() {
    let (_h, app, _) = app_with_admin().await;
    let (me, other) = (app.browser(), app.browser());
    me.login(EMAIL, PASSWORD).await;
    other.login(EMAIL, PASSWORD).await;
    me.get("/admin/password").await;
    let old = me.cookie("hs_session").unwrap();
    let change = "current=kelp+forest+at+dawn&new=a+brand+new+secret&confirm=a+brand+new+secret";
    assert_eq!(me.post("/admin/password", change).await.status, 303);
    let new = me.cookie("hs_session").unwrap();
    assert_ne!(old, new, "a new session cookie");
    assert_eq!(me.get("/admin").await.status, 200);
    assert_eq!(other.get("/admin").await.status, 303, "other session ended");
    let replay = app.browser();
    let req = http::Request::get("/admin")
        .header("cookie", format!("hs_session={old}"))
        .body(axum::body::Body::empty())
        .unwrap();
    assert_eq!(replay.send(req).await.status, 303, "old cookie ended");
    assert_eq!(app.count("SELECT count(*) FROM staff_session").await, 1);
    assert_eq!(app.browser().login(EMAIL, NEW).await.status, 303);
}

#[tokio::test]
async fn wrong_current_passwords_count_toward_the_lock_and_are_audited() {
    let (_h, app, _) = app_with_admin().await;
    let b = app.browser();
    b.login(EMAIL, PASSWORD).await;
    b.get("/admin/password").await;
    let bad = "current=nope+nope+nope&new=a+brand+new+secret&confirm=a+brand+new+secret";
    for _ in 0..4 {
        assert_eq!(b.post("/admin/password", bad).await.status, 400);
    }
    assert_eq!(failed_logins(&app).await, 4);
    let r = b.post("/admin/password", bad).await;
    assert_eq!(r.status, 303);
    assert_eq!(r.header("location"), Some("/admin/login"));
    assert_eq!(lockouts(&app).await, 1);
    assert_eq!(app.count("SELECT count(*) FROM staff_session").await, 0);
    let audited = "SELECT count(*) FROM audit_log WHERE action = 'staff.password_change_failed'";
    assert_eq!(app.count(audited).await, 5);
    assert_eq!(b.login(EMAIL, PASSWORD).await.status, 401, "locked");
}

#[tokio::test]
async fn a_temporary_password_ends_sessions_forces_a_change_and_unlocks() {
    let (h, app, id) = app_with_admin().await;
    let live = app.browser();
    live.login(EMAIL, PASSWORD).await;
    let guesser = app.browser();
    for _ in 0..5 {
        guesser.login(EMAIL, WRONG).await;
    }
    assert_eq!(lockouts(&app).await, 1);
    let temp = "temporary pass 02";
    harpseal::set_temporary_password(
        &h.services(),
        &Actor::System,
        &id,
        temp,
        &RequestCtx::default(),
    )
    .await
    .unwrap();
    assert_eq!(app.count("SELECT count(*) FROM staff_session").await, 0);
    assert_eq!(live.get("/admin").await.status, 303);
    assert_eq!(
        app.count("SELECT count(*) FROM staff WHERE locked_until IS NULL")
            .await,
        1
    );
    let r = app.browser().login(EMAIL, temp).await;
    assert_eq!(
        r.header("location"),
        Some("/admin/password"),
        "unlocked, must change"
    );
}

#[tokio::test]
async fn a_disabled_account_is_refused_and_its_live_session_killed() {
    let (h, app, _) = app_with_admin().await;
    let b = app.browser();
    b.login(EMAIL, PASSWORD).await;
    assert_eq!(b.get("/admin").await.status, 200);
    let disable = harpseal::Statement::new("UPDATE staff SET status = 'disabled'");
    h.db().execute(disable).await.unwrap();
    assert_eq!(b.get("/admin").await.status, 303);
    assert_eq!(app.count("SELECT count(*) FROM staff_session").await, 0);
    assert_eq!(app.browser().login(EMAIL, PASSWORD).await.status, 401);
}

#[tokio::test]
async fn the_tick_sweeps_expired_sessions() {
    let (h, app, _) = app_with_admin().await;
    app.browser().login(EMAIL, PASSWORD).await;
    h.clock().advance_minutes(11 * 60);
    let now = Timestamp(h.clock().now_ms());
    assert_eq!(app.deployment().tick(now).await.unwrap().sessions_pruned, 0);
    h.clock().advance_minutes(60);
    let now = Timestamp(h.clock().now_ms());
    assert_eq!(app.deployment().tick(now).await.unwrap().sessions_pruned, 1);
    assert_eq!(app.count("SELECT count(*) FROM staff_session").await, 0);
}

#[tokio::test]
async fn logout_without_the_csrf_token_is_refused() {
    let (_h, app, _) = app_with_admin().await;
    let b = app.browser();
    b.login(EMAIL, PASSWORD).await;
    assert_eq!(b.post_raw("/admin/logout", "").await.status, 403);
    assert_eq!(b.get("/admin").await.status, 200, "still signed in");
}

#[tokio::test]
async fn htmx_requests_get_hx_redirect_instead_of_303() {
    let (_h, app, _) = app_with_admin().await;
    let req = http::Request::get("/admin")
        .header("hx-request", "true")
        .body(axum::body::Body::empty())
        .unwrap();
    let r = app.request(req).await;
    assert_eq!(r.status, 200);
    assert_eq!(r.header("hx-redirect"), Some("/admin/login"));
}

#[tokio::test]
async fn a_login_post_whose_field_does_not_match_the_cookie_is_refused() {
    let (_h, app, _) = app_with_admin().await;
    let b = app.browser();
    b.get("/admin/login").await;
    assert!(b.cookie("hs_csrf").is_some());
    let r = b
        .post_raw("/admin/login", &format!("{}&_csrf=00ff", form(PASSWORD)))
        .await;
    assert_eq!(r.status, 403);
    assert!(b.cookie("hs_session").is_none());
}

#[tokio::test]
async fn back_office_responses_carry_security_headers() {
    let (_h, app, _) = app_with_admin().await;
    for path in ["/admin/login", "/admin"] {
        let r = app.get(path).await;
        assert_eq!(r.header("cache-control"), Some("no-store"), "{path}");
        assert_eq!(r.header("x-frame-options"), Some("DENY"), "{path}");
        assert_eq!(r.header("referrer-policy"), Some("same-origin"), "{path}");
    }
    assert_eq!(app.get("/notice").await.header("x-frame-options"), None);
}

#[tokio::test]
async fn build_refuses_branding_set_in_code_that_from_toml_would_refuse() {
    let h = Harness::new();
    let mut c = Harness::config();
    c.branding.primary_color = "red;}body{display:none".into();
    let err = h.builder().config(c).build().unwrap_err();
    assert!(
        matches!(err, harpseal::BuildError::InvalidConfig(_)),
        "{err}"
    );
}

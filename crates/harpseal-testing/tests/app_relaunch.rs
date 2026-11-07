//! Mini App (#86): every launch asks the platform again who is there. A
//! browser still holding user A's App Session cookie, opened by user B
//! (a shared lab PC on Telegram Web, LIFF in an external browser), never
//! renders A's page: the launch drops A's session and B exchanges their
//! own launch data. Real adapters at the HTTP seam, as `app_module`: a
//! LIFF ID token answered by LINE's verify endpoint (canned), a Telegram
//! `initData` checked by HMAC. The Kernel's home and a Module's page
//! (through `AppStudent`) both go through the check.

mod app_module_support;

use app_module_support::*;
use harpseal_testing::{Browser, Harness, TestApp, TestResponse};

/// A second LINE user, linked to Nok (`s3`).
const LINE_USER_B: &str = "U5ea1ba7000000000000000000000beef";
/// A LINE user the Deployment has never seen (no account).
const LINE_USER_C: &str = "U5ea1ba7000000000000000000000cafe";
/// A second Telegram user (Pim, `s4`), derived as `web_app.rs`'s vector:
/// `query_id=…OrRd`, `user={"id":7012345679,"first_name":"Mali",
/// "language_code":"en"}`, `auth_date=1793836800`.
const INIT_DATA_B: &str = "auth_date=1793836800&query_id=AAHdF6IQAAAAAN0XohDhrOrd\
    &user=%7B%22id%22%3A7012345679%2C%22first_name%22%3A%22Mali%22%2C%22language_code%22%3A%22en%22%7D\
    &hash=c5421165bcc9c31cfd023f6be05fd3fdad2b7d468be2535259a9aa76ec9b32ee";
const TG_USER_B: &str = "7012345679";

/// [`boot`] plus Nok (`s3`) on LINE as B, Pim (`s4`) on Telegram as B,
/// (C has no account at all).
async fn world() -> (Harness, TestApp) {
    let (h, app) = boot().await;
    for (id, code, name) in [
        ("s3", "6801003", "Nok Gull"),
        ("s4", "6801004", "Pim Puffin"),
    ] {
        run(
            &h,
            &format!(
                "INSERT INTO student (id, student_code, display_name, locale, created_at, \
                 updated_at) VALUES ('{id}', '{code}', '{name}', 'en', 1, 1)"
            ),
        )
        .await;
    }
    for (acc, kind, ext) in [
        ("a-line-b", "line", LINE_USER_B),
        ("a-tg-b", "telegram", TG_USER_B),
    ] {
        run(
            &h,
            &format!(
                "INSERT INTO channel_account (id, channel, external_id, first_seen_at, \
                 flow_state, updated_at) VALUES ('{acc}', '{kind}', '{ext}', 1, 'linked', 1)"
            ),
        )
        .await;
    }
    for (link, student, acc, kind) in [
        ("l-line-b", "s3", "a-line-b", "line"),
        ("l-tg-b", "s4", "a-tg-b", "telegram"),
    ] {
        run(
            &h,
            &format!(
                "INSERT INTO link (id, student_id, channel_account_id, channel, status, method, \
                 linked_at, updated_at) VALUES ('{link}', '{student}', '{acc}', '{kind}', \
                 'active', 'email_otp', 1, 1)"
            ),
        )
        .await;
    }
    (h, app)
}

/// LINE's verify endpoint answers the next ID token as `sub`.
fn line_says(h: &Harness, sub: &str) {
    h.http().respond_with(
        200,
        format!(
            r#"{{"iss":"https://access.line.me","sub":"{sub}","aud":"2000000000","exp":1793840400,"iat":1793836800}}"#
        ),
    );
}

async fn sessions(h: &Harness, account: &str) -> i64 {
    let rows = h
        .db()
        .query(
            harpseal::Statement::new(
                "SELECT count(*) AS n FROM app_session WHERE channel_account_id = ?",
            )
            .bind(account),
        )
        .await
        .unwrap();
    rows.get(0, "n").and_then(harpseal::Value::as_i64).unwrap()
}

/// The launch shows nothing of A: the entry page, A's cookie cleared.
fn dropped(r: &TestResponse, a_name: &str) {
    assert!(is_entry(r), "{}", r.body);
    assert!(!r.body.contains(a_name), "{}", r.body);
    assert!(!r.body.contains("id=\"who\""));
    let cleared = r.headers.get_all("set-cookie").iter().any(|v| {
        let v = v.to_str().unwrap();
        v.starts_with("app_session=;") && v.contains("Max-Age=0")
    });
    assert!(cleared, "the cookie is cleared");
}

async fn exchange(b: &Browser<'_>, channel: &str, token: &str) -> TestResponse {
    let r = b.open_mini_app_raw(channel, token, PAGE).await;
    assert_eq!(r.status, 303, "{}", r.body);
    r
}

#[tokio::test]
async fn on_line_user_b_with_a_s_cookie_gets_their_own_page_never_a_s() {
    let (h, app) = world().await;
    // B opens the Mini App from their LINE: a new launch, on the Kernel's
    // home and on a Module's page alike.
    for path in ["/app", PAGE] {
        let b = signed_in(&h, &app, "line").await;
        assert_eq!(b.get(path).await.status, 200);
        let r = b.launch(path).await;
        dropped(&r, "Karn Seal");
        assert!(r.body.contains(&format!("name=\"next\" value=\"{path}\"")));
        assert_eq!(sessions(&h, "a-line").await, 0, "A's row is gone");
    }
    let b = signed_in(&h, &app, "line").await;
    dropped(&b.launch(PAGE).await, "Karn Seal");
    line_says(&h, LINE_USER_B);
    exchange(&b, "line", ID_TOKEN).await;
    let r = b.get(PAGE).await;
    assert!(who(&r).starts_with("s3|"), "{}", r.body);
    assert!(!r.body.contains("Karn Seal"));
    assert_eq!(sessions(&h, "a-line-b").await, 1);
}

#[tokio::test]
async fn on_telegram_user_b_with_a_s_cookie_gets_their_own_page_never_a_s() {
    let (h, app) = world().await;
    let b = signed_in(&h, &app, "telegram").await;
    assert!(who(&b.get(PAGE).await).starts_with("s2|"));
    let r = b.launch(PAGE).await;
    dropped(&r, "Mali Tern");
    assert_eq!(sessions(&h, "a-telegram").await, 0);
    let r = b.open_mini_app_raw("telegram", INIT_DATA_B, PAGE).await;
    assert_eq!(r.status, 303, "{}", r.body);
    let r = b.get(PAGE).await;
    assert!(who(&r).starts_with("s4|"), "{}", r.body);
    assert!(!r.body.contains("Mali Tern"));
}

#[tokio::test]
async fn an_exchange_ends_the_session_the_browser_still_held() {
    let (h, app) = world().await;
    // Two tabs: B's exchange arrives while A's cookie is still sent.
    let b = signed_in(&h, &app, "telegram").await;
    exchange(&b, "telegram", INIT_DATA_B).await;
    assert_eq!(sessions(&h, "a-telegram").await, 0, "A's row is gone");
    assert_eq!(sessions(&h, "a-tg-b").await, 1);
    assert!(who(&b.get(PAGE).await).starts_with("s4|"));
    // C is unknown: the "not linked" page, and B's session ends too.
    line_says(&h, LINE_USER_C);
    let r = b.open_mini_app_raw("line", ID_TOKEN, PAGE).await;
    assert_eq!(r.status, 200);
    assert!(r.body.contains("ยังไม่ได้เชื่อมกับรหัสนักศึกษา"), "{}", r.body);
    assert_eq!(sessions(&h, "a-tg-b").await, 0);
    assert!(b.cookie("app_session").is_none());
    assert!(is_entry(&b.get(PAGE).await));
}

#[tokio::test]
async fn the_same_user_keeps_navigating_and_relaunches_into_their_own_page() {
    let (h, app) = world().await;
    let b = signed_in(&h, &app, "line").await;
    let token = b.cookie("app_session").unwrap();
    // Links, forms and HTMX inside the launch: no exchange, same session.
    for path in [PAGE, "/app", PAGE] {
        assert_eq!(b.get(path).await.status, 200, "{path}");
    }
    assert_eq!(b.cookie("app_session").as_deref(), Some(token.as_str()));
    // Each such page ends the entry page's loop guard, so the next launch
    // exchanges at once.
    assert!(b.get(PAGE).await.body.contains("removeItem('hs_app_x')"));
    assert_eq!(h.http().requests().len(), 1, "one verify call: the sign-in");
    // A relaunch by A: entry page, then A again (one more verify call).
    dropped(&b.launch(PAGE).await, "Karn Seal");
    line_says(&h, LINE_USER);
    exchange(&b, "line", ID_TOKEN).await;
    assert!(who(&b.get(PAGE).await).starts_with("s1|"));
    assert_eq!(sessions(&h, "a-line").await, 1);
}

#[tokio::test]
async fn without_fetch_metadata_only_a_referer_from_this_host_continues() {
    let (h, app) = world().await;
    let get = |referer: Option<&str>| {
        let mut req = http::Request::get(PAGE).header("host", "sealbay.example");
        if let Some(r) = referer {
            req = req.header("referer", r);
        }
        req.body(axum::body::Body::empty()).unwrap()
    };
    let b = signed_in(&h, &app, "line").await;
    // An older webview following our own link: the Referer names us.
    let r = b.send_as_is(get(Some("https://sealbay.example/app"))).await;
    assert!(who(&r).starts_with("s1|"), "{}", r.body);
    // Opened from elsewhere, or with no Referer at all: a new launch.
    for referer in [
        Some("https://liff.line.me/2000000000-SealBay0/app/probe"),
        None,
    ] {
        let b = signed_in(&h, &app, "line").await;
        dropped(&b.send_as_is(get(referer)).await, "Karn Seal");
    }
    // Nothing waits for launch data that never comes: once dropped, an
    // in-app navigation has no session either.
    assert!(is_entry(&b.launch(PAGE).await));
    assert!(is_entry(&b.get(PAGE).await));
    assert_eq!(sessions(&h, "a-line").await, 0);
}

#[tokio::test]
async fn sign_out_drops_the_session_only_with_its_csrf_token() {
    let (h, app) = world().await;
    let b = signed_in(&h, &app, "line").await;
    let home = b.get("/app").await;
    assert!(
        home.body.contains("action=\"/app/sign-out\""),
        "{}",
        home.body
    );
    assert!(home.body.contains("ออกจากระบบ"));
    // No token, or a cross-site post: 403, the session stays.
    assert_eq!(b.post_raw("/app/sign-out", "_csrf=00").await.status, 403);
    let cross = http::Request::post("/app/sign-out")
        .header("content-type", "application/x-www-form-urlencoded")
        .header("sec-fetch-site", "cross-site")
        .body(axum::body::Body::from(format!(
            "_csrf={}",
            b.csrf().unwrap()
        )))
        .unwrap();
    assert_eq!(b.send(cross).await.status, 403);
    assert_eq!(sessions(&h, "a-line").await, 1);
    let r = b.post("/app/sign-out", "").await;
    assert_eq!(r.status, 200, "{}", r.body);
    assert!(r.body.contains("ออกจากระบบแล้ว"), "{}", r.body);
    assert!(!r.body.contains("Karn Seal"));
    assert_eq!(sessions(&h, "a-line").await, 0);
    assert!(b.cookie("app_session").is_none());
    assert!(is_entry(&b.get(PAGE).await));
    // Again, without a session: the same page.
    assert_eq!(b.post("/app/sign-out", "").await.status, 200);
}

//! `POST`s to a Module's own Mini App routes (#72): CSRF is checked by
//! the extractors, never left to the handler. `AppStudent` alone wants the
//! `X-CSRF-Token` header (HTMX); `AppForm` also takes the `_csrf` field
//! (a plain form). Cross-site requests are refused even with the token:
//! another bot's Mini App inside web.telegram.org shares the partitioned
//! cookie. Setup: `app_module_support`.

mod app_module_support;

use axum::body::Body;
use harpseal_testing::{Browser, TestResponse};

use app_module_support::{FORM, NOTE, PAGE, boot, is_entry, run, signed_in};

fn post(path: &str, headers: &[(&str, &str)], form: &str) -> http::Request<Body> {
    let mut req =
        http::Request::post(path).header("content-type", "application/x-www-form-urlencoded");
    for (k, v) in headers {
        req = req.header(*k, *v);
    }
    req.body(Body::from(form.to_string())).unwrap()
}

/// The session's token, from the probe page's `_csrf` input.
async fn token(b: &Browser<'_>) -> String {
    b.get(PAGE).await;
    b.csrf().expect("the probe page carries the token")
}

fn refused(r: &TestResponse) -> bool {
    // The Kernel's "not valid, open it again from the chat" page.
    let text = r.body.contains("คำขอนี้ใช้ไม่ได้") || r.body.contains("This request is not valid");
    r.status == 403 && text && !r.body.contains("saved:") && !r.body.contains("form:")
}

#[tokio::test]
async fn unsafe_requests_need_the_session_token_on_both_platforms() {
    let (h, app) = boot().await;
    for (channel, id) in [("line", "s1"), ("telegram", "s2")] {
        let b = signed_in(&h, &app, channel).await;
        let t = token(&b).await;
        // `AppStudent` alone: no token, the field only, a wrong header.
        assert!(refused(&b.send(post(NOTE, &[], "")).await), "{channel}");
        let field = format!("_csrf={t}");
        assert!(refused(&b.send(post(NOTE, &[], &field)).await), "{channel}");
        let wrong = [("x-csrf-token", "00ff")];
        assert!(refused(&b.send(post(NOTE, &wrong, "")).await), "{channel}");
        let right = [("x-csrf-token", t.as_str())];
        let r = b.send(post(NOTE, &right, "")).await;
        assert_eq!(
            (r.status.as_u16(), r.body.clone()),
            (200, format!("saved:{id}"))
        );
        // `AppForm`: the field or the header, never neither.
        assert!(
            refused(&b.send(post(FORM, &[], "text=hi")).await),
            "{channel}"
        );
        let r = b.send(post(FORM, &[], &format!("text=hi&_csrf={t}"))).await;
        assert_eq!(r.body, format!("form:{id}:hi"));
        let r = b.send(post(FORM, &right, "text=yo")).await;
        assert_eq!(r.body, format!("form:{id}:yo"));
        assert_eq!(r.header("cache-control"), Some("no-store"));
    }
}

#[tokio::test]
async fn another_students_token_does_not_pass() {
    let (h, app) = boot().await;
    let karn = signed_in(&h, &app, "line").await;
    let mali = signed_in(&h, &app, "telegram").await;
    let mali_token = token(&mali).await;
    let r = karn
        .send(post(FORM, &[], &format!("text=hi&_csrf={mali_token}")))
        .await;
    assert!(refused(&r));
}

#[tokio::test]
async fn cross_site_posts_are_refused_even_with_the_token() {
    let (h, app) = boot().await;
    let b = signed_in(&h, &app, "telegram").await;
    let t = token(&b).await;
    let cross = [
        ("x-csrf-token", t.as_str()),
        ("sec-fetch-site", "cross-site"),
    ];
    assert!(refused(&b.send(post(NOTE, &cross, "")).await));
    let r = b
        .send(post(
            FORM,
            &[("sec-fetch-site", "cross-site")],
            &format!("text=hi&_csrf={t}"),
        ))
        .await;
    assert!(refused(&r));
    // Same origin (the Mini App itself, also inside Telegram Web's iframe).
    let same = [
        ("x-csrf-token", t.as_str()),
        ("sec-fetch-site", "same-origin"),
    ];
    assert_eq!(b.send(post(NOTE, &same, "")).await.body, "saved:s2");
    // A cross-site GET is a navigation: allowed (a link from the chat).
    let get = http::Request::get(PAGE)
        .header("sec-fetch-site", "cross-site")
        .body(Body::empty())
        .unwrap();
    assert_eq!(b.send(get).await.status, 200);
}

#[tokio::test]
async fn without_a_session_a_post_gets_the_entry_page_and_htmx_a_redirect_to_its_page() {
    let (h, app) = boot().await;
    let r = app.request(post(FORM, &[], "text=hi&_csrf=00")).await;
    assert!(is_entry(&r), "{}", r.body);
    // HTMX would swap the entry page into the fragment: a full reload, and
    // HTMX follows `HX-Redirect` with a GET, so a POST goes back to the
    // page it came from (never to its own POST-only path: 405, blank).
    let from = ("hx-current-url", "https://sealbay.example/app/probe?x=1");
    let hx = [("hx-request", "true"), from];
    let r = app.request(post(NOTE, &hx, "")).await;
    assert_eq!(r.status, 200);
    assert_eq!(r.header("hx-redirect"), Some(PAGE));
    assert!(!r.body.contains("xchg"));
    assert!(is_entry(&app.get(PAGE).await), "the reload signs in again");
    // No HX-Current-URL, or one off `/app`: the home.
    for current in [None, Some("https://evil.example//evil.example/x")] {
        let mut hx = vec![("hx-request", "true")];
        hx.extend(current.map(|c| ("hx-current-url", c)));
        let r = app.request(post(NOTE, &hx, "")).await;
        assert_eq!(r.header("hx-redirect"), Some("/app"), "{current:?}");
    }
    // A GET reloads itself.
    let get = http::Request::get(PAGE)
        .header("hx-request", "true")
        .body(Body::empty())
        .unwrap();
    assert_eq!(app.request(get).await.header("hx-redirect"), Some(PAGE));
    // A revoked Link: the same for HTMX; the reload shows "not linked".
    let b = signed_in(&h, &app, "line").await;
    let t = token(&b).await;
    run(&h, "UPDATE link SET status = 'revoked' WHERE id = 'l-line'").await;
    let r = b
        .send(post(
            NOTE,
            &[("hx-request", "true"), ("x-csrf-token", &t), from],
            "",
        ))
        .await;
    assert_eq!(r.header("hx-redirect"), Some(PAGE));
    let page = b.get(PAGE).await;
    assert_eq!(page.status, 200);
    assert!(page.body.contains("ยังไม่ได้เชื่อมกับรหัสนักศึกษา"));
}

#[tokio::test]
async fn a_form_over_64_kib_is_413() {
    let (h, app) = boot().await;
    let b = signed_in(&h, &app, "line").await;
    let t = token(&b).await;
    let big = format!("_csrf={t}&text={}", "a".repeat(64 * 1024));
    assert_eq!(b.send(post(FORM, &[], &big)).await.status, 413);
}

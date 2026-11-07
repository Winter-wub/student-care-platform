//! Mini App (#86): every launch exchanges, so the exchange is bounded per
//! IP group by an attempt ceiling counted before LINE is called
//! (`[mini_app] verify_per_minute`), and each account keeps at most five
//! App Sessions. Real LINE adapter at the HTTP seam (canned answers; with
//! none queued the answer is `200 {}`, no valid claim: a refusal).

mod app_module_support;

use std::future::Future;
use std::pin::Pin;
use std::task::Poll;

use app_module_support::*;
use harpseal::{DeploymentConfig, Statement, Value};
use harpseal_testing::{Harness, TEST_CONFIG, TestApp, TestResponse};

/// Karn (`s1`) linked on LINE, with `[mini_app] verify_per_minute = 40`.
async fn boot_with_ceiling() -> (Harness, TestApp) {
    let h = Harness::new();
    let toml = format!("{TEST_CONFIG}\n[mini_app]\nverify_per_minute = 40\n");
    let cfg = DeploymentConfig::from_toml(&toml).unwrap();
    let app = h.boot(builder(&h).config(cfg).build().unwrap()).await;
    for sql in [
        "INSERT INTO student (id, student_code, display_name, locale, created_at, updated_at) \
         VALUES ('s1', '6801001', 'Karn Seal', 'th', 1, 1)",
        &format!(
            "INSERT INTO channel_account (id, channel, external_id, first_seen_at, flow_state, \
             updated_at) VALUES ('a-line', 'line', '{LINE_USER}', 1, 'linked', 1)"
        ),
        "INSERT INTO link (id, student_id, channel_account_id, channel, status, method, \
         linked_at, updated_at) VALUES ('l-line', 's1', 'a-line', 'line', 'active', \
         'email_otp', 1, 1)",
    ] {
        run(&h, sql).await;
    }
    (h, app)
}

fn exchange(ip: &str, token: &str) -> http::Request<axum::body::Body> {
    http::Request::post("/app/session")
        .header("content-type", "application/x-www-form-urlencoded")
        .header("cf-connecting-ip", ip)
        .body(axum::body::Body::from(format!(
            "channel=line&token={token}&next=%2Fapp"
        )))
        .unwrap()
}

fn valid_claim(h: &Harness) {
    h.http().respond_with(
        200,
        format!(
            r#"{{"iss":"https://access.line.me","sub":"{LINE_USER}","aud":"2000000000","exp":1793840400,"iat":1793836800}}"#
        ),
    );
}

/// Polls every request together, as a burst sent at once.
async fn all_at_once(app: &TestApp, reqs: Vec<http::Request<axum::body::Body>>) -> Vec<u16> {
    let mut futs: Vec<Pin<Box<dyn Future<Output = TestResponse> + '_>>> = reqs
        .into_iter()
        .map(|r| Box::pin(app.request(r)) as Pin<Box<dyn Future<Output = TestResponse> + '_>>)
        .collect();
    let mut out: Vec<Option<u16>> = vec![None; futs.len()];
    std::future::poll_fn(|cx| {
        for (f, o) in futs.iter_mut().zip(out.iter_mut()) {
            if o.is_none()
                && let Poll::Ready(r) = f.as_mut().poll(cx)
            {
                *o = Some(r.status.as_u16());
            }
        }
        if out.iter().all(Option::is_some) {
            Poll::Ready(out.iter().map(|o| o.unwrap_or_default()).collect())
        } else {
            Poll::Pending
        }
    })
    .await
}

#[tokio::test]
async fn a_forged_burst_sent_at_once_reaches_line_at_most_the_ceiling() {
    let (h, app) = boot_with_ceiling().await;
    let reqs = (0..100)
        .map(|i| exchange("203.0.113.7", &format!("forged-{i}")))
        .collect();
    let statuses = all_at_once(&app, reqs).await;
    let calls = h.http_requests().len();
    assert!((1..=40).contains(&calls), "{calls} verify calls");
    assert!(
        statuses.iter().all(|s| *s == 403 || *s == 429),
        "{statuses:?}"
    );
    assert_eq!(statuses.iter().filter(|s| **s == 403).count(), calls);
    // Another group still verifies.
    assert_eq!(app.request(exchange("198.51.100.2", "x")).await.status, 403);
}

#[tokio::test]
async fn accepted_tokens_count_toward_the_ceiling_too() {
    let (h, app) = boot_with_ceiling().await;
    for i in 0..40 {
        valid_claim(&h);
        let r = app.request(exchange("203.0.113.8", ID_TOKEN)).await;
        assert_eq!(r.status, 303, "launch {i}");
    }
    let r = app.request(exchange("203.0.113.8", ID_TOKEN)).await;
    assert_eq!(r.status, 429);
    assert_eq!(h.http_requests().len(), 40, "LINE is not called");
    h.clock().advance_ms(60_000);
    valid_claim(&h);
    assert_eq!(
        app.request(exchange("203.0.113.8", ID_TOKEN)).await.status,
        303
    );
}

#[tokio::test]
async fn a_replayed_valid_token_keeps_at_most_five_sessions() {
    let (h, app) = boot().await;
    let mut last = String::new();
    for _ in 0..20 {
        valid_claim(&h);
        // No cookie: each replay is a fresh exchange.
        let r = app.request(exchange("203.0.113.9", ID_TOKEN)).await;
        assert_eq!(r.status, 303);
        last = r.header("set-cookie").unwrap().to_string();
    }
    let rows = h
        .db()
        .query(Statement::new(
            "SELECT count(*) AS n FROM app_session WHERE channel_account_id = 'a-line'",
        ))
        .await
        .unwrap();
    assert_eq!(rows.get(0, "n").and_then(Value::as_i64), Some(5));
    // The newest one works.
    let token = last.split(';').next().unwrap().to_string();
    let req = http::Request::get(PAGE)
        .header("cookie", token)
        .header("sec-fetch-site", "same-origin")
        .body(axum::body::Body::empty())
        .unwrap();
    assert!(who(&app.request(req).await).starts_with("s1|"));
}

#[tokio::test]
async fn the_ceiling_and_idle_minutes_are_checked_at_load() {
    for bad in [
        "idle_minutes = 1",
        "idle_minutes = 61",
        "verify_per_minute = 29",
        "verify_per_minute = 100001",
    ] {
        let toml = format!("{TEST_CONFIG}\n[mini_app]\n{bad}\n");
        assert!(DeploymentConfig::from_toml(&toml).is_err(), "{bad}");
    }
    let ok = format!("{TEST_CONFIG}\n[mini_app]\nidle_minutes = 2\nverify_per_minute = 30\n");
    let c = DeploymentConfig::from_toml(&ok).unwrap();
    assert_eq!(
        (c.mini_app.idle_minutes, c.mini_app.verify_per_minute),
        (2, 30)
    );
    assert_eq!(
        DeploymentConfig::from_toml(TEST_CONFIG)
            .unwrap()
            .mini_app
            .verify_per_minute,
        300
    );
}

//! Tests of dev mode: query decoding, the startup rules, the email bound
//! and the secret check at the router.

use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use harpseal::{Email, EmailSender};
use harpseal_testing::Harness;
use tower::ServiceExt;

use super::*;

#[test]
fn query_params_are_decoded() {
    let q = "to=s68010001%40sealbay.example&x=a+b";
    assert_eq!(query_param(q, "to").unwrap(), "s68010001@sealbay.example");
    assert_eq!(query_param(q, "x").unwrap(), "a b");
    assert_eq!(query_param(q, "y"), None);
    assert_eq!(query_param("to=%E0%B8%81", "to").unwrap(), "ก");
    assert_eq!(query_param("to=50%", "to").unwrap(), "50%");
}

#[test]
fn the_secret_must_match_exactly() {
    assert!(same("abc", "abc"));
    assert!(!same("abc", "abd"));
    assert!(!same("abc", "ab"));
    assert!(!same("", "abc"));
}

const GOOD: &str = "0123456789abcdef0123";

#[test]
fn dev_mode_needs_loopback_and_a_strong_secret() {
    for addr in [
        "127.0.0.1:8080",
        "127.0.0.5:1",
        "[::1]:8080",
        "localhost:8080",
    ] {
        assert_eq!(check_safe(addr, GOOD), Ok(()), "{addr}");
    }
    for addr in [
        "0.0.0.0:8080",
        "192.168.1.10:8080",
        "[::]:8080",
        "example.org:80",
    ] {
        let e = check_safe(addr, GOOD).unwrap_err();
        assert!(e.contains("loopback"), "{addr}: {e}");
    }
    for secret in ["change-me", "short", "0123456789abcde"] {
        let e = check_safe("127.0.0.1:8080", secret).unwrap_err();
        assert!(e.contains("at least 16"), "{secret}: {e}");
    }
}

#[tokio::test]
async fn only_the_last_50_emails_are_kept() {
    let c = CapturingEmail::default();
    for n in 0..60 {
        let e = Email {
            to: format!("s{n}@sealbay.example"),
            subject: "รหัส".into(),
            text: "ข้อความ".into(),
            html: None,
        };
        c.send(e).await.unwrap();
    }
    assert_eq!(c.0.lock().unwrap().len(), KEPT_EMAILS);
    assert!(c.last_to("s9@sealbay.example").is_none(), "dropped");
    assert!(c.last_to("s59@sealbay.example").is_some());
}

async fn status(router: &axum::Router, secret: Option<&str>) -> StatusCode {
    let mut req = Request::post("/dev/tick");
    if let Some(s) = secret {
        req = req.header(SECRET_HEADER, s);
    }
    let resp = router
        .clone()
        .oneshot(req.body(Body::empty()).unwrap())
        .await
        .unwrap();
    resp.status()
}

#[tokio::test]
async fn the_dev_routes_answer_404_without_the_right_secret() {
    let h = Harness::new();
    let secrets = example_deployment::Secrets::from_lookup(|k| match k {
        "LINE_LIFF_ID" => Some("1234567890-AbCdEfGh".into()),
        "TELEGRAM_WEB_APP_URL" => Some("https://app.seal-bay.invalid/tg".into()),
        "FAKE_SECRET" => Some(GOOD.into()),
        _ => None,
    });
    let (d, fake) = example_deployment::deployment_with_fake(h.services(), secrets).unwrap();
    d.migrate().await.unwrap();
    let r = router(Arc::new(d), fake, CapturingEmail::default(), GOOD.into());
    assert_eq!(status(&r, None).await, StatusCode::NOT_FOUND);
    assert_eq!(
        status(&r, Some("wrong-secret-0123456789")).await,
        StatusCode::NOT_FOUND
    );
    assert_eq!(status(&r, Some(GOOD)).await, StatusCode::OK);
}

//! Branding's limits on recorded LINE HTTP (#114): a change that would
//! take the channel past 1000 rich menus is refused before anything is
//! created, in Thai on the page; the locked Menu's re-lock passes over an
//! account that keeps failing after 5 runs, so the others are done.

mod common;

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use async_trait::async_trait;
use axum::body::Body;
use common::branding::with_locked;
use common::relink::*;
use common::{LineApi, png};
use harpseal::branding::{MenuImagesSaved, set_menu_images};
use harpseal::{
    Actor, HttpClient, MenuImageProblem, MenuSlot, RequestCtx, Statement, Timestamp, Value,
};
use harpseal_testing::Harness;
use serde_json::json;

fn image(tag: u8) -> Vec<u8> {
    let mut v = png(2500, 843);
    v.extend([tag; 16]);
    v
}

async fn count(h: &Harness, sql: &'static str) -> i64 {
    let rows = h.db().query(Statement::new(sql)).await.unwrap();
    rows.get(0, "n").and_then(Value::as_i64).unwrap_or(-1)
}

fn creates(api: &LineApi) -> usize {
    api.requests()
        .iter()
        .filter(|r| {
            r.method == http::Method::POST && r.uri == "https://api.line.me/v2/bot/richmenu"
        })
        .count()
}

#[tokio::test]
async fn a_change_past_1000_rich_menus_is_refused_before_any_create_in_thai() {
    let s = setup(2).await;
    // Revision 0 is there already: 999 more fill the channel.
    for i in 0..999 {
        s.api.preexisting(
            &format!("richmenu-hand-{i:04}"),
            json!({ "name": format!("made-by-hand-{i}") }),
        );
    }
    let before = creates(&s.api);
    let d = s.app.deployment();
    let r = set_menu_images(d, None, Some(image(1)), &s.editor, &RequestCtx::default()).await;
    let problem = MenuImageProblem::TooManyMenus {
        held: 1000,
        adding: 2,
        limit: 1000,
    };
    assert_eq!(
        r.unwrap(),
        MenuImagesSaved::Refused(vec![(MenuSlot::Unlocked, problem)])
    );
    assert_eq!(creates(&s.api), before, "nothing created");
    for sql in [
        "SELECT count(*) AS n FROM setting WHERE key LIKE 'kernel.branding.%'",
        "SELECT count(*) AS n FROM audit_log WHERE action LIKE 'branding.%'",
        "SELECT count(*) AS n FROM job WHERE kind = 'kernel.menu.relink'",
    ] {
        assert_eq!(count(&s.h, sql).await, 0, "{sql}");
    }
    assert!(s.h.files().keys().is_empty());

    // The page says what to do, in Thai.
    let b = s.app.browser();
    b.login("editor@sealbay.example", "kelp forest at dawn")
        .await;
    b.get("/admin/branding").await;
    let boundary = "----seal-full";
    let mut body = Vec::new();
    for (name, value, file) in [
        ("_csrf", b.csrf().unwrap().into_bytes(), ""),
        ("op", b"apply".to_vec(), ""),
        ("unlocked", image(2), "; filename=\"menu.png\""),
    ] {
        let head = format!(
            "--{boundary}\r\nContent-Disposition: form-data; name=\"{name}\"{file}\r\n\r\n"
        );
        body.extend(head.into_bytes());
        body.extend(value);
        body.extend(b"\r\n");
    }
    body.extend(format!("--{boundary}--\r\n").into_bytes());
    let req = http::Request::post("/admin/branding/menu")
        .header(
            "content-type",
            format!("multipart/form-data; boundary={boundary}"),
        )
        .body(Body::from(body))
        .unwrap();
    let r = b.send(req).await;
    assert_eq!(r.status, 422);
    assert!(
        r.body.contains("LINE มีริชเมนูอยู่แล้ว 1000 รายการ") && r.body.contains("menu_generation"),
        "{}",
        r.body
    );
}

/// The recorded LINE API, but once armed, every link of `failing`'s rich
/// menu answers 500 (and is counted).
struct OneFails {
    api: LineApi,
    armed: AtomicBool,
    failing: String,
    tries: AtomicUsize,
}

#[async_trait]
impl HttpClient for OneFails {
    async fn send(&self, req: http::Request<Vec<u8>>) -> harpseal::Result<http::Response<Vec<u8>>> {
        let link = format!("https://api.line.me/v2/bot/user/{}/richmenu/", self.failing);
        if self.armed.load(Ordering::SeqCst)
            && req.method() == http::Method::POST
            && req.uri().to_string().starts_with(&link)
        {
            self.tries.fetch_add(1, Ordering::SeqCst);
            let body = br#"{"message":"An error occurred in the backend"}"#.to_vec();
            return Ok(http::Response::builder().status(500).body(body).unwrap());
        }
        self.api.send(req).await
    }
}

#[tokio::test]
async fn the_relock_passes_over_an_account_failing_5_runs_and_finishes_the_rest() {
    let api = LineApi::default();
    let http = Arc::new(OneFails {
        api: api.clone(),
        armed: AtomicBool::new(false),
        failing: user(1),
        tries: AtomicUsize::new(0),
    });
    let (h, d) = with_locked(Harness::new(), http.clone(), 0, 4).await;
    let before = api.linked(&user(1)).unwrap();
    let rows = h
        .db()
        .query(Statement::new("SELECT id FROM staff"))
        .await
        .unwrap();
    let staff = Actor::Staff(rows.get(0, "id").and_then(Value::as_text).unwrap().into());
    let r = set_menu_images(&d, Some(image(3)), None, &staff, &RequestCtx::default()).await;
    assert_eq!(r.unwrap(), MenuImagesSaved::Saved);
    http.armed.store(true, Ordering::SeqCst);
    for _ in 0..12 {
        h.clock().advance_minutes(61);
        d.tick(Timestamp(h.clock().now_ms())).await.unwrap();
    }
    let relocked = api.linked(&user(0)).unwrap();
    assert_ne!(relocked, before);
    for i in [2, 3] {
        assert_eq!(api.linked(&user(i)), Some(relocked.clone()), "account {i}");
    }
    assert_eq!(
        api.linked(&user(1)),
        Some(before),
        "passed over, never moved"
    );
    assert_eq!(
        http.tries.load(Ordering::SeqCst),
        6,
        "5 failed runs, then the 6th passes over"
    );
    let queued = count(
        &h,
        "SELECT count(*) AS n FROM job WHERE kind = 'kernel.branding.relock' \
         AND state IN ('queued', 'running')",
    )
    .await;
    assert_eq!(queued, 0, "the Job finished");
}

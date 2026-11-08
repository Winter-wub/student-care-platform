//! The locked Menu's re-lock after a new locked image (#114): an account
//! that links while its page is being re-locked keeps its unlocked Menu.

mod common;

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use async_trait::async_trait;
use common::branding::with_locked;
use common::relink::user;
use common::{LineApi, png};
use harpseal::branding::{MenuImagesSaved, set_menu_images};
use harpseal::{Actor, Db, HttpClient, RequestCtx, Statement, Timestamp, Value};
use harpseal_testing::Harness;

/// The recorded LINE API; once armed, the first link of `trigger`'s rich
/// menu marks account `links` linked, as a Student finishing their OTP
/// in the middle of the re-lock page would.
struct LinksMidPage {
    api: LineApi,
    db: Arc<dyn Db>,
    armed: AtomicBool,
    trigger: String,
    links: String,
}

#[async_trait]
impl HttpClient for LinksMidPage {
    async fn send(&self, req: http::Request<Vec<u8>>) -> harpseal::Result<http::Response<Vec<u8>>> {
        let hit = req.method() == http::Method::POST
            && req.uri().to_string().starts_with(&format!(
                "https://api.line.me/v2/bot/user/{}/richmenu/",
                self.trigger
            ));
        let resp = self.api.send(req).await;
        if hit && self.armed.swap(false, Ordering::SeqCst) {
            let st = Statement::new(
                "UPDATE channel_account SET flow_state = 'linked' WHERE external_id = ?",
            )
            .bind(self.links.as_str());
            self.db.execute(st).await?;
        }
        resp
    }
}

#[tokio::test]
async fn an_account_linked_during_the_relock_page_is_not_relocked() {
    let h = Harness::new();
    let api = LineApi::default();
    let http = Arc::new(LinksMidPage {
        api: api.clone(),
        db: h.db(),
        armed: AtomicBool::new(false),
        trigger: user(3),
        links: user(4),
    });
    let (h, d) = with_locked(h, http.clone(), 0, 8).await;
    let before = api.linked(&user(4)).unwrap();
    let rows = h
        .db()
        .query(Statement::new("SELECT id FROM staff"))
        .await
        .unwrap();
    let staff = Actor::Staff(rows.get(0, "id").and_then(Value::as_text).unwrap().into());
    let mut new = png(2500, 843);
    new.extend([9; 16]);
    let r = set_menu_images(&d, Some(new), None, &staff, &RequestCtx::default()).await;
    assert_eq!(r.unwrap(), MenuImagesSaved::Saved);
    http.armed.store(true, Ordering::SeqCst);
    for _ in 0..5 {
        h.clock().advance_minutes(61);
        d.tick(Timestamp(h.clock().now_ms())).await.unwrap();
    }
    let relocked = api.linked(&user(3)).unwrap();
    assert_ne!(relocked, before);
    assert_eq!(api.linked(&user(5)), Some(relocked), "the rest of the page");
    assert_eq!(
        api.linked(&user(4)),
        Some(before),
        "linked meanwhile: left alone"
    );
}

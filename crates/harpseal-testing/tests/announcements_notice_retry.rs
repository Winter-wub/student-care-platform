//! The edit notice's retries (#77 review): a retried chunk meters a Push
//! the platform already accepted once (ADR-0010), and an older run never
//! lowers a newer run's mark.

mod announcements_support;

use std::collections::HashSet;
use std::sync::{Arc, Mutex};

use announcements_support::notice::*;
use announcements_support::*;
use async_trait::async_trait;
use harpseal::{
    BatchReceipt, Channel, ChannelAccountId, ChannelKind, Delivery, Inbound, IncomingFile, Locale,
    Menu, Message, Metering, MiniApp, Multicast, Result, ResumeToken, SendReceipt, Statement,
    Timestamp, WebhookRequest,
};
use harpseal_channel_fake::FakeChannel;

/// The LineLike Fake behind LINE's retry key: a keyed batch sent again
/// with the same key is answered 409 (`replayed_units`) and not delivered.
#[derive(Clone)]
struct RetryKeyed {
    inner: FakeChannel,
    keys: Arc<Mutex<HashSet<String>>>,
}

#[async_trait]
impl Channel for RetryKeyed {
    fn kind(&self) -> ChannelKind {
        self.inner.kind()
    }
    async fn parse_webhook(&self, req: WebhookRequest) -> Result<Vec<Inbound>> {
        self.inner.parse_webhook(req).await
    }
    async fn download(&self, f: &IncomingFile) -> Result<Vec<u8>> {
        self.inner.download(f).await
    }
    async fn send(
        &self,
        to: &ChannelAccountId,
        d: &Delivery,
        m: &Message,
        l: &Locale,
    ) -> Result<SendReceipt> {
        self.inner.send(to, d, m, l).await
    }
    async fn send_batch_resumed(
        &self,
        to: &ChannelAccountId,
        d: &Delivery,
        msgs: &[Message],
        l: &Locale,
        key: &str,
        resume: Option<&ResumeToken>,
    ) -> BatchReceipt {
        if !self.keys.lock().unwrap().insert(key.to_string()) {
            let replay = |_: &Message| {
                SendReceipt::new(None, Metering::Metered, true)
                    .with_metered_units(1)
                    .with_replayed_units(1)
            };
            return BatchReceipt::complete(msgs.iter().map(replay).collect());
        }
        self.inner
            .send_batch_resumed(to, d, msgs, l, key, resume)
            .await
    }
    async fn set_menu(&self, to: &ChannelAccountId, menu: Option<&Menu>) -> Result<()> {
        self.inner.set_menu(to, menu).await
    }
    fn mini_app(&self) -> Option<&dyn MiniApp> {
        self.inner.mini_app()
    }
    fn multicast(&self) -> Option<&dyn Multicast> {
        self.inner.multicast()
    }
}

/// Ticks until no Job is claimed (a failed Job waits for its retry).
async fn settle(w: &World) {
    for _ in 0..40 {
        let now = Timestamp(w.h.clock().now_ms());
        if w.app.deployment().tick(now).await.unwrap().claimed == 0 {
            return;
        }
    }
    panic!("jobs never settled");
}

#[tokio::test]
async fn a_retried_chunk_meters_an_accepted_push_once() {
    let w = boot_line(CONFIG, 500, |inner| RetryKeyed {
        inner,
        keys: Arc::default(),
    })
    .await;
    roster(&w).await;
    let b = signed_in(&w, "Officer").await;
    let id = sent(&w, &b).await;
    let before = metered(&w, "fake").await;
    edit(&w, &b, &id, "ใหม่", true).await;
    // The first Push goes out (and is recorded), then the mark is lost.
    run(
        &w.h,
        "CREATE TRIGGER lost_mark BEFORE UPDATE OF notice_version ON announcement_delivery \
         BEGIN SELECT RAISE(ABORT, 'disk I/O error'); END",
    )
    .await;
    settle(&w).await;
    assert_eq!(
        texts(
            &w,
            "SELECT state || ' ' || last_error FROM job WHERE kind = 'announcements.notice.chunk'"
        )
        .await,
        ["queued database: disk I/O error"],
        "the chunk failed and waits for its retry"
    );
    assert_eq!(metered(&w, "fake").await - before, 1);
    run(&w.h, "DROP TRIGGER lost_mark").await;
    // The Job's retry sends the same key again: LINE answers 409.
    w.h.clock().advance_minutes(60);
    settle(&w).await;
    assert_eq!((notices(&w, "s1").len(), notices(&w, "s3").len()), (1, 1));
    assert_eq!(metered(&w, "fake").await - before, 2, "not 3");
    assert_eq!(
        texts(&w, "SELECT notice_report ->> 'sent' FROM announcement").await,
        ["2"]
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_older_run_never_lowers_a_newer_mark() {
    let w = boot().await;
    roster(&w).await;
    let b = signed_in(&w, "Officer").await;
    let id = sent(&w, &b).await;
    edit(&w, &b, &id, "แก้ครั้งที่หนึ่ง", true).await;
    // While v1's first Push is on its way, a newer run (v1000) takes the
    // report and marks every row.
    let db = w.h.db().clone();
    let newer = format!(
        "UPDATE announcement SET notice_report = json_object('version', 1000, 'at', 0, \
         'sent', 0, 'failed', 0, 'total', NULL) WHERE id = '{id}'; \
         UPDATE announcement_delivery SET notice_version = 1000 WHERE announcement_id = '{id}'"
    );
    w.line.before_next_send(move || {
        tokio::task::block_in_place(|| {
            let rt = tokio::runtime::Handle::current();
            for sql in newer.split("; ") {
                rt.block_on(db.execute(Statement::new(sql.to_string())))
                    .unwrap();
            }
        });
    });
    drain(&w).await;
    assert_eq!(
        texts(
            &w,
            "SELECT DISTINCT coalesce(notice_version, -1) FROM announcement_delivery \
             WHERE status = 'sent' AND channel = 'fake'"
        )
        .await,
        ["1000"]
    );
}

//! Exact metering of resumed sends at the Deployment seam (#62): a
//! scripted Channel returns the receipts a LINE-like platform would (409s
//! as `replayed_units`, a resume token), and `usage_daily.metered` must
//! count every Push request exactly once in the three cases of the ticket,
//! whether the earlier attempt's answer or its write was lost.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use harpseal::{
    BatchReceipt, Card, CardList, Channel, ChannelAccountId, ChannelKind, Delivery, Error, Inbound,
    IncomingFile, Locale, Menu, Message, Metering, Result, ResumeToken, SendOptions, SendReceipt,
    Statement, WebhookRequest,
};
use harpseal_testing::{Harness, TestApp};

const KIND: ChannelKind = ChannelKind("scripted");

/// What the Kernel handed the Channel on one call.
#[derive(Debug, Clone, PartialEq)]
struct Got {
    messages: usize,
    key: String,
    resume: Option<ResumeToken>,
}

/// Returns the queued receipts, one per batch, and records each call.
#[derive(Clone, Default)]
struct Scripted {
    next: Arc<Mutex<VecDeque<BatchReceipt>>>,
    got: Arc<Mutex<Vec<Got>>>,
}

impl Scripted {
    fn then(&self, b: BatchReceipt) {
        self.next.lock().unwrap().push_back(b);
    }
    fn got(&self) -> Vec<Got> {
        self.got.lock().unwrap().clone()
    }
}

#[async_trait]
impl Channel for Scripted {
    fn kind(&self) -> ChannelKind {
        KIND
    }
    async fn parse_webhook(&self, _: WebhookRequest) -> Result<Vec<Inbound>> {
        Ok(Vec::new())
    }
    async fn download(&self, _: &IncomingFile) -> Result<Vec<u8>> {
        Err(Error::Platform("no files".into()))
    }
    async fn send(
        &self,
        _: &ChannelAccountId,
        _: &Delivery,
        _: &Message,
        _: &Locale,
    ) -> Result<SendReceipt> {
        Err(Error::Platform("batches only".into()))
    }
    async fn send_batch_resumed(
        &self,
        _: &ChannelAccountId,
        _: &Delivery,
        msgs: &[Message],
        _: &Locale,
        key: &str,
        resume: Option<&ResumeToken>,
    ) -> BatchReceipt {
        self.got.lock().unwrap().push(Got {
            messages: msgs.len(),
            key: key.into(),
            resume: resume.cloned(),
        });
        self.next
            .lock()
            .unwrap()
            .pop_front()
            .expect("a scripted batch")
    }
    async fn set_menu(&self, _: &ChannelAccountId, _: Option<&Menu>) -> Result<()> {
        Ok(())
    }
}

/// A Push receipt opening `units` requests, `replayed` of them answered 409.
fn pushed(units: u32, replayed: u32) -> SendReceipt {
    SendReceipt::new(None, Metering::Free, true)
        .with_metered_units(units)
        .with_replayed_units(replayed)
}

fn stopped() -> Error {
    Error::RateLimited {
        retry_after_secs: 30,
    }
}

async fn boot() -> (Harness, TestApp, Scripted) {
    let h = Harness::new();
    let ch = Scripted::default();
    let app = h
        .boot(h.builder().channel(ch.clone()).build().unwrap())
        .await;
    (h, app, ch)
}

fn to() -> ChannelAccountId {
    ChannelAccountId::new(KIND, "u-sealbay-1")
}

fn news(n: usize) -> Message {
    Message::CardList(CardList(
        (0..n)
            .map(|i| Card {
                title: format!("ข่าวคณะ {i}"),
                body: "มหาวิทยาลัยอ่าวแมวน้ำ".into(),
                buttons: Vec::new(),
            })
            .collect(),
    ))
}

const METERED: &str =
    "SELECT coalesce(sum(metered), 0) FROM usage_daily WHERE channel = 'scripted'";
const ROWS: &str = "SELECT msg_index, units, error IS NULL AS done FROM outbound_message \
     ORDER BY msg_index";

/// `(msg_index, units, complete)` of every row.
async fn rows(app: &TestApp) -> Vec<(i64, i64, bool)> {
    let r = app.query(ROWS).await;
    (0..r.len())
        .map(|i| {
            let int = |c| r.get(i, c).unwrap().as_i64().unwrap();
            (int("msg_index"), int("units"), int("done") == 1)
        })
        .collect()
}

/// Makes the next write of an `outbound_message` row fail (and so the
/// whole `Db::batch` recording a send) until [`heal`].
async fn break_writes(h: &Harness) {
    h.db()
        .execute(Statement::new(
            "CREATE TRIGGER lost_write BEFORE INSERT ON outbound_message \
             BEGIN SELECT RAISE(ABORT, 'disk I/O error'); END",
        ))
        .await
        .unwrap();
}

async fn heal(h: &Harness) {
    h.db()
        .execute(Statement::new("DROP TRIGGER lost_write"))
        .await
        .unwrap();
}

#[tokio::test]
async fn case_1_a_whole_replay_of_a_recorded_message_meters_2_not_4() {
    // The first attempt sent and recorded the Message (2 Push requests);
    // the caller never saw the answer and sends the batch again: LINE
    // answers 409 to both requests.
    let (_h, app, ch) = boot().await;
    let d = app.deployment();
    let msgs = [news(61)];
    let opts = SendOptions::new().key("job-1/u1");
    ch.then(BatchReceipt::complete(vec![pushed(2, 0)]));
    d.send_batch_with(&to(), None, &msgs, &Locale::th(), &opts)
        .await
        .unwrap();
    ch.then(BatchReceipt::complete(vec![pushed(2, 2)]));
    d.send_batch_with(&to(), None, &msgs, &Locale::th(), &opts)
        .await
        .unwrap();
    assert_eq!(app.count(METERED).await, 2);
    assert_eq!(rows(&app).await, [(0, 2, true)], "one row, not two");
}

#[tokio::test]
async fn case_2_a_resume_after_an_unrecorded_partial_send_meters_2_not_1() {
    // Push 1 of the Message went out, push 2 failed, and the write
    // recording push 1 failed too. The resume replays push 1 (409) and
    // sends push 2: both are on no record, both count once.
    let (h, app, ch) = boot().await;
    let d = app.deployment();
    let msgs = [news(61)];
    let opts = SendOptions::new().key("job-2/u1");
    break_writes(&h).await;
    ch.then(BatchReceipt::part_way(Vec::new(), pushed(1, 0), stopped()));
    let err = d
        .send_batch_with(&to(), None, &msgs, &Locale::th(), &opts)
        .await
        .unwrap_err();
    assert!(matches!(err, Error::Db(_)), "{err}");
    assert_eq!(app.count(METERED).await, 0, "the write was rolled back");
    heal(&h).await;
    ch.then(BatchReceipt::complete(vec![pushed(2, 1)]));
    d.send_batch_with(&to(), None, &msgs, &Locale::th(), &opts)
        .await
        .unwrap();
    assert_eq!(app.count(METERED).await, 2);
    assert_eq!(rows(&app).await, [(0, 2, true)]);
}

#[tokio::test]
async fn case_2_recorded_the_partial_send_meters_1_then_1_more() {
    let (_h, app, ch) = boot().await;
    let d = app.deployment();
    let msgs = [news(61)];
    let opts = SendOptions::new().key("job-3/u1");
    ch.then(BatchReceipt::part_way(Vec::new(), pushed(1, 0), stopped()));
    let err = d
        .send_batch_with(&to(), None, &msgs, &Locale::th(), &opts)
        .await
        .unwrap_err();
    assert!(
        matches!(
            err,
            Error::PartialSend {
                sent: 0,
                total: 1,
                ..
            }
        ),
        "{err}"
    );
    assert_eq!(app.count(METERED).await, 1);
    assert_eq!(rows(&app).await, [(0, 1, false)], "partly sent");
    ch.then(BatchReceipt::complete(vec![pushed(2, 1)]));
    d.send_batch_with(&to(), None, &msgs, &Locale::th(), &opts.resume_after(&err))
        .await
        .unwrap();
    assert_eq!(app.count(METERED).await, 2);
    assert_eq!(rows(&app).await, [(0, 2, true)], "completed, not doubled");
}

#[tokio::test]
async fn case_3_a_resume_of_a_shared_request_hands_back_the_token_and_meters_2() {
    // [X, A]: push 1 = X + the first parts of A (charged to X), push 2 =
    // the rest of A, failed. The resume covers A only, with the token.
    let (_h, app, ch) = boot().await;
    let d = app.deployment();
    let msgs = [Message::text("ประกาศ: สอบกลางภาค"), news(61)];
    let opts = SendOptions::new().key("job-4/u1");
    let token = ResumeToken::new("scripted:push-2");
    ch.then(
        BatchReceipt::part_way(vec![pushed(1, 0)], pushed(0, 0), stopped())
            .with_resume(Some(token.clone())),
    );
    let err = d
        .send_batch_with(&to(), None, &msgs, &Locale::th(), &opts)
        .await
        .unwrap_err();
    assert!(
        matches!(
            err,
            Error::PartialSend {
                sent: 1,
                total: 2,
                ..
            }
        ),
        "{err}"
    );
    assert_eq!(err.resume_token(), Some(&token));
    assert_eq!(rows(&app).await, [(0, 1, true), (1, 0, false)]);
    // Push 2 went out after all (its answer was lost): 409 now.
    ch.then(BatchReceipt::complete(vec![pushed(1, 1)]));
    let again = d
        .send_batch_with(&to(), None, &msgs, &Locale::th(), &opts.resume_after(&err))
        .await
        .unwrap();
    assert_eq!(again.len(), 1, "the receipts of Messages 1..");
    let got = ch.got();
    assert_eq!(got[1].messages, 1, "Messages from the stopped one on");
    assert_eq!(got[1].resume, Some(token));
    assert_eq!(got[1].key, "job-4/u1");
    assert_eq!(app.count(METERED).await, 2, "push 1 and push 2, once each");
    assert_eq!(rows(&app).await, [(0, 1, true), (1, 1, true)]);
}

#[tokio::test]
async fn a_lost_answer_inside_a_recorded_partial_send_counts_once_meters_3() {
    // Push 1 recorded, push 2 accepted but its answer lost (recorded as a
    // failure), push 3 new: 409, 409, 200 on the resume.
    let (_h, app, ch) = boot().await;
    let d = app.deployment();
    let msgs = [news(150)];
    let opts = SendOptions::new().key("job-5/u1");
    ch.then(BatchReceipt::part_way(Vec::new(), pushed(1, 0), stopped()));
    let err = d
        .send_batch_with(&to(), None, &msgs, &Locale::th(), &opts)
        .await
        .unwrap_err();
    ch.then(BatchReceipt::complete(vec![pushed(3, 2)]));
    d.send_batch_with(&to(), None, &msgs, &Locale::th(), &opts.resume_after(&err))
        .await
        .unwrap();
    assert_eq!(app.count(METERED).await, 3);
    assert_eq!(rows(&app).await, [(0, 3, true)]);
}

#[tokio::test]
async fn a_failed_write_after_a_partial_send_then_a_retry_meters_each_request_once_3() {
    // [X, A]: X (1 unit) and part of A went out, the write failed. The
    // retry of the whole batch replays push 1 (409, X), and A's push 2
    // and 3 go out now: 3 units, each once.
    let (h, app, ch) = boot().await;
    let d = app.deployment();
    let msgs = [Message::text("ประกาศ: ปิดหอสมุด"), news(150)];
    let opts = SendOptions::new().key("job-6/u1");
    break_writes(&h).await;
    ch.then(BatchReceipt::part_way(
        vec![pushed(1, 0)],
        pushed(0, 0),
        stopped(),
    ));
    assert!(matches!(
        d.send_batch_with(&to(), None, &msgs, &Locale::th(), &opts)
            .await,
        Err(Error::Db(_))
    ));
    heal(&h).await;
    ch.then(BatchReceipt::complete(vec![pushed(1, 1), pushed(2, 0)]));
    d.send_batch_with(&to(), None, &msgs, &Locale::th(), &opts)
        .await
        .unwrap();
    assert_eq!(app.count(METERED).await, 3);
    assert_eq!(rows(&app).await, [(0, 1, true), (1, 2, true)]);
}

#[tokio::test]
async fn a_resume_past_the_batch_is_invalid_and_sends_nothing() {
    let (_h, app, ch) = boot().await;
    let opts = SendOptions::new().key("job-7/u1").from_message(3);
    let got = app
        .deployment()
        .send_batch_with(&to(), None, &[news(1)], &Locale::th(), &opts)
        .await;
    assert!(matches!(got, Err(Error::Invalid(_))), "{got:?}");
    assert!(ch.got().is_empty());
}

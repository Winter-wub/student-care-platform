//! Resuming a partial send on LINE through the Deployment (#62): the
//! request that failed is rebuilt from the resume token with the same
//! objects and `X-Line-Retry-Key`, so every part reaches the Student once,
//! and `usage_daily.metered` counts every Push request exactly once in the
//! three cases of the ticket.

mod common;

use std::sync::Arc;

use common::{SECRET, TOKEN, USER, json};
use harpseal::{Card, CardList, ChannelAccountId, Error, Locale, Message, SendOptions, Statement};
use harpseal_channel_line::{KIND, LineChannel, LineConfig, RETRY_KEY_HEADER};
use harpseal_testing::{Harness, RecordedRequest, TestApp};
use serde_json::Value;

async fn boot() -> (Harness, TestApp) {
    let h = Harness::new();
    let line = LineChannel::new(
        LineConfig::new(SECRET, TOKEN, ""),
        Arc::new(h.http().clone()),
    );
    let app = h.boot(h.builder().channel(line).build().unwrap()).await;
    (h, app)
}

fn to() -> ChannelAccountId {
    ChannelAccountId::new(KIND, USER)
}

/// 61 Cards: 6 carousels (A(6)).
fn cards61() -> Message {
    Message::CardList(CardList(
        (0..61)
            .map(|i| Card {
                title: format!("ประกาศ {i}"),
                body: "มหาวิทยาลัยอ่าวแมวน้ำ".into(),
                buttons: Vec::new(),
            })
            .collect(),
    ))
}

const OK: &str = "{}";
const BOOM: &str = r#"{"message":"boom"}"#;
const ACCEPTED: &str = r#"{"message":"The retry key is already accepted"}"#;
const METERED: &str = "SELECT coalesce(sum(metered), 0) FROM usage_daily WHERE channel = 'line'";
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

/// `(retry key, objects)` of each Push.
fn pushes(reqs: &[RecordedRequest]) -> Vec<(String, Vec<Value>)> {
    reqs.iter()
        .map(|r| {
            let key = r.headers[RETRY_KEY_HEADER].to_str().unwrap().to_string();
            let objects = json(&r.body)["messages"].as_array().unwrap().clone();
            (key, objects)
        })
        .collect()
}

#[tokio::test]
async fn case_3_a_packed_request_resumed_delivers_every_part_once_and_meters_2() {
    // [X(1), A(6)]: push 1 = X + A1..A4 (charged to X), push 2 = A5 A6
    // (charged to A) fails.
    let (h, app) = boot().await;
    let d = app.deployment();
    let msgs = [Message::text("ประกาศ: งดเรียนวันศุกร์"), cards61()];
    let opts = SendOptions::new().key("job-1/a1");
    h.http().respond_with(200, OK);
    h.http().respond_with(500, BOOM);
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
    assert!(err.resume_token().is_some(), "LINE says where to resume");
    let first = pushes(&h.http().take_requests());
    assert_eq!(first.len(), 2);
    assert_eq!((first[0].1.len(), first[1].1.len()), (5, 2));
    assert_eq!(app.count(METERED).await, 1);
    assert_eq!(rows(&app).await, [(0, 1, true), (1, 0, false)]);
    // The resume rebuilds push 2 only: the same objects, the same key.
    h.http().respond_with(200, OK);
    let again = d
        .send_batch_with(&to(), None, &msgs, &Locale::th(), &opts.resume_after(&err))
        .await
        .unwrap();
    assert_eq!(again.len(), 1);
    let resumed = pushes(&h.http().take_requests());
    assert_eq!(resumed, [first[1].clone()], "push 2 again, nothing else");
    // Accepted requests: push 1 and the resumed push 2: X and A1..A6 once.
    let delivered: Vec<Value> = [&first[0], &resumed[0]]
        .iter()
        .flat_map(|(_, o)| o.clone())
        .collect();
    assert_eq!(delivered.len(), 7);
    for (i, a) in delivered.iter().enumerate() {
        assert!(!delivered[i + 1..].contains(a), "part {i} delivered twice");
    }
    assert_eq!(app.count(METERED).await, 2);
    assert_eq!(rows(&app).await, [(0, 1, true), (1, 1, true)]);
}

#[tokio::test]
async fn case_3_the_failed_push_went_out_after_all_409_meters_2() {
    // Push 2 reached LINE but its answer was lost: the resumed push 2
    // carries its key, LINE answers 409 and delivers nothing twice; the
    // unit was never recorded, so it counts once now.
    let (h, app) = boot().await;
    let d = app.deployment();
    let msgs = [Message::text("ประกาศ: งดเรียนวันศุกร์"), cards61()];
    let opts = SendOptions::new().key("job-2/a1");
    h.http().respond_with(200, OK);
    h.http().respond_with(500, BOOM);
    let err = d
        .send_batch_with(&to(), None, &msgs, &Locale::th(), &opts)
        .await
        .unwrap_err();
    let first = pushes(&h.http().take_requests());
    h.http().respond_with(409, ACCEPTED);
    let again = d
        .send_batch_with(&to(), None, &msgs, &Locale::th(), &opts.resume_after(&err))
        .await
        .unwrap();
    assert_eq!((again[0].metered_units, again[0].replayed_units), (1, 1));
    assert_eq!(pushes(&h.http().take_requests()), [first[1].clone()]);
    assert_eq!(app.count(METERED).await, 2);
    assert_eq!(rows(&app).await, [(0, 1, true), (1, 1, true)]);
}

#[tokio::test]
async fn case_1_a_recorded_message_replayed_whole_meters_2_not_4() {
    // The batch went out and was recorded; the caller lost the answer and
    // sends it again with the same key: 409 on both pushes.
    let (h, app) = boot().await;
    let d = app.deployment();
    let msgs = [cards61()];
    for _ in 0..2 {
        h.http().respond_with(200, OK);
    }
    d.send_batch_with_key(&to(), None, &msgs, &Locale::th(), "job-3/a1")
        .await
        .unwrap();
    for _ in 0..2 {
        h.http().respond_with(409, ACCEPTED);
    }
    let again = d
        .send_batch_with_key(&to(), None, &msgs, &Locale::th(), "job-3/a1")
        .await
        .unwrap();
    assert_eq!((again[0].metered_units, again[0].replayed_units), (2, 2));
    assert_eq!(app.count(METERED).await, 2);
    assert_eq!(rows(&app).await, [(0, 2, true)]);
}

#[tokio::test]
async fn case_2_the_write_of_a_partial_send_failed_the_retry_meters_2_not_1() {
    // Push 1 went out, push 2 failed, and recording push 1 failed too. The
    // retry with the same key replays push 1 (409) and sends push 2: both
    // count, once.
    let (h, app) = boot().await;
    let d = app.deployment();
    let msgs = [cards61()];
    let opts = SendOptions::new().key("job-4/a1");
    h.db()
        .execute(Statement::new(
            "CREATE TRIGGER lost_write BEFORE INSERT ON outbound_message \
             BEGIN SELECT RAISE(ABORT, 'disk I/O error'); END",
        ))
        .await
        .unwrap();
    h.http().respond_with(200, OK);
    h.http().respond_with(500, BOOM);
    let err = d
        .send_batch_with(&to(), None, &msgs, &Locale::th(), &opts)
        .await
        .unwrap_err();
    assert!(matches!(err, Error::Db(_)), "{err}");
    assert_eq!(app.count(METERED).await, 0);
    h.db()
        .execute(Statement::new("DROP TRIGGER lost_write"))
        .await
        .unwrap();
    let first = pushes(&h.http().take_requests());
    h.http().respond_with(409, ACCEPTED);
    h.http().respond_with(200, OK);
    d.send_batch_with(&to(), None, &msgs, &Locale::th(), &opts.resume_after(&err))
        .await
        .unwrap();
    let retry = pushes(&h.http().take_requests());
    assert_eq!(retry, first, "the same two requests, the same keys");
    assert_eq!(app.count(METERED).await, 2);
    assert_eq!(rows(&app).await, [(0, 2, true)]);
}

#[tokio::test]
async fn ten_identical_texts_resumed_at_push_2_reuse_its_key_not_push_1s() {
    // Push 1 and push 2 carry the same 5 objects; only the occurrence
    // number tells their keys apart. A resume at push 2 must not reuse
    // push 1's key, or LINE would answer 409 and drop the 5 texts.
    let (h, app) = boot().await;
    let d = app.deployment();
    let msgs = vec![
        Message::text("ประกาศ: หยุดเรียนวันศุกร์");
        10
    ];
    let opts = SendOptions::new().key("job-5/a1");
    h.http().respond_with(200, OK);
    h.http().respond_with(500, BOOM);
    let err = d
        .send_batch_with(&to(), None, &msgs, &Locale::th(), &opts)
        .await
        .unwrap_err();
    assert!(matches!(err, Error::PartialSend { sent: 5, .. }), "{err}");
    let first = pushes(&h.http().take_requests());
    assert_ne!(first[0].0, first[1].0);
    h.http().respond_with(200, OK);
    d.send_batch_with(&to(), None, &msgs, &Locale::th(), &opts.resume_after(&err))
        .await
        .unwrap();
    assert_eq!(pushes(&h.http().take_requests()), [first[1].clone()]);
    assert_eq!(app.count(METERED).await, 2);
}

/// 121 Cards: 11 carousels, so Push requests of 5, 5 and 1.
fn cards121() -> Message {
    Message::CardList(CardList(
        (0..121)
            .map(|i| Card {
                title: format!("ข่าวคณะ {i}"),
                body: "มหาวิทยาลัยอ่าวแมวน้ำ".into(),
                buttons: Vec::new(),
            })
            .collect(),
    ))
}

#[tokio::test]
async fn one_message_resumed_whose_failed_push_went_out_meters_2() {
    // A(6): push 1 = A1..A5 (1 unit, recorded), push 2 = A6 accepted but
    // its answer lost. The token resume rebuilds push 2 only: 409.
    let (h, app) = boot().await;
    let d = app.deployment();
    let msgs = [cards61()];
    let opts = SendOptions::new().key("job-6/a1");
    h.http().respond_with(200, OK);
    h.http().respond_with(500, BOOM);
    let err = d
        .send_batch_with(&to(), None, &msgs, &Locale::th(), &opts)
        .await
        .unwrap_err();
    let first = pushes(&h.http().take_requests());
    assert_eq!(rows(&app).await, [(0, 1, false)]);
    h.http().respond_with(409, ACCEPTED);
    let again = d
        .send_batch_with(&to(), None, &msgs, &Locale::th(), &opts.resume_after(&err))
        .await
        .unwrap();
    assert_eq!((again[0].metered_units, again[0].replayed_units), (2, 2));
    assert_eq!(pushes(&h.http().take_requests()), [first[1].clone()]);
    assert_eq!(app.count(METERED).await, 2, "LINE charged 2");
    assert_eq!(rows(&app).await, [(0, 2, true)]);
}

#[tokio::test]
async fn three_pushes_push_2_lost_then_resumed_meters_3() {
    let (h, app) = boot().await;
    let d = app.deployment();
    let msgs = [cards121()];
    let opts = SendOptions::new().key("job-7/a1");
    h.http().respond_with(200, OK);
    h.http().respond_with(500, BOOM);
    let err = d
        .send_batch_with(&to(), None, &msgs, &Locale::th(), &opts)
        .await
        .unwrap_err();
    let first = pushes(&h.http().take_requests());
    assert_eq!((first[0].1.len(), first[1].1.len()), (5, 5));
    h.http().respond_with(409, ACCEPTED);
    h.http().respond_with(200, OK);
    d.send_batch_with(&to(), None, &msgs, &Locale::th(), &opts.resume_after(&err))
        .await
        .unwrap();
    let resumed = pushes(&h.http().take_requests());
    assert_eq!(resumed.len(), 2);
    assert_eq!(resumed[0], first[1], "push 2 rebuilt, same key");
    assert_eq!(resumed[1].1.len(), 1);
    assert_eq!(app.count(METERED).await, 3);
    assert_eq!(rows(&app).await, [(0, 3, true)]);
}

#[tokio::test]
async fn a_resume_that_fails_again_carries_its_units_on_meters_3() {
    // One Message over push 1, 2 and 3. Push 2 fails; the resume sends push
    // 2, and push 3 reaches LINE but its answer is lost (500). The next
    // resume rebuilds push 3 only and LINE answers 409: the token must carry
    // both earlier units (push 1 from the first token, push 2 now) so the
    // 409 counts once against the 2 on record. LINE charged 3.
    let (h, app) = boot().await;
    let d = app.deployment();
    let msgs = [cards121()];
    let opts = SendOptions::new().key("job-8/a1");
    h.http().respond_with(200, OK);
    h.http().respond_with(500, BOOM);
    let err = d
        .send_batch_with(&to(), None, &msgs, &Locale::th(), &opts)
        .await
        .unwrap_err();
    h.http().take_requests();
    h.http().respond_with(200, OK);
    h.http().respond_with(500, BOOM);
    let opts = opts.resume_after(&err);
    let err = d
        .send_batch_with(&to(), None, &msgs, &Locale::th(), &opts)
        .await
        .unwrap_err();
    assert!(matches!(err, Error::PartialSend { sent: 0, .. }), "{err}");
    let second = pushes(&h.http().take_requests());
    assert_eq!(app.count(METERED).await, 2);
    assert_eq!(rows(&app).await, [(0, 2, false)]);
    h.http().respond_with(409, ACCEPTED);
    let done = d
        .send_batch_with(&to(), None, &msgs, &Locale::th(), &opts.resume_after(&err))
        .await
        .unwrap();
    assert_eq!((done[0].metered_units, done[0].replayed_units), (3, 3));
    assert_eq!(pushes(&h.http().take_requests()), [second[1].clone()]);
    assert_eq!(app.count(METERED).await, 3);
    assert_eq!(rows(&app).await, [(0, 3, true)]);
}

#[tokio::test]
async fn an_unkeyed_send_resumed_reuses_its_retry_keys() {
    let (h, app) = boot().await;
    let d = app.deployment();
    let msgs = [Message::text("ประกาศ: งดเรียนวันศุกร์"), cards61()];
    h.http().respond_with(200, OK);
    h.http().respond_with(500, BOOM);
    let err = d
        .send_batch(&to(), None, &msgs, &Locale::th())
        .await
        .unwrap_err();
    let first = pushes(&h.http().take_requests());
    h.http().respond_with(409, ACCEPTED);
    d.send_batch_with(
        &to(),
        None,
        &msgs,
        &Locale::th(),
        &SendOptions::new().resume_after(&err),
    )
    .await
    .unwrap();
    assert_eq!(pushes(&h.http().take_requests()), [first[1].clone()]);
    assert_eq!(app.count(METERED).await, 2);
    assert_eq!(rows(&app).await, [(0, 1, true), (1, 1, true)]);
}

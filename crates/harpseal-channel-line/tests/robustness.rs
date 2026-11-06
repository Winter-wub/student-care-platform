//! Metered units per Push request, `X-Line-Retry-Key`, and partial sends
//! (#35) on the wire.

mod common;

use std::sync::Arc;

use common::{LineApi, SECRET, TOKEN, USER, json};
use harpseal::{
    Card, CardList, Channel, ChannelAccountId, Delivery, Error, Locale, Message, Metering,
    ReplyToken, Timestamp,
};
use harpseal_channel_line::{KIND, LineChannel, LineConfig, RETRY_KEY_HEADER};

fn channel() -> (LineChannel, LineApi) {
    let api = LineApi::default();
    let cfg = LineConfig::new(SECRET, TOKEN, "2000000000-SealBay0");
    (LineChannel::new(cfg, Arc::new(api.clone())), api)
}

fn to() -> ChannelAccountId {
    ChannelAccountId::new(KIND, USER)
}

fn reply(token: &str) -> Delivery {
    Delivery::Reply(ReplyToken {
        token: token.into(),
        expires_at: Timestamp(i64::MAX),
    })
}

fn texts(n: usize) -> Vec<Message> {
    (1..=n).map(|i| Message::text(format!("ข่าว {i}"))).collect()
}

/// 61 Cards: 6 carousels (12 each, then 1), so Push requests of 5 and 1.
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

fn retry_keys(reqs: &[harpseal_testing::RecordedRequest]) -> Vec<Option<String>> {
    reqs.iter()
        .map(|r| {
            r.headers
                .get(RETRY_KEY_HEADER)
                .map(|v| v.to_str().unwrap().to_string())
        })
        .collect()
}

#[tokio::test]
async fn one_message_over_two_push_requests_costs_two_units() {
    let (ch, api) = channel();
    let r = ch
        .send(&to(), &Delivery::Push, &cards61(), &Locale::th())
        .await
        .unwrap();
    let reqs = api.take();
    let sizes: Vec<usize> = reqs
        .iter()
        .map(|r| json(&r.body)["messages"].as_array().unwrap().len())
        .collect();
    assert_eq!(sizes, [5, 1]);
    assert_eq!((r.metered_units, r.metering), (2, Metering::Metered));
    // By Reply: 5 carousels free, the sixth in one metered Push.
    let r = ch
        .send(&to(), &reply("rt-61"), &cards61(), &Locale::th())
        .await
        .unwrap();
    assert_eq!(r.metered_units, 1);
    assert!(r.pushed);
}

#[tokio::test]
async fn pushes_carry_a_retry_key_derived_from_the_batch_key() {
    let (ch, api) = channel();
    let d = reply("rt-7");
    let first = ch
        .send_batch_keyed(&to(), &d, &texts(12), &Locale::th(), "job-1/u1")
        .await
        .into_result()
        .unwrap();
    assert_eq!(first.len(), 12);
    let keys = retry_keys(&api.take());
    assert_eq!(keys.len(), 3, "a reply of 5 and Pushes of 5 and 2");
    assert_eq!(keys[0], None, "a reply has no retry key");
    let (k1, k2) = (keys[1].clone().unwrap(), keys[2].clone().unwrap());
    assert_ne!(k1, k2, "one key per request");
    assert_eq!(k1.len(), 36, "a UUID");
    // A retry of the same batch with the same key reuses both keys.
    ch.send_batch_keyed(&to(), &d, &texts(12), &Locale::th(), "job-1/u1")
        .await;
    let again = retry_keys(&api.take());
    assert_eq!(again[1..], keys[1..]);
    // Another batch key, other keys; no batch key, no header.
    ch.send_batch_keyed(&to(), &d, &texts(12), &Locale::th(), "job-2/u1")
        .await;
    assert_ne!(retry_keys(&api.take())[1], keys[1]);
    ch.send_batch(&to(), &Delivery::Push, &texts(2), &Locale::th())
        .await;
    assert_eq!(retry_keys(&api.take()), [None]);
}

#[tokio::test]
async fn a_409_for_an_accepted_retry_key_counts_as_sent() {
    let (ch, api) = channel();
    api.http()
        .respond_with(409, r#"{"message":"The retry key is already accepted"}"#);
    let out = ch
        .send_batch_keyed(&to(), &Delivery::Push, &texts(2), &Locale::th(), "k")
        .await;
    assert!(out.error.is_none(), "{:?}", out.error);
    assert_eq!(out.receipts.len(), 2);
    assert_eq!(out.receipts[0].metered_units, 1);
    assert!(out.receipts.iter().all(|r| r.message_id.is_none()));
    // Without a retry key a 409 is a failure.
    api.http().respond_with(409, r#"{"message":"conflict"}"#);
    let out = ch
        .send_batch(&to(), &Delivery::Push, &texts(1), &Locale::th())
        .await;
    assert!(matches!(out.error, Some(Error::Platform(_))));
}

#[tokio::test]
async fn a_message_whose_first_push_went_out_is_unfinished() {
    let (ch, api) = channel();
    api.http().respond_with(200, "{}");
    api.http()
        .respond_with(429, r#"{"message":"Too many requests"}"#);
    let out = ch
        .send_batch_keyed(
            &to(),
            &Delivery::Push,
            &[Message::text("ก่อน"), cards61()],
            &Locale::th(),
            "k",
        )
        .await;
    // Push 1: the text + 4 carousels; push 2 (2 carousels) fails.
    assert_eq!(out.receipts.len(), 1, "the text went out whole");
    let unfinished = out.unfinished.expect("4 carousels went out");
    assert_eq!(out.receipts[0].metered_units, 1);
    assert_eq!(
        unfinished.metered_units, 0,
        "it opened no request that went"
    );
    assert!(matches!(out.error, Some(Error::RateLimited { .. })));
    assert_eq!(api.take().len(), 2);
}

#[tokio::test]
async fn a_failure_on_a_message_boundary_has_nothing_unfinished() {
    let (ch, api) = channel();
    api.http().respond_with(200, "{}");
    api.http().respond_with(500, r#"{"message":"boom"}"#);
    let out = ch
        .send_batch(&to(), &Delivery::Push, &texts(7), &Locale::th())
        .await;
    assert_eq!(out.receipts.len(), 5);
    assert!(out.unfinished.is_none());
    assert!(matches!(out.error, Some(Error::Platform(_))));
    assert_eq!(api.take().len(), 2);
}

#[tokio::test]
async fn multicast_reports_whole_groups_and_a_retry_reuses_their_keys() {
    let (ch, api) = channel();
    let m = ch.multicast().unwrap();
    let to: Vec<ChannelAccountId> = (0..501)
        .map(|i| ChannelAccountId::new(KIND, format!("U{i:032x}")))
        .collect();
    let msgs = texts(6);
    api.http().respond_with(200, "{}");
    api.http().respond_with(200, "{}");
    api.http().respond_with(429, "{}");
    let out = m.send_many(&to, &msgs, &Locale::th(), "news-9").await;
    assert_eq!(out.receipts.len(), 500 * 6, "the first group of 500");
    assert!(matches!(out.error, Some(Error::RateLimited { .. })));
    let units: u32 = out.receipts.iter().map(|r| r.metered_units).sum();
    assert_eq!(units, 500 * 2, "two requests per recipient");
    let first = retry_keys(&api.take());
    assert!(first.iter().all(Option::is_some));
    // Resume with the recipients not reported, same key: the request that
    // failed is rebuilt with the same retry key.
    let done = out.receipts.len() / msgs.len();
    let out = m
        .send_many(&to[done..], &msgs, &Locale::th(), "news-9")
        .await;
    assert_eq!(out.receipts.len(), 6);
    let retried = retry_keys(&api.take());
    assert_eq!(retried[0], first[2]);
}

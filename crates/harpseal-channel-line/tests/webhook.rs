//! Every recorded webhook fixture parsed in full, and the signature cases.

mod common;

use std::sync::Arc;

use common::{LineApi, SECRET, TOKEN, USER, fixture, fixture_body, signed};
use harpseal::{Channel, Error, Inbound, Timestamp};
use harpseal_channel_line::{KIND, LineChannel, LineConfig};

const AT: i64 = 1_793_836_800_000;

fn channel() -> LineChannel {
    LineChannel::new(
        LineConfig::new(SECRET, TOKEN, ""),
        Arc::new(LineApi::default()),
    )
}

async fn one(name: &str) -> Inbound {
    let mut got = channel().parse_webhook(fixture(name)).await.unwrap();
    assert_eq!(got.len(), 1, "{name}");
    let ev = got.remove(0);
    assert_eq!(ev.from().channel, KIND);
    assert_eq!(ev.from().external_id, USER);
    ev
}

fn token(ev: &Inbound) -> &str {
    let r = ev.reply().expect("a reply token");
    assert_eq!(r.expires_at, Timestamp(AT + 60_000));
    &r.token
}

#[tokio::test]
async fn every_fixture_becomes_its_inbound() {
    let ev = one("text").await;
    assert!(matches!(&ev, Inbound::Text { text, .. } if text == "สอบถามค่าเทอมครับ"));
    assert_eq!(token(&ev), "b60d432864f44d079f6d8efe86cf404b");

    let ev = one("file").await;
    let Inbound::File { file, .. } = &ev else {
        panic!("{ev:?}")
    };
    assert_eq!(file.platform_file_id, "468789577898262530");
    assert_eq!(file.file_name.as_deref(), Some("ใบลาป่วย.pdf"));
    assert_eq!(file.size_bytes, Some(2138));
    token(&ev);

    let ev = one("image").await;
    let Inbound::File { file, .. } = &ev else {
        panic!("{ev:?}")
    };
    assert_eq!(file.platform_file_id, "468789577898262531");
    assert_eq!(file.file_name, None);

    let ev = one("postback").await;
    assert!(matches!(&ev, Inbound::Action { action, .. } if action.as_str() == "news.ack.1042"));
    assert_eq!(token(&ev), "d81f654086166f29b18f0a1fa8e1626d");

    let ev = one("follow").await;
    assert!(matches!(ev, Inbound::Follow { .. }));
    token(&ev);

    let ev = one("refollow").await;
    assert!(
        matches!(ev, Inbound::Unblocked { .. }),
        "follow.isUnblocked"
    );
    token(&ev);

    assert!(matches!(one("unfollow").await, Inbound::Blocked { .. }));
}

#[tokio::test]
async fn signatures_are_checked_before_parsing() {
    let ch = channel();
    let body = fixture_body("text");
    let mut missing = signed(&body, SECRET);
    missing.headers.clear();
    assert!(matches!(
        ch.parse_webhook(missing).await,
        Err(Error::BadSignature)
    ));
    let wrong = signed(&body, "another-secret");
    assert!(matches!(
        ch.parse_webhook(wrong).await,
        Err(Error::BadSignature)
    ));
    // A body that is not even JSON: still BadSignature, so never parsed.
    let garbage = signed(b"\xff\xfe not json", "another-secret");
    assert!(matches!(
        ch.parse_webhook(garbage).await,
        Err(Error::BadSignature)
    ));
    let signed_garbage = signed(b"{not json", SECRET);
    assert!(matches!(
        ch.parse_webhook(signed_garbage).await,
        Err(Error::Invalid(_))
    ));
    // One flipped byte in a signed body.
    let mut tampered = signed(&body, SECRET);
    tampered.body[40] ^= 1;
    assert!(matches!(
        ch.parse_webhook(tampered).await,
        Err(Error::BadSignature)
    ));
}

#[tokio::test]
async fn an_empty_event_list_is_the_console_verify_call() {
    let ch = channel();
    let req = signed(br#"{"destination":"Useal0bay0bot","events":[]}"#, SECRET);
    assert!(ch.parse_webhook(req).await.unwrap().is_empty());
}

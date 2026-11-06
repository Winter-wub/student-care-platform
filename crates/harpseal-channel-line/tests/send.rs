//! Reply, Push, multicast and content download on the wire: the exact
//! request JSON, receipts and metering, and how LINE errors surface.

mod common;

use std::sync::Arc;

use common::{LineApi, SECRET, TOKEN, USER, json};
use harpseal::{
    Attachment, Capability, Channel, ChannelAccountId, Delivery, Error, IncomingFile, Locale,
    Message, Metering, ReplyToken, Timestamp,
};
use harpseal_channel_line::{KIND, LineChannel, LineConfig, render};
use serde_json::json;

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

#[tokio::test]
async fn a_reply_carries_the_rendered_objects() {
    let (ch, api) = channel();
    let msg = harpseal::fixtures::news_card();
    let r = ch
        .send(&to(), &reply("rt-1"), &msg, &Locale::th())
        .await
        .unwrap();
    let reqs = api.take();
    assert_eq!(reqs.len(), 1);
    assert_eq!(reqs[0].method, http::Method::POST);
    assert_eq!(reqs[0].uri, "https://api.line.me/v2/bot/message/reply");
    assert_eq!(reqs[0].headers["content-type"], "application/json");
    assert_eq!(
        reqs[0].headers["authorization"],
        format!("Bearer {TOKEN}").as_str()
    );
    let want = render::message(&msg, &ch.render_cx(&Locale::th()));
    assert_eq!(
        json(&reqs[0].body),
        json!({ "replyToken": "rt-1", "messages": want })
    );
    assert_eq!(r.metering, Metering::Free);
    assert!(!r.pushed);
    assert_eq!(r.message_id.unwrap().0, "500000000000000001");
}

#[tokio::test]
async fn seven_objects_are_a_reply_of_5_and_one_metered_push_of_2() {
    let (ch, api) = channel();
    let receipts = ch
        .send_batch(&to(), &reply("rt-7"), &texts(7), &Locale::th())
        .await
        .into_result()
        .unwrap();
    let reqs = api.take();
    let uris: Vec<&str> = reqs.iter().map(|r| r.uri.as_str()).collect();
    assert_eq!(
        uris,
        [
            "https://api.line.me/v2/bot/message/reply",
            "https://api.line.me/v2/bot/message/push"
        ]
    );
    let push = json(&reqs[1].body);
    assert_eq!(push["to"], USER);
    assert_eq!(
        push["messages"],
        json!([
            { "type": "text", "text": "ข่าว 6" }, { "type": "text", "text": "ข่าว 7" }
        ])
    );
    let metering: Vec<Metering> = receipts.iter().map(|r| r.metering).collect();
    let mut want = vec![Metering::Free; 7];
    want[5] = Metering::Metered;
    assert_eq!(
        metering, want,
        "one metered unit, on the Message opening the Push"
    );
    let pushed: Vec<bool> = receipts.iter().map(|r| r.pushed).collect();
    assert_eq!(pushed, [false, false, false, false, false, true, true]);
}

#[tokio::test]
async fn twelve_pushed_objects_are_three_metered_requests() {
    let (ch, api) = channel();
    let receipts = ch
        .send_batch(&to(), &Delivery::Push, &texts(12), &Locale::th())
        .await
        .into_result()
        .unwrap();
    let sizes: Vec<usize> = api
        .take()
        .iter()
        .map(|r| json(&r.body)["messages"].as_array().unwrap().len())
        .collect();
    assert_eq!(sizes, [5, 5, 2]);
    let metered: Vec<usize> = (0..12)
        .filter(|i| receipts[*i].metering == Metering::Metered)
        .collect();
    assert_eq!(metered, [0, 5, 10]);
}

#[tokio::test]
async fn a_spent_reply_token_falls_back_to_push() {
    let (ch, api) = channel();
    api.http()
        .respond_with(400, r#"{"message":"Invalid reply token"}"#);
    let r = ch
        .send(
            &to(),
            &reply("rt-old"),
            &Message::text("สวัสดี"),
            &Locale::th(),
        )
        .await
        .unwrap();
    let reqs = api.take();
    assert!(reqs[0].uri.ends_with("/reply"));
    assert!(reqs[1].uri.ends_with("/push"));
    assert_eq!(r.metering, Metering::Metered);
    assert!(r.pushed);
}

#[tokio::test]
async fn a_403_on_push_is_unreachable_and_a_429_is_rate_limited() {
    let (ch, api) = channel();
    api.http().respond_with(403, r#"{"message":"Not found"}"#);
    let blocked = ch
        .send(&to(), &Delivery::Push, &Message::text("x"), &Locale::th())
        .await;
    assert!(matches!(blocked, Err(Error::Unreachable)), "{blocked:?}");
    let (ch, api) = channel();
    api.http()
        .respond_with(429, r#"{"message":"You have reached your monthly limit."}"#);
    let limited = ch
        .send(&to(), &Delivery::Push, &Message::text("x"), &Locale::th())
        .await;
    assert!(matches!(
        limited,
        Err(Error::RateLimited {
            retry_after_secs: 60
        })
    ));
}

#[tokio::test]
async fn a_failed_request_reports_the_messages_sent_before_it() {
    let (ch, api) = channel();
    api.http().respond_with(200, "{}");
    api.http().respond_with(500, r#"{"message":"boom"}"#);
    let out = ch
        .send_batch(&to(), &Delivery::Push, &texts(7), &Locale::th())
        .await;
    assert_eq!(out.receipts.len(), 5);
    assert!(matches!(out.error, Some(Error::Platform(_))));
}

#[tokio::test]
async fn an_attachment_is_unsupported_and_stops_the_batch() {
    let (ch, api) = channel();
    let file = Message::Attachment(Attachment {
        file_key: "files/a.pdf".into(),
        file_name: "ตารางสอบ.pdf".into(),
        content_type: "application/pdf".into(),
        size_bytes: 10,
        url: "https://files.sealbay.example/a.pdf".into(),
    });
    let out = ch
        .send_batch(
            &to(),
            &Delivery::Push,
            &[Message::text("ก่อน"), file],
            &Locale::th(),
        )
        .await;
    assert_eq!(out.receipts.len(), 1, "the text before it went out");
    assert!(matches!(
        out.error,
        Some(Error::Unsupported(Capability::SendAttachment))
    ));
    assert_eq!(api.take().len(), 1);
    assert!(!ch.capabilities().send_attachment);
}

#[tokio::test]
async fn multicast_takes_500_recipients_per_request_metered_per_recipient() {
    let (ch, api) = channel();
    assert!(ch.capabilities().has(Capability::Multicast));
    let m = ch.multicast().expect("LINE multicasts");
    assert_eq!(m.max_recipients(), 500);
    let to: Vec<ChannelAccountId> = (0..501)
        .map(|i| ChannelAccountId::new(KIND, format!("U{i:032x}")))
        .collect();
    let msgs = texts(6);
    let receipts = m.send_many(&to, &msgs, &Locale::th()).await.unwrap();
    let reqs = api.take();
    assert!(
        reqs.iter()
            .all(|r| r.uri == "https://api.line.me/v2/bot/message/multicast")
    );
    let shape: Vec<(usize, usize)> = reqs
        .iter()
        .map(|r| {
            let v = json(&r.body);
            (
                v["to"].as_array().unwrap().len(),
                v["messages"].as_array().unwrap().len(),
            )
        })
        .collect();
    assert_eq!(shape, [(500, 5), (500, 1), (1, 5), (1, 1)]);
    assert_eq!(json(&reqs[3].body)["to"], json!([to[500].external_id]));
    assert_eq!(receipts.len(), 501 * 6);
    let metered = receipts
        .iter()
        .filter(|r| r.metering == Metering::Metered)
        .count();
    assert_eq!(metered, 501 * 2, "two requests' worth per recipient");
    assert_eq!(
        receipts[6].metering,
        Metering::Metered,
        "recipient 1, message 0"
    );
    assert_eq!(
        receipts[11].metering,
        Metering::Metered,
        "recipient 1, message 5"
    );
    assert!(receipts.iter().all(|r| r.pushed));
}

#[tokio::test]
async fn multicast_refuses_other_channels_and_attachments() {
    let (ch, api) = channel();
    let m = ch.multicast().unwrap();
    let other = [ChannelAccountId::new(
        harpseal::ChannelKind("telegram"),
        "1",
    )];
    let got = m.send_many(&other, &texts(1), &Locale::th()).await;
    assert!(matches!(got, Err(Error::Invalid(_))));
    let file = Message::Attachment(Attachment {
        file_key: "files/a.png".into(),
        file_name: "a.png".into(),
        content_type: "image/png".into(),
        size_bytes: 10,
        url: "https://files.sealbay.example/a.png".into(),
    });
    let got = m.send_many(&[to()], &[file], &Locale::th()).await;
    assert!(matches!(
        got,
        Err(Error::Unsupported(Capability::SendAttachment))
    ));
    assert!(api.take().is_empty());
}

#[tokio::test]
async fn files_download_from_the_content_api() {
    let (ch, api) = channel();
    api.http().respond_with(200, b"%PDF-1.7".to_vec());
    let file = IncomingFile {
        platform_file_id: "468789577898262530".into(),
        file_name: Some("ใบลาป่วย.pdf".into()),
        content_type: None,
        size_bytes: Some(2138),
    };
    assert_eq!(ch.download(&file).await.unwrap(), b"%PDF-1.7");
    let reqs = api.take();
    assert_eq!(reqs[0].method, http::Method::GET);
    assert_eq!(
        reqs[0].uri,
        "https://api-data.line.me/v2/bot/message/468789577898262530/content"
    );
    assert_eq!(
        reqs[0].headers["authorization"],
        format!("Bearer {TOKEN}").as_str()
    );

    let external = IncomingFile {
        platform_file_id: "https://img.sealbay.example/a.jpg".into(),
        ..file.clone()
    };
    ch.download(&external).await.unwrap();
    let reqs = api.take();
    assert_eq!(reqs[0].uri, "https://img.sealbay.example/a.jpg");
    assert!(
        reqs[0].headers.get("authorization").is_none(),
        "no token to third parties"
    );

    let bad = IncomingFile {
        platform_file_id: "../richmenu".into(),
        ..file
    };
    assert!(matches!(ch.download(&bad).await, Err(Error::Invalid(_))));
}

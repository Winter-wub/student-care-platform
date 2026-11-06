//! Partial sends (#35): a Message that renders into several Telegram
//! messages and fails part-way reports the parts that went out.

mod common;

use common::*;
use harpseal::fixtures;
use harpseal::{Channel, Delivery, Error, Locale, Message};

const FLOOD: &str = r#"{"ok":false,"error_code":429,"description":"Too Many Requests: retry after 5","parameters":{"retry_after":5}}"#;

fn ok(id: i64) -> String {
    format!(r#"{{"ok":true,"result":{{"message_id":{id}}}}}"#)
}

#[tokio::test]
async fn a_card_list_failing_at_card_3_reports_2_cards_unfinished() {
    let (tg, http) = channel();
    for id in [501, 502, 503] {
        http.respond_with(200, ok(id));
    }
    http.respond_with(429, FLOOD);
    let out = tg
        .send_batch(
            &chat(),
            &Delivery::Push,
            &[Message::text("ข่าวประจำสัปดาห์"), fixtures::news_list_15()],
            &Locale::th(),
        )
        .await;
    assert_eq!(out.receipts.len(), 1, "the text went out whole");
    let part = out.unfinished.expect("two Cards went out");
    assert_eq!(part.message_id.unwrap().0, "503", "the last part's id");
    assert_eq!(part.metered_units, 0, "Telegram never meters");
    assert!(matches!(
        out.error,
        Some(Error::RateLimited {
            retry_after_secs: 5
        })
    ));
    assert_eq!(calls(&http).len(), 4, "nothing after the refused Card");
}

#[tokio::test]
async fn keyboard_follow_ups_report_the_messages_before_them() {
    let (tg, http) = channel();
    http.respond_with(200, ok(700));
    http.respond_with(429, FLOOD);
    let out = tg
        .send_batch(
            &chat(),
            &Delivery::Push,
            &[fixtures::degrade_choices_120()],
            &Locale::th(),
        )
        .await;
    assert!(out.receipts.is_empty());
    assert_eq!(out.unfinished.unwrap().message_id.unwrap().0, "700");
    assert!(out.error.is_some());
}

#[tokio::test]
async fn a_failure_on_the_first_part_has_nothing_unfinished() {
    let (tg, http) = channel();
    http.respond_with(429, FLOOD);
    let out = tg
        .send_batch(
            &chat(),
            &Delivery::Push,
            &[fixtures::news_list_15()],
            &Locale::th(),
        )
        .await;
    assert!(out.receipts.is_empty());
    assert!(out.unfinished.is_none());
    assert!(matches!(out.error, Some(Error::RateLimited { .. })));
    // `send` alone still reports the bare error.
    http.respond_with(200, ok(1));
    http.respond_with(429, FLOOD);
    let got = tg
        .send(
            &chat(),
            &Delivery::Push,
            &fixtures::news_list_15(),
            &Locale::th(),
        )
        .await;
    assert!(matches!(got, Err(Error::RateLimited { .. })));
}

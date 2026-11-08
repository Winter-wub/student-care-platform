//! `Button::Url` (#108) on the Fake Channel: recorded as sent, on both
//! profiles; a URL that is not https is refused like the real adapters
//! refuse it (an error, nothing recorded).

use harpseal::{Button, Card, Channel, Delivery, Error, Locale, Message};
use harpseal_channel_fake::FakeChannel;

const URL: &str = "https://reg.seal-bay.invalid/exam";

fn card(b: Button) -> Message {
    Message::Card(Card {
        title: "ตารางสอบ".into(),
        body: "ดูได้ที่ปุ่ม".into(),
        buttons: vec![Button::mini_app("เปิดดูรายละเอียด", "/app/x"), b],
    })
}

#[tokio::test]
async fn url_buttons_are_recorded_and_bad_ones_refused() {
    for fake in [
        FakeChannel::new("fake-secret-0123456789")
            .unwrap()
            .line_like(),
        FakeChannel::new("fake-secret-0123456789")
            .unwrap()
            .telegram_like(),
    ] {
        let to = FakeChannel::account("u1");
        let ok = card(Button::url("ตารางสอบ", URL).unwrap());
        fake.send(&to, &Delivery::Push, &ok, &Locale::th())
            .await
            .unwrap();
        let sent = fake.sent_to(&to);
        let Some(Message::Card(c)) = sent.last() else {
            panic!("{sent:?}")
        };
        assert_eq!(c.buttons[1].safe_url(), Some(URL));

        let bad = card(harpseal::fixtures::raw_url_button(
            "x",
            "javascript:alert(1)",
        ));
        let e = fake.send(&to, &Delivery::Push, &bad, &Locale::th()).await;
        assert!(matches!(e, Err(Error::Invalid(_))), "{e:?}");
        assert_eq!(fake.sent_to(&to).len(), sent.len(), "nothing recorded");
    }
}

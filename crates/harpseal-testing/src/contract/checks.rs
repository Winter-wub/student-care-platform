//! The individual contract checks. Each takes a fresh [`Target`] and panics
//! with a message naming the Channel and what it got wrong.

use harpseal::{
    ActionId, Attachment, Button, Capability, Card, CardList, Choices, Delivery, Error, Inbound,
    Locale, Menu, MenuEntry, Message, Metering, ReplyToken, SendReceipt, Timestamp,
};

use std::sync::atomic::{AtomicU64, Ordering};

use super::{Call, Target};

fn id(s: &str) -> ActionId {
    ActionId::new(s).expect("contract Action ids are short")
}

fn card(n: usize) -> Card {
    Card {
        title: format!("ประกาศ {n}"),
        body: "มหาวิทยาลัยอ่าวแมวน้ำ".into(),
        buttons: vec![Button::action("รับทราบ", id(&format!("contract.card.{n}")))],
    }
}

fn choices(text: String, n: usize, prefix: &str) -> Message {
    Message::Choices(Choices {
        text,
        options: (0..n)
            .map(|i| Button::action(format!("ตัวเลือก {i}"), id(&format!("{prefix}.{i}"))))
            .collect(),
    })
}

/// A fresh reply token (never expiring during the suite) for each Reply
/// send: platforms such as LINE accept a token once.
fn reply() -> Delivery {
    static NEXT: AtomicU64 = AtomicU64::new(1);
    let n = NEXT.fetch_add(1, Ordering::Relaxed);
    Delivery::Reply(ReplyToken {
        token: format!("contract-reply-{n}"),
        expires_at: Timestamp(i64::MAX),
    })
}

/// The requests of one send are consistent with its receipts: at most
/// one reply call, only with the given token and within the reply limit;
/// exactly one metered receipt per metered Push request.
fn check_calls(t: &Target, delivery: &Delivery, receipts: &[SendReceipt], what: &str) {
    let kind = t.channel.kind();
    let log = t.wire.take();
    let replies: Vec<&Call> = log
        .calls
        .iter()
        .filter(|c| c.reply_token.is_some())
        .collect();
    assert!(
        replies.len() <= 1,
        "{kind}: {what}: one reply call per token, got {replies:?}"
    );
    if let Some(c) = replies.first() {
        let given = delivery.reply_token().map(|r| r.token.as_str());
        assert_eq!(
            c.reply_token.as_deref(),
            given,
            "{kind}: {what}: the given token"
        );
        let limit = t.wire.reply_limit().unwrap_or(usize::MAX);
        assert!(
            c.objects <= limit,
            "{kind}: {what}: {} objects in one reply",
            c.objects
        );
    }
    let pushes = log.calls.iter().filter(|c| c.reply_token.is_none()).count();
    let per_push = usize::from(t.wire.metering(&Delivery::Push) == Metering::Metered);
    let metered = receipts
        .iter()
        .filter(|r| r.metering == Metering::Metered)
        .count();
    assert_eq!(
        metered,
        pushes * per_push,
        "{kind}: {what}: one metered unit per Push request"
    );
    let any_pushed = receipts.iter().any(|r| r.pushed);
    assert_eq!(
        any_pushed,
        pushes > 0,
        "{kind}: {what}: `pushed` matches the Push requests"
    );
}

/// A bad signature is `BadSignature` even over an unparseable body, and
/// over a valid one; so is a missing signature. A good signature over an
/// unparseable body is a parse error instead.
pub async fn signature_is_checked_before_parsing(t: &Target) {
    let kind = t.channel.kind();
    let f = &t.fixtures;
    let refused = [
        ("a bad signature over an unparseable body", &f.bad_signature),
        (
            "a bad signature over a valid body",
            &f.bad_signature_valid_body,
        ),
        ("a missing signature", &f.missing_signature),
    ];
    for (what, req) in refused {
        let got = t.channel.parse_webhook(req.clone()).await;
        assert!(
            matches!(got, Err(Error::BadSignature)),
            "{kind}: {what} must be BadSignature, got {got:?}"
        );
    }
    let garbage = t
        .channel
        .parse_webhook(t.fixtures.signed_garbage.clone())
        .await;
    assert!(
        matches!(garbage, Err(ref e) if !matches!(e, Error::BadSignature)),
        "{kind}: a signed but unparseable body must be a parse error, got {garbage:?}"
    );
}

/// Each recorded webhook becomes exactly one Inbound of the right kind,
/// from an account on this Channel.
pub async fn parses_each_inbound_kind(t: &Target) {
    let kind = t.channel.kind();
    let f = &t.fixtures;
    let cases = [
        ("follow", &f.follow),
        ("text", &f.text),
        ("file", &f.file),
        ("action", &f.action),
        ("blocked", &f.blocked),
        ("unblocked", &f.unblocked),
    ];
    for (want, req) in cases {
        let events = t
            .channel
            .parse_webhook(req.clone())
            .await
            .unwrap_or_else(|e| panic!("{kind}: the {want} fixture must parse: {e}"));
        assert_eq!(
            events.len(),
            1,
            "{kind}: the {want} fixture holds one event"
        );
        let ev = &events[0];
        assert_eq!(ev.kind(), want, "{kind}: the {want} fixture");
        assert_eq!(
            ev.from().channel,
            kind,
            "{kind}: {want} comes from this Channel"
        );
        assert!(
            !ev.from().external_id.is_empty(),
            "{kind}: {want} has an account"
        );
        match ev {
            Inbound::Action { action, .. } => assert_eq!(action, &f.action_id, "{kind}"),
            Inbound::File { file, .. } => {
                assert!(!file.platform_file_id.is_empty(), "{kind}: file id")
            }
            Inbound::Text { text, .. } => assert!(!text.is_empty(), "{kind}: text"),
            _ => {}
        }
    }
}

/// Text, Card, CardList and Choices send by Push and by Reply, each with a
/// receipt whose metering is what the adapter declares; a 7-message batch
/// gets 7 receipts.
pub async fn sends_the_minimum(t: &Target) {
    let kind = t.channel.kind();
    let to = &t.fixtures.to;
    let locale = Locale::th();
    let minimum = [
        Message::text("สวัสดีจากมหาวิทยาลัยอ่าวแมวน้ำ"),
        Message::Card(card(1)),
        Message::CardList(CardList(vec![card(2), card(3)])),
        choices("เลือกหนึ่งข้อ".into(), 3, "contract.min"),
    ];
    for push in [true, false] {
        for msg in &minimum {
            let delivery = if push { Delivery::Push } else { reply() };
            let r = t
                .channel
                .send(to, &delivery, msg, &locale)
                .await
                .unwrap_or_else(|e| panic!("{kind}: sending a {} failed: {e}", msg.kind()));
            let what = format!("a {} sent as {delivery:?}", msg.kind());
            check_calls(t, &delivery, std::slice::from_ref(&r), &what);
        }
    }
    let batch: Vec<Message> = (0..7)
        .map(|i| Message::text(format!("ข้อความ {i}")))
        .collect();
    let delivery = reply();
    let receipts = t
        .channel
        .send_batch(to, &delivery, &batch, &locale)
        .await
        .into_result()
        .unwrap_or_else(|e| panic!("{kind}: a batch of 7 failed: {e}"));
    assert_eq!(receipts.len(), 7, "{kind}: one receipt per Message");
    check_calls(t, &delivery, &receipts, "a batch of 7 sent as Reply");
    t.wire.take();
}

/// The Menu can be set and reset to the locked Menu.
pub async fn menu_set_and_unset(t: &Target) {
    let kind = t.channel.kind();
    let menu = Menu {
        entries: vec![MenuEntry {
            label: "ข่าวล่าสุด".into(),
            action: id("contract.menu.news"),
        }],
    };
    let to = &t.fixtures.to;
    let set = t.channel.set_menu(to, Some(&menu)).await;
    assert!(set.is_ok(), "{kind}: setting a Menu failed: {set:?}");
    assert_eq!(t.wire.menu_entries(to), Some(1), "{kind}: the Menu is set");
    let unset = t.channel.set_menu(to, None).await;
    assert!(unset.is_ok(), "{kind}: locking the Menu failed: {unset:?}");
    assert_eq!(t.wire.menu_entries(to), None, "{kind}: the Menu is locked");
}

/// `capabilities()` matches the accessors, and an attachment without the
/// Capability is `Unsupported` with nothing sent.
pub async fn capabilities_agree(t: &Target) {
    let c = &t.channel;
    let kind = c.kind();
    let caps = c.capabilities();
    let pairs = [
        (Capability::EditMessage, c.editor().is_some()),
        (Capability::DeleteMessage, c.deleter().is_some()),
        (Capability::MiniApp, c.mini_app().is_some()),
        (Capability::SendAttachment, c.attachments().is_some()),
        (Capability::Multicast, c.multicast().is_some()),
    ];
    for (cap, present) in pairs {
        assert_eq!(caps.has(cap), present, "{kind}: {cap:?}");
    }
    if c.attachments().is_none() {
        let file = Message::Attachment(Attachment {
            file_key: "files/timetable.pdf".into(),
            file_name: "ตารางเรียน.pdf".into(),
            content_type: "application/pdf".into(),
            size_bytes: 1024,
            url: "https://files.seal-bay.invalid/timetable.pdf".into(),
        });
        let out = c
            .send_batch(&t.fixtures.to, &Delivery::Push, &[file], &Locale::th())
            .await;
        assert!(out.receipts.is_empty(), "{kind}: nothing is sent");
        assert!(
            matches!(
                out.error,
                Some(Error::Unsupported(Capability::SendAttachment))
            ),
            "{kind}: {:?}",
            out.error
        );
    }
}

/// Every Action button survives the degrade rules: more Cards than a
/// carousel holds, more options than quick replies (or a keyboard) hold, a
/// label too long for a quick reply and text longer than one message.
pub async fn degrade_keeps_every_action(t: &Target) {
    let kind = t.channel.kind();
    let long_label = Message::Choices(Choices {
        text: "เลือก".into(),
        options: vec![Button::action("ก".repeat(30), id("contract.label.0"))],
    });
    let cases = [
        Message::CardList(CardList((0..15).map(card).collect())),
        choices("เลือกวิชา".into(), 20, "contract.qr"),
        choices("ข้อความยาว ".repeat(600), 150, "contract.kb"),
        long_label,
    ];
    for msg in cases {
        let want = harpseal::render::action_ids(&msg);
        t.channel
            .send(&t.fixtures.to, &Delivery::Push, &msg, &Locale::th())
            .await
            .unwrap_or_else(|e| panic!("{kind}: sending a long {} failed: {e}", msg.kind()));
        let mut got = Vec::new();
        for payload in t.wire.take().action_payloads {
            got.push(press(t, &payload).await.as_str().to_string());
        }
        for a in &want {
            assert!(
                got.contains(a),
                "{kind}: Action {a} was dropped from a {}",
                msg.kind()
            );
        }
        assert_eq!(got.len(), want.len(), "{kind}: each Action exactly once");
    }
}

/// An Action id sent in a button comes back unchanged when pressed.
pub async fn action_ids_round_trip(t: &Target) {
    let kind = t.channel.kind();
    let want = id("contract.ping.ทดสอบ");
    let msg = Message::Choices(Choices {
        text: "กดปุ่ม".into(),
        options: vec![Button::action("ปิง", want.clone())],
    });
    t.channel
        .send(&t.fixtures.to, &Delivery::Push, &msg, &Locale::th())
        .await
        .unwrap_or_else(|e| panic!("{kind}: send failed: {e}"));
    let payloads = t.wire.take().action_payloads;
    assert_eq!(payloads.len(), 1, "{kind}: one button, one payload");
    assert_eq!(press(t, &payloads[0]).await, want, "{kind}: round trip");
}

async fn press(t: &Target, payload: &str) -> ActionId {
    let kind = t.channel.kind();
    let req = t.wire.press(&t.fixtures.to, payload);
    let events = t
        .channel
        .parse_webhook(req)
        .await
        .unwrap_or_else(|e| panic!("{kind}: pressing {payload:?} did not parse: {e}"));
    match events.as_slice() {
        [Inbound::Action { from, action, .. }] => {
            assert_eq!(
                from, &t.fixtures.to,
                "{kind}: the press comes from the account"
            );
            action.clone()
        }
        other => panic!("{kind}: pressing {payload:?} gave {other:?}"),
    }
}

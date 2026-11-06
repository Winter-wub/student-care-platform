//! The contract [`Target`] for the Fake Channel: recorded webhooks in its
//! wire format and a [`Wire`] reading its record.

use std::sync::Arc;

use harpseal::{ActionId, Delivery, Inbound, IncomingFile, Metering, ReplyToken, Timestamp};
use harpseal::{ChannelAccountId, WebhookRequest};
use harpseal_channel_fake::{FakeChannel, FakeProfile, SIGNATURE_HEADER};

use super::{Call, Fixtures, Target, Wire, WireLog};

/// Reads what the Fake Channel sent: the Action ids of every recorded
/// object are its wire payloads.
#[derive(Debug, Clone)]
pub struct FakeWire(pub FakeChannel);

impl Wire for FakeWire {
    fn take(&self) -> WireLog {
        let action_payloads = self
            .0
            .take_sent()
            .iter()
            .flat_map(|s| harpseal::render::action_ids(&s.message))
            .collect();
        let calls = self
            .0
            .take_calls()
            .into_iter()
            .map(|c| match c.reply_token {
                Some(t) => Call::reply(t, c.objects),
                None => Call::push(c.objects),
            })
            .collect();
        WireLog::new(action_payloads, calls)
    }
    fn press(&self, from: &ChannelAccountId, payload: &str) -> WebhookRequest {
        let body = serde_json::json!([{
            "type": "action",
            "from": from.external_id,
            "data": payload,
        }]);
        self.0.signed(body.to_string().into_bytes())
    }
    fn metering(&self, delivery: &Delivery) -> Metering {
        match delivery {
            Delivery::Push if self.0.profile().push_metered() => Metering::Metered,
            _ => Metering::Free,
        }
    }
    fn reply_limit(&self) -> Option<usize> {
        self.0.profile().reply_limit()
    }
    fn menu_entries(&self, to: &ChannelAccountId) -> Option<usize> {
        self.0.menu(to).flatten().map(|m| m.entries.len())
    }
}

/// A fresh Fake Channel in `profile` with its fixtures.
pub fn fake(profile: FakeProfile) -> Target {
    let ch = FakeChannel::new("contract-secret")
        .expect("secret is not empty")
        .with_profile(profile);
    let to = FakeChannel::account("U-contract-1");
    let reply = || {
        Some(ReplyToken {
            token: "r-contract".into(),
            expires_at: Timestamp(i64::MAX),
        })
    };
    let action_id = ActionId::new("contract.fixture.ack").expect("short id");
    let one = |ev: Inbound| ch.webhook(&[ev]);
    let follow = one(Inbound::Follow {
        from: to.clone(),
        reply: reply(),
        event_id: None,
        redelivery: false,
    });
    let wrong = |mut req: WebhookRequest| {
        req.headers.insert(
            SIGNATURE_HEADER,
            http::HeaderValue::from_static("0000000000"),
        );
        req
    };
    let mut missing_signature = follow.clone();
    missing_signature.headers.remove(SIGNATURE_HEADER);
    let fixtures = Fixtures::builder(to.clone())
        .follow(follow.clone())
        .text(one(Inbound::Text {
            from: to.clone(),
            text: "สวัสดีครับ".into(),
            reply: reply(),
            event_id: None,
            redelivery: false,
        }))
        .file(one(Inbound::File {
            from: to.clone(),
            file: IncomingFile {
                platform_file_id: "file-1".into(),
                file_name: Some("ใบลา.pdf".into()),
                content_type: Some("application/pdf".into()),
                size_bytes: Some(2048),
            },
            reply: reply(),
            event_id: None,
            redelivery: false,
        }))
        .action(
            one(Inbound::Action {
                from: to.clone(),
                action: action_id.clone(),
                reply: reply(),
                event_id: None,
                redelivery: false,
            }),
            action_id,
        )
        .blocked(one(Inbound::Blocked {
            from: to.clone(),
            event_id: None,
            redelivery: false,
        }))
        .unblocked(one(Inbound::Unblocked {
            from: to.clone(),
            reply: reply(),
            event_id: None,
            redelivery: false,
        }))
        .bad_signature(wrong(ch.signed(b"{not json".to_vec())))
        .bad_signature_valid_body(wrong(follow))
        .missing_signature(missing_signature)
        .signed_garbage(ch.signed(b"{not json".to_vec()))
        .build();
    let wire = Arc::new(FakeWire(ch.clone()));
    Target::new(Arc::new(ch), fixtures, wire)
}

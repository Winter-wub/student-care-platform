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
    fn menu_key(&self, to: &ChannelAccountId) -> Option<Option<String>> {
        Some(
            self.0
                .menu(to)
                .flatten()
                .and_then(|m| m.key().map(String::from)),
        )
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
    let follow = one(Inbound::follow(to.clone(), reply()));
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
        .text(one(Inbound::text(to.clone(), "สวัสดีครับ", reply())))
        .file(one(Inbound::file(
            to.clone(),
            IncomingFile {
                platform_file_id: "file-1".into(),
                file_name: Some("ใบลา.pdf".into()),
                content_type: Some("application/pdf".into()),
                size_bytes: Some(2048),
            },
            reply(),
        )))
        .action(
            one(Inbound::action(to.clone(), action_id.clone(), reply())),
            action_id,
        )
        .blocked(one(Inbound::blocked(to.clone())))
        .unblocked(one(Inbound::unblocked(to.clone(), reply())))
        .bad_signature(wrong(ch.signed(b"{not json".to_vec())))
        .bad_signature_valid_body(wrong(follow))
        .missing_signature(missing_signature)
        .signed_garbage(ch.signed(b"{not json".to_vec()))
        .build();
    let wire = Arc::new(FakeWire(ch.clone()));
    Target::new(Arc::new(ch), fixtures, wire)
}

//! The contract [`Target`] for the Fake Channel: recorded webhooks in its
//! wire format and a [`Wire`] reading its record.

use std::sync::Arc;

use harpseal::{ActionId, Delivery, Inbound, IncomingFile, Metering, ReplyToken, Timestamp};
use harpseal::{ChannelAccountId, WebhookRequest};
use harpseal_channel_fake::{FakeChannel, FakeProfile, SIGNATURE_HEADER};

use super::{Fixtures, Target, Wire};

/// Reads what the Fake Channel sent: the Action ids of every recorded
/// object are its wire payloads.
#[derive(Debug, Clone)]
pub struct FakeWire(pub FakeChannel);

impl Wire for FakeWire {
    fn take_action_payloads(&self) -> Vec<String> {
        self.0
            .take_sent()
            .iter()
            .flat_map(|s| harpseal::render::action_ids(&s.message))
            .collect()
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
    let mut bad_signature = ch.signed(b"{not json".to_vec());
    bad_signature.headers.insert(
        SIGNATURE_HEADER,
        http::HeaderValue::from_static("0000000000"),
    );
    let fixtures = Fixtures::new(
        one(Inbound::Follow {
            from: to.clone(),
            reply: reply(),
        }),
        one(Inbound::Text {
            from: to.clone(),
            text: "สวัสดีครับ".into(),
            reply: reply(),
        }),
        one(Inbound::File {
            from: to.clone(),
            file: IncomingFile {
                platform_file_id: "file-1".into(),
                file_name: Some("ใบลา.pdf".into()),
                content_type: Some("application/pdf".into()),
                size_bytes: Some(2048),
            },
            reply: reply(),
        }),
        (
            one(Inbound::Action {
                from: to.clone(),
                action: action_id.clone(),
                reply: reply(),
            }),
            action_id,
        ),
        one(Inbound::Blocked { from: to.clone() }),
        one(Inbound::Unblocked {
            from: to.clone(),
            reply: reply(),
        }),
        bad_signature,
        ch.signed(b"{not json".to_vec()),
        to,
    );
    let wire = Arc::new(FakeWire(ch.clone()));
    Target::new(Arc::new(ch), fixtures, wire)
}

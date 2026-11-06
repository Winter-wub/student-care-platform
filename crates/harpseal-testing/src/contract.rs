//! The Channel contract suite: what every adapter must do, checked through
//! `&dyn Channel` only (spec, "Testing Decisions"). An adapter crate runs it
//! from a test with a factory that builds a fresh [`Target`]:
//!
//! ```ignore
//! #[tokio::test]
//! async fn line_passes_the_channel_contract() {
//!     harpseal_testing::contract::run(|| my_target()).await;
//! }
//! ```
//!
//! A [`Target`] is the Channel, the recorded webhooks ([`Fixtures`]) and a
//! [`Wire`] that reads what the adapter put on the wire (from a
//! `RecordingHttp`, or the Fake Channel's record). [`fake`] builds one for
//! the Fake Channel in any profile.

mod checks;
mod fake;

use std::sync::Arc;

use harpseal::{ActionId, Channel, ChannelAccountId, Delivery, Metering, WebhookRequest};

pub use checks::{
    action_ids_round_trip, capabilities_agree, degrade_keeps_every_action, menu_set_and_unset,
    parses_each_inbound_kind, sends_the_minimum, signature_is_checked_before_parsing,
};
pub use fake::{FakeWire, fake};

/// Recorded webhook calls, one event each, as the platform sends them.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct Fixtures {
    /// The Student added the bot.
    pub follow: WebhookRequest,
    /// A text message.
    pub text: WebhookRequest,
    /// A file.
    pub file: WebhookRequest,
    /// A button press carrying [`Fixtures::action_id`].
    pub action: WebhookRequest,
    /// The Action id inside [`Fixtures::action`].
    pub action_id: ActionId,
    /// The Student blocked the bot.
    pub blocked: WebhookRequest,
    /// The Student unblocked the bot.
    pub unblocked: WebhookRequest,
    /// A wrong signature over a body the adapter could not parse: must be
    /// `BadSignature`, proving the signature is checked first.
    pub bad_signature: WebhookRequest,
    /// A correct signature over a body the adapter cannot parse: must be
    /// an error other than `BadSignature`.
    pub signed_garbage: WebhookRequest,
    /// The account the suite sends to.
    pub to: ChannelAccountId,
}

impl Fixtures {
    /// Every field; an adapter test fills them from its recorded payloads.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        follow: WebhookRequest,
        text: WebhookRequest,
        file: WebhookRequest,
        action: (WebhookRequest, ActionId),
        blocked: WebhookRequest,
        unblocked: WebhookRequest,
        bad_signature: WebhookRequest,
        signed_garbage: WebhookRequest,
        to: ChannelAccountId,
    ) -> Self {
        Self {
            follow,
            text,
            file,
            action: action.0,
            action_id: action.1,
            blocked,
            unblocked,
            bad_signature,
            signed_garbage,
            to,
        }
    }
}

/// What the suite needs to see beyond the Channel trait.
pub trait Wire: Send + Sync {
    /// The Action payloads (LINE postback `data`, Telegram `callback_data`,
    /// the Fake Channel's ids) the adapter sent since the last call, in
    /// order, and forgets them.
    fn take_action_payloads(&self) -> Vec<String>;
    /// A correctly signed webhook call in which `from` presses a button
    /// carrying `payload`.
    fn press(&self, from: &ChannelAccountId, payload: &str) -> WebhookRequest;
    /// The metering the adapter declares for an object sent with `delivery`.
    fn metering(&self, delivery: &Delivery) -> Metering;
}

/// One adapter under test.
#[derive(Clone)]
#[non_exhaustive]
pub struct Target {
    /// The adapter.
    pub channel: Arc<dyn Channel>,
    /// Its recorded webhooks.
    pub fixtures: Fixtures,
    /// Its wire reader.
    pub wire: Arc<dyn Wire>,
}

impl Target {
    /// A target.
    pub fn new(channel: Arc<dyn Channel>, fixtures: Fixtures, wire: Arc<dyn Wire>) -> Self {
        Self {
            channel,
            fixtures,
            wire,
        }
    }
}

/// Runs every check, each on a fresh [`Target`] from `factory`. Panics with
/// the failing check's message.
pub async fn run(factory: impl Fn() -> Target) {
    signature_is_checked_before_parsing(&factory()).await;
    parses_each_inbound_kind(&factory()).await;
    sends_the_minimum(&factory()).await;
    menu_set_and_unset(&factory()).await;
    capabilities_agree(&factory()).await;
    degrade_keeps_every_action(&factory()).await;
    action_ids_round_trip(&factory()).await;
}

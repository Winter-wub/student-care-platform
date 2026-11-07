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
mod menu;
mod resume;

use std::sync::Arc;

use harpseal::{ActionId, Channel, ChannelAccountId, Delivery, Metering, WebhookRequest};

pub use checks::{
    action_ids_round_trip, capabilities_agree, degrade_keeps_every_action,
    metering_counts_push_requests, parses_each_inbound_kind, sends_the_minimum,
    signature_is_checked_before_parsing,
};
pub use fake::{FakeWire, fake};
pub use menu::{keyed_menu_is_set, menu_set_and_unset, relink_menu_reaches_the_account};
pub use resume::a_foreign_resume_token_is_ignored;

/// Recorded webhook calls, one event each, as the platform sends them.
/// Build with [`Fixtures::builder`]: new fixtures can be added later
/// without breaking adapter tests.
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
    /// A wrong signature over a valid body: must be `BadSignature`.
    pub bad_signature_valid_body: WebhookRequest,
    /// A valid body with no signature header: must be `BadSignature`.
    pub missing_signature: WebhookRequest,
    /// A correct signature over a body the adapter cannot parse: must be
    /// an error other than `BadSignature`.
    pub signed_garbage: WebhookRequest,
    /// The account the suite sends to.
    pub to: ChannelAccountId,
}

impl Fixtures {
    /// Starts a set whose sends go to `to`.
    pub fn builder(to: ChannelAccountId) -> FixturesBuilder {
        FixturesBuilder {
            to,
            follow: None,
            text: None,
            file: None,
            action: None,
            blocked: None,
            unblocked: None,
            bad_signature: None,
            bad_signature_valid_body: None,
            missing_signature: None,
            signed_garbage: None,
        }
    }
}

/// Collects [`Fixtures`]; [`FixturesBuilder::build`] names any missing one.
#[derive(Debug, Clone)]
pub struct FixturesBuilder {
    to: ChannelAccountId,
    follow: Option<WebhookRequest>,
    text: Option<WebhookRequest>,
    file: Option<WebhookRequest>,
    action: Option<(WebhookRequest, ActionId)>,
    blocked: Option<WebhookRequest>,
    unblocked: Option<WebhookRequest>,
    bad_signature: Option<WebhookRequest>,
    bad_signature_valid_body: Option<WebhookRequest>,
    missing_signature: Option<WebhookRequest>,
    signed_garbage: Option<WebhookRequest>,
}

macro_rules! setters {
    ($($(#[$doc:meta])* $name:ident),* $(,)?) => {$(
        $(#[$doc])*
        pub fn $name(mut self, req: WebhookRequest) -> Self {
            self.$name = Some(req);
            self
        }
    )*};
}

impl FixturesBuilder {
    setters! {
        /// See [`Fixtures::follow`].
        follow,
        /// See [`Fixtures::text`].
        text,
        /// See [`Fixtures::file`].
        file,
        /// See [`Fixtures::blocked`].
        blocked,
        /// See [`Fixtures::unblocked`].
        unblocked,
        /// See [`Fixtures::bad_signature`].
        bad_signature,
        /// See [`Fixtures::bad_signature_valid_body`].
        bad_signature_valid_body,
        /// See [`Fixtures::missing_signature`].
        missing_signature,
        /// See [`Fixtures::signed_garbage`].
        signed_garbage,
    }

    /// See [`Fixtures::action`]: the press and the Action id inside it.
    pub fn action(mut self, req: WebhookRequest, id: ActionId) -> Self {
        self.action = Some((req, id));
        self
    }

    /// The set. Panics naming the first fixture not given.
    pub fn build(self) -> Fixtures {
        fn need<T>(v: Option<T>, name: &str) -> T {
            v.unwrap_or_else(|| panic!("contract fixture `{name}` is missing"))
        }
        let (action, action_id) = need(self.action, "action");
        Fixtures {
            follow: need(self.follow, "follow"),
            text: need(self.text, "text"),
            file: need(self.file, "file"),
            action,
            action_id,
            blocked: need(self.blocked, "blocked"),
            unblocked: need(self.unblocked, "unblocked"),
            bad_signature: need(self.bad_signature, "bad_signature"),
            bad_signature_valid_body: need(
                self.bad_signature_valid_body,
                "bad_signature_valid_body",
            ),
            missing_signature: need(self.missing_signature, "missing_signature"),
            signed_garbage: need(self.signed_garbage, "signed_garbage"),
            to: self.to,
        }
    }
}

/// One platform request the adapter made: a reply call (spending a token)
/// or a push call, with the number of message objects in it.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Call {
    /// The reply token spent, `None` for a Push.
    pub reply_token: Option<String>,
    /// Message objects in the request.
    pub objects: usize,
}

impl Call {
    /// A reply call spending `token`.
    pub fn reply(token: impl Into<String>, objects: usize) -> Self {
        Self {
            reply_token: Some(token.into()),
            objects,
        }
    }
    /// A push call.
    pub fn push(objects: usize) -> Self {
        Self {
            reply_token: None,
            objects,
        }
    }
}

/// What the adapter put on the wire since the last [`Wire::take`].
#[derive(Debug, Clone, Default)]
#[non_exhaustive]
pub struct WireLog {
    /// Action payloads (LINE postback `data`, Telegram `callback_data`, the
    /// Fake Channel's ids) in message objects, in order.
    pub action_payloads: Vec<String>,
    /// Message-sending requests, in order (Menu calls excluded).
    pub calls: Vec<Call>,
}

impl WireLog {
    /// A log.
    pub fn new(action_payloads: Vec<String>, calls: Vec<Call>) -> Self {
        Self {
            action_payloads,
            calls,
        }
    }
}

/// What the suite needs to see beyond the Channel trait.
pub trait Wire: Send + Sync {
    /// Everything sent since the last call, and forgets it.
    fn take(&self) -> WireLog;
    /// A correctly signed webhook call in which `from` presses a button
    /// carrying `payload`.
    fn press(&self, from: &ChannelAccountId, payload: &str) -> WebhookRequest;
    /// The metering the adapter declares for one request sent with
    /// `delivery` (a metered unit is one Push request to one recipient).
    fn metering(&self, delivery: &Delivery) -> Metering;
    /// Most objects one reply call may carry (LINE: 5), `None` for no limit.
    fn reply_limit(&self) -> Option<usize>;
    /// Entries of the Menu `to` currently has on the platform, `None` when
    /// it shows the locked (pre-Link) Menu.
    fn menu_entries(&self, to: &ChannelAccountId) -> Option<usize>;
    /// The Menu key of the Menu `to` currently has, as far as the
    /// platform shows it (default: [`SeenKey::Hidden`]).
    fn menu_key(&self, to: &ChannelAccountId) -> SeenKey {
        let _ = to;
        SeenKey::Hidden
    }
}

/// The Menu key a [`Wire`] sees on the platform.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum SeenKey {
    /// The platform does not show Menu keys.
    Hidden,
    /// No Menu, or one without a key.
    None,
    /// This key.
    Key(String),
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
    metering_counts_push_requests(&factory()).await;
    menu_set_and_unset(&factory()).await;
    keyed_menu_is_set(&factory()).await;
    relink_menu_reaches_the_account(&factory()).await;
    capabilities_agree(&factory()).await;
    degrade_keeps_every_action(&factory()).await;
    action_ids_round_trip(&factory()).await;
    a_foreign_resume_token_is_ignored(&factory()).await;
}

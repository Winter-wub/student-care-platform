//! The fictional "Seal Bay University" Deployment: Thai default Locale,
//! LINE + Telegram + Fake Channels, email-OTP Verification and one small
//! customer Module ([`Contact`]) adding a Menu entry. Both runtime binaries
//! (`../native`, `../worker`) call [`deployment`] with their own Services.

#![warn(missing_docs)]
#![forbid(unsafe_code)]

use harpseal::{
    ActionContext, ActionId, BuildError, CatalogEntry, Deployment, DeploymentBuilder,
    DeploymentConfig, EmailOtp, MenuEntrySpec, Message, Module, ModuleRegistrar, Services,
};
use harpseal_channel_fake::FakeChannel;
use harpseal_channel_line::{LineChannel, LineConfig};
use harpseal_channel_telegram::{TelegramChannel, TelegramConfig};

/// `deployment.toml`, compiled in.
pub const DEPLOYMENT_TOML: &str = include_str!("../deployment.toml");

/// The parsed `deployment.toml`.
pub fn config() -> DeploymentConfig {
    DeploymentConfig::from_toml(DEPLOYMENT_TOML).expect("deployment.toml is valid")
}

/// Secrets from the environment / Worker secrets, never from TOML.
#[derive(Debug, Clone, Default)]
pub struct Secrets {
    /// LINE Official Account.
    pub line: LineConfig,
    /// Telegram bot.
    pub telegram: TelegramConfig,
    /// Fake Channel webhook secret (demos and e2e).
    pub fake_secret: String,
}

/// A customer Module: a "contact Staff" Menu entry that answers with the
/// office's details (`contact.reply`, worded in `deployment.toml`).
pub struct Contact;

const CONTACT_TEXTS: &[CatalogEntry] = &[
    CatalogEntry::new("contact.menu", "th", "ติดต่อเจ้าหน้าที่"),
    CatalogEntry::new("contact.menu", "en", "Contact Staff"),
    CatalogEntry::new("contact.reply", "th", "ติดต่อกองกิจการนักศึกษา"),
    CatalogEntry::new("contact.reply", "en", "Please contact Student Affairs."),
];

impl Module for Contact {
    fn name(&self) -> &'static str {
        "contact"
    }

    fn register(&self, r: &mut ModuleRegistrar) {
        let entry = MenuEntrySpec {
            label_key: "contact.menu",
            action: ActionId::new("contact.show").expect("Action id is ≤ 64 bytes"),
            order: 90,
        };
        r.messages(CONTACT_TEXTS)
            .menu_entry(entry, |cx: ActionContext| async move {
                Ok(vec![Message::text(cx.t("contact.reply"))])
            });
    }
}

/// Everything but the Fake Channel: config, Services, LINE, Telegram,
/// email OTP and the Modules. Tests add the harness's Fake Channel.
pub fn builder(services: Services, secrets: Secrets) -> DeploymentBuilder {
    let config = config();
    let otp = EmailOtp::new(config.otp.clone());
    let http = services.http.clone();
    Deployment::builder()
        .config(config)
        .runtime(services)
        .channel(LineChannel::new(secrets.line, http.clone()))
        .channel(TelegramChannel::new(secrets.telegram, http))
        .verification(otp)
        .module(Contact)
}

/// Why [`deployment`] refused to start.
#[derive(Debug)]
#[non_exhaustive]
pub enum StartError {
    /// `FAKE_SECRET` is empty: the Fake Channel would accept unsigned calls.
    FakeSecret(harpseal::Error),
    /// The builder refused the configuration.
    Build(BuildError),
}

impl std::fmt::Display for StartError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StartError::FakeSecret(e) => write!(f, "FAKE_SECRET: {e}"),
            StartError::Build(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for StartError {}

/// The whole Deployment, with a Fake Channel keyed by `secrets.fake_secret`,
/// which must not be empty.
pub fn deployment(services: Services, secrets: Secrets) -> Result<Deployment, StartError> {
    let fake = FakeChannel::new(secrets.fake_secret.clone()).map_err(StartError::FakeSecret)?;
    builder(services, secrets)
        .channel(fake)
        .build()
        .map_err(StartError::Build)
}

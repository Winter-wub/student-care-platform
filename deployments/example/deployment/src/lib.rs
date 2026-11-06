//! The fictional "Seal Bay University" Deployment: Thai default Locale,
//! LINE + Telegram + Fake Channels, email-OTP Verification, the built-in
//! Announcements Module and one small customer Module ([`Contact`]) adding
//! a Menu entry. Announcements sends Mini App buttons, so `LINE_LIFF_ID`
//! and `TELEGRAM_WEB_APP_URL` must be set or `build()` refuses to start. Both runtime binaries
//! (`../native`, `../worker`) call [`deployment`] with their own Services.

#![warn(missing_docs)]
#![forbid(unsafe_code)]

use harpseal::modules::announcements::Announcements;
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

impl Secrets {
    /// Every variable [`Secrets::from_lookup`] reads.
    pub const VARS: [&'static str; 7] = [
        "LINE_CHANNEL_SECRET",
        "LINE_ACCESS_TOKEN",
        "LINE_LIFF_ID",
        "TELEGRAM_BOT_TOKEN",
        "TELEGRAM_WEBHOOK_SECRET",
        "TELEGRAM_WEB_APP_URL",
        "FAKE_SECRET",
    ];

    /// Reads [`Secrets::VARS`] through `get`: the process environment on
    /// the native binary, Worker secrets and vars on Workers. Missing values
    /// are empty; [`deployment`] refuses an empty `FAKE_SECRET`.
    pub fn from_lookup(get: impl Fn(&str) -> Option<String>) -> Self {
        let v = |k: &str| get(k).unwrap_or_default();
        Secrets {
            line: LineConfig::new(
                v("LINE_CHANNEL_SECRET"),
                v("LINE_ACCESS_TOKEN"),
                v("LINE_LIFF_ID"),
            ),
            telegram: TelegramConfig::new(
                v("TELEGRAM_BOT_TOKEN"),
                v("TELEGRAM_WEBHOOK_SECRET"),
                v("TELEGRAM_WEB_APP_URL"),
            ),
            fake_secret: v("FAKE_SECRET"),
        }
    }
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
    let clock = services.clock.clone();
    Deployment::builder()
        .config(config)
        .runtime(services)
        .channel(LineChannel::new(secrets.line, http.clone()))
        // The Clock checks the age of Telegram Mini App `initData`.
        .channel(TelegramChannel::new(secrets.telegram, http).with_clock(clock))
        .verification(otp)
        .module(Announcements)
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

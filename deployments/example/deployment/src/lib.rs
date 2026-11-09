//! The fictional "Seal Bay University" Deployment: Thai default Locale,
//! LINE + Telegram + Fake Channels, email-OTP Verification, the built-in
//! Announcements Module and four small customer Modules: [`Contact`] adds
//! a Menu entry, [`my_info::MyInfo`] a Mini App page linked from the Mini
//! App home, [`fees::Fees`] a back-office spreadsheet upload, and
//! [`reminders::Reminders`] a weekly fee reminder to chosen Students (chat
//! when linked, email otherwise).
//! Announcements sends Mini App buttons, so `LINE_LIFF_ID` and
//! `TELEGRAM_WEB_APP_URL` must be set or `build()` refuses to start.
//! LINE's rich menus come with bundled images ([`menu_images`]); with real
//! LINE credentials the images are checked before the Deployment starts.
//! Both runtime binaries (`../native`, `../worker`) call [`deployment`]
//! with their own Services.

#![warn(missing_docs)]
#![forbid(unsafe_code)]

pub mod fees;
pub mod menu_images;
pub mod my_info;
pub mod reminders;
pub mod seed;

use harpseal::modules::announcements::Announcements;
use harpseal::{
    ActionContext, ActionId, BuildError, CatalogEntry, Deployment, DeploymentBuilder,
    DeploymentConfig, EmailOtp, MenuEntrySpec, Message, Module, ModuleRegistrar, PublicUrl,
    Services,
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
    /// `HARPSEAL_PUBLIC_URL`: the https origin the Deployment is served at
    /// (#109), over `public_url` in `deployment.toml`; empty keeps the TOML.
    /// Not a secret, read the same way so both runtimes see it.
    pub public_url: String,
}

impl Secrets {
    /// Every variable [`Secrets::from_lookup`] reads.
    pub const VARS: [&'static str; 8] = [
        "LINE_CHANNEL_SECRET",
        "LINE_ACCESS_TOKEN",
        "LINE_LIFF_ID",
        "TELEGRAM_BOT_TOKEN",
        "TELEGRAM_WEBHOOK_SECRET",
        "TELEGRAM_WEB_APP_URL",
        "FAKE_SECRET",
        "HARPSEAL_PUBLIC_URL",
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
            )
            .with_locked_menu(menu_images::locked_menu())
            .with_menu_images(
                menu_images::LOCKED_PNG.to_vec(),
                menu_images::UNLOCKED_PNG.to_vec(),
            ),
            telegram: TelegramConfig::new(
                v("TELEGRAM_BOT_TOKEN"),
                v("TELEGRAM_WEBHOOK_SECRET"),
                v("TELEGRAM_WEB_APP_URL"),
            ),
            fake_secret: v("FAKE_SECRET"),
            public_url: v("HARPSEAL_PUBLIC_URL"),
        }
    }
}

impl Secrets {
    /// Replaces the bundled rich menu images by the files named in
    /// `LINE_MENU_IMAGE_LOCKED` / `LINE_MENU_IMAGE_UNLOCKED` (a path; unset
    /// or empty keeps the bundled image), read with `read`: the native
    /// binary passes `std::fs::read`. Workers have no files and skip this.
    pub fn with_menu_image_files(
        mut self,
        get: impl Fn(&str) -> Option<String>,
        read: impl Fn(&str) -> std::io::Result<Vec<u8>>,
    ) -> Result<Self, StartError> {
        let (locked, unlocked) = menu_images::load(get, read).map_err(StartError::MenuImages)?;
        self.line = self.line.with_menu_images(locked, unlocked);
        Ok(self)
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
/// Refuses a `HARPSEAL_PUBLIC_URL` that is set but not an https origin.
pub fn builder(services: Services, secrets: Secrets) -> Result<DeploymentBuilder, StartError> {
    let mut config = config();
    if !secrets.public_url.trim().is_empty() {
        config.public_url =
            Some(PublicUrl::new(&secrets.public_url).map_err(StartError::PublicUrl)?);
    }
    let otp = EmailOtp::new(config.otp.clone());
    let http = services.http.clone();
    let clock = services.clock.clone();
    Ok(Deployment::builder()
        .config(config)
        .runtime(services)
        .channel(LineChannel::new(secrets.line, http.clone()))
        // The Clock checks the age of Telegram Mini App `initData`.
        .channel(TelegramChannel::new(secrets.telegram, http).with_clock(clock))
        .verification(otp)
        .module(Announcements)
        .module(Contact)
        .module(my_info::MyInfo)
        .module(fees::Fees)
        .module(reminders::Reminders))
}

/// Why [`deployment`] refused to start.
#[derive(Debug)]
#[non_exhaustive]
pub enum StartError {
    /// `FAKE_SECRET` is empty: the Fake Channel would accept unsigned calls.
    FakeSecret(harpseal::Error),
    /// A LINE Channel with real credentials has no usable rich menu image,
    /// or a `LINE_MENU_IMAGE_*` file could not be read.
    MenuImages(String),
    /// The builder refused the configuration.
    Build(BuildError),
    /// `HARPSEAL_PUBLIC_URL` is set but not an https origin.
    PublicUrl(harpseal::Error),
}

impl std::fmt::Display for StartError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StartError::FakeSecret(e) => write!(f, "FAKE_SECRET: {e}"),
            StartError::MenuImages(m) => write!(f, "LINE rich menu images: {m}"),
            StartError::Build(e) => write!(f, "{e}"),
            StartError::PublicUrl(e) => write!(f, "HARPSEAL_PUBLIC_URL: {e}"),
        }
    }
}

impl std::error::Error for StartError {}

/// The whole Deployment, with a Fake Channel keyed by `secrets.fake_secret`,
/// which must not be empty.
pub fn deployment(services: Services, secrets: Secrets) -> Result<Deployment, StartError> {
    deployment_with_fake(services, secrets).map(|(d, _)| d)
}

/// [`deployment`], also returning a handle on its Fake Channel (it shares
/// the recorded state), for the native binary's dev mode.
pub fn deployment_with_fake(
    services: Services,
    secrets: Secrets,
) -> Result<(Deployment, FakeChannel), StartError> {
    deployment_with_fake_in(services, secrets, false)
}

/// [`deployment_with_fake`], built in dev mode when `dev` is set, so the
/// back office also shows the Fake Channel (#131); the native binary's
/// `HARPSEAL_DEV_ROUTES=1`.
pub fn deployment_with_fake_in(
    services: Services,
    secrets: Secrets,
    dev: bool,
) -> Result<(Deployment, FakeChannel), StartError> {
    let fake = FakeChannel::new(secrets.fake_secret.clone()).map_err(StartError::FakeSecret)?;
    // With real credentials, refuse now rather than at the first Inbound.
    secrets.line.check_menu_images().map_err(|e| {
        let why = match e {
            harpseal::Error::Invalid(m) => m,
            other => other.to_string(),
        };
        StartError::MenuImages(format!(
            "{why}; set {} and {} to PNG files of 2500 x 843 or 2500 x 1686 px (at most 1 MB), \
             or unset them to use the bundled images",
            menu_images::LOCKED_VAR,
            menu_images::UNLOCKED_VAR
        ))
    })?;
    let d = builder(services, secrets)?
        .dev(dev)
        .channel(fake.clone())
        .build()
        .map_err(StartError::Build)?;
    Ok((d, fake))
}

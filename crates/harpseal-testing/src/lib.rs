//! The test seam for harpseal Deployments (spec, "Testing Decisions").
//!
//! [`Harness::new`] makes fake [`Services`]: an in-memory SQLite `Db`
//! (rusqlite, foreign keys on, the same `Db` the native runtime uses), a
//! [`CapturingEmail`], a [`ManualClock`], a [`RecordingHttp`], a
//! deterministic [`SeqRandom`] and a [`MemoryFiles`] store, plus a
//! [`FakeChannel`]. [`Harness::start`] builds a Deployment from
//! [`Harness::builder`], runs its migrations and hands back a [`TestApp`]
//! that drives the axum Router in-process with `tower::ServiceExt::oneshot`.
//!
//! ```no_run
//! # async fn demo() {
//! let h = harpseal_testing::Harness::new();
//! let app = h.start().await;
//! assert_eq!(app.get("/notice").await.status, 200);
//! h.clock().advance_minutes(5);
//! assert!(h.emails().is_empty());
//! # }
//! ```
//!
//! Native only (rusqlite): this crate is a dev-dependency, never part of a
//! Worker build.

#![warn(missing_docs)]
#![forbid(unsafe_code)]

pub mod a11y;
mod app;
mod browser;
pub mod contract;
mod fakes;

use std::sync::Arc;

use harpseal::{
    Actor, Db, Deployment, DeploymentBuilder, DeploymentConfig, Email, EmailOtp, Locale, NewStaff,
    RequestCtx, Services,
};
use harpseal_channel_fake::{FakeChannel, FakeProfile};
use harpseal_runtime_native::SqliteDb;

pub use app::{TestApp, TestResponse};
pub use browser::{Browser, urlencode};
pub use fakes::{
    CapturingEmail, ManualClock, MemoryFiles, RecordedRequest, RecordingHttp, SeqRandom,
};

/// Secret the harness's Fake Channel expects on webhook calls.
pub const FAKE_SECRET: &str = "fake-secret";
/// Where the [`ManualClock`] starts: 2026-11-05T00:00:00Z.
pub const START_MS: i64 = 1_793_836_800_000;
/// Seed of the [`SeqRandom`].
pub const SEED: u64 = 0x05EA_1BA7;

/// The `deployment.toml` [`Harness::builder`] uses: a fictional university
/// with Thai as default Locale and one Role per Kernel Permission group.
pub const TEST_CONFIG: &str = r#"
name = "Seal Bay University (test)"
default_locale = "th"
utc_offset = "+07:00"

[roles]
Administrator = ["kernel.student.view", "kernel.student.edit", "kernel.staff.manage"]
Viewer = ["kernel.student.view"]
"#;

/// Fake Services, a Fake Channel and helpers to boot a Deployment on them.
#[derive(Clone)]
pub struct Harness {
    db: Arc<SqliteDb>,
    files: MemoryFiles,
    email: CapturingEmail,
    http: RecordingHttp,
    clock: ManualClock,
    random: SeqRandom,
    fake: FakeChannel,
}

impl Default for Harness {
    fn default() -> Self {
        Self::new()
    }
}

impl Harness {
    /// Fresh fakes: an empty in-memory database, no emails, no requests,
    /// the clock at [`START_MS`].
    pub fn new() -> Self {
        let db = SqliteDb::memory().expect("in-memory SQLite opens");
        Self {
            db: Arc::new(db),
            files: MemoryFiles::default(),
            email: CapturingEmail::default(),
            http: RecordingHttp::default(),
            clock: ManualClock::new(START_MS),
            random: SeqRandom::new(SEED),
            fake: FakeChannel::new(FAKE_SECRET).expect("FAKE_SECRET is not empty"),
        }
    }

    /// The same harness with its Fake Channel imitating `profile`
    /// ([`FakeProfile::LineLike`] for Reply/Push metering and LINE's
    /// limits, [`FakeProfile::TelegramLike`] for no reply tokens).
    pub fn with_fake_profile(mut self, profile: FakeProfile) -> Self {
        self.fake = self.fake.with_profile(profile);
        self
    }

    /// The fakes as `Services`, for `Deployment::builder().runtime(..)`.
    pub fn services(&self) -> Services {
        Services {
            db: self.db.clone(),
            files: Arc::new(self.files.clone()),
            email: Arc::new(self.email.clone()),
            http: Arc::new(self.http.clone()),
            clock: Arc::new(self.clock.clone()),
            random: Arc::new(self.random.clone()),
        }
    }

    /// [`TEST_CONFIG`], parsed.
    pub fn config() -> DeploymentConfig {
        DeploymentConfig::from_toml(TEST_CONFIG).expect("TEST_CONFIG is valid")
    }

    /// A builder with [`TEST_CONFIG`], these Services, the Fake Channel and
    /// email OTP. Add Modules (or more Channels) and build.
    pub fn builder(&self) -> DeploymentBuilder {
        Deployment::builder()
            .config(Self::config())
            .runtime(self.services())
            .channel(self.fake.clone())
            .verification(EmailOtp::new(Self::config().otp))
    }

    /// Builds [`Harness::builder`] as is and boots it.
    pub async fn start(&self) -> TestApp {
        let d = self.builder().build().expect("the test Deployment builds");
        self.boot(d).await
    }

    /// Runs the Deployment's migrations and serves its Router.
    pub async fn boot(&self, d: Deployment) -> TestApp {
        d.migrate().await.expect("migrations apply");
        TestApp::new(d, self.fake.clone())
    }

    /// The Fake Channel (shares state with the one in the builder).
    pub fn fake_channel(&self) -> FakeChannel {
        self.fake.clone()
    }
    /// The database, for setup and for reading back rows.
    pub fn db(&self) -> Arc<dyn Db> {
        self.db.clone()
    }
    /// The clock; move it with `advance_minutes` and friends.
    pub fn clock(&self) -> &ManualClock {
        &self.clock
    }
    /// The email capture.
    pub fn email(&self) -> &CapturingEmail {
        &self.email
    }
    /// Every email sent so far.
    pub fn emails(&self) -> Vec<Email> {
        self.email.sent()
    }
    /// The HTTP recorder; queue canned answers with `respond_with`.
    pub fn http(&self) -> &RecordingHttp {
        &self.http
    }
    /// Every outgoing HTTP request so far.
    pub fn http_requests(&self) -> Vec<RecordedRequest> {
        self.http.requests()
    }
    /// Creates an active Staff member holding `roles` (Role names from the
    /// TOML seed or the database); returns `staff.id`. Call after
    /// [`Harness::start`] / [`Harness::boot`] (the tables must exist).
    pub async fn create_staff(&self, email: &str, password: &str, roles: &[&str]) -> String {
        let mut s = NewStaff::new(email, "Test Staff", password, Locale::th());
        s.roles = roles.iter().map(|r| r.to_string()).collect();
        self.create_staff_with(s).await
    }

    /// Creates the Staff member `s` as the system; returns `staff.id`.
    pub async fn create_staff_with(&self, s: NewStaff) -> String {
        harpseal::create_staff(&self.services(), &Actor::System, s, &RequestCtx::default())
            .await
            .expect("staff is created")
    }

    /// The in-memory FileStore.
    pub fn files(&self) -> &MemoryFiles {
        &self.files
    }
}

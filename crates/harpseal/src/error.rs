//! One error type for every port and Channel, and the build-time errors.

use crate::delivery::{Capability, ChannelKind};
use crate::message::ActionId;

/// Every failure a Core port or Channel adapter can report.
#[derive(Debug)]
#[non_exhaustive]
pub enum Error {
    /// The Channel does not have this Channel Capability.
    Unsupported(Capability),
    /// The Channel Account blocked the bot; the Link becomes unreachable.
    Unreachable,
    /// The platform asked us to slow down.
    RateLimited {
        /// Seconds the platform asked us to wait.
        retry_after_secs: u32,
    },
    /// A webhook failed signature verification.
    BadSignature,
    /// Input broke an invariant (e.g. an Action id over 64 bytes).
    Invalid(String),
    /// The database refused the statement or the batch was rolled back.
    Db(String),
    /// Any other platform / IO failure, with the adapter's message.
    Platform(String),
    /// The change clashes with the current state (e.g. a live Link already
    /// exists, or the Link is already revoked).
    Conflict(String),
    /// A batch (or one multi-part Message) stopped part-way. The first
    /// `sent` of its `total` Messages went out and were recorded
    /// (`outbound_message` rows and `usage_daily`), as were the parts of
    /// Message `sent` that went out before the failure, if any. A retry
    /// resumes with Messages `sent..`; nothing before `sent` is sent again.
    /// The parts of Message `sent` that were already delivered may be
    /// delivered twice, on any Channel (Telegram cannot de-duplicate, and
    /// a LINE resume packs its requests differently, so the retry keys
    /// differ). That is the accepted trade-off: a repeated Card beats a
    /// lost one.
    PartialSend {
        /// Messages wholly sent and recorded.
        sent: usize,
        /// Messages in the batch.
        total: usize,
        /// Why it stopped (e.g. `RateLimited`).
        source: Box<Error>,
    },
}

/// `Result` with the Core's [`Error`].
pub type Result<T, E = Error> = core::result::Result<T, E>;

impl core::fmt::Display for Error {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Error::Unsupported(c) => write!(f, "channel capability not available: {c:?}"),
            Error::Unreachable => f.write_str("channel account is unreachable"),
            Error::RateLimited { retry_after_secs } => {
                write!(f, "rate limited, retry after {retry_after_secs} s")
            }
            Error::BadSignature => f.write_str("webhook signature rejected"),
            Error::Invalid(m) => write!(f, "invalid: {m}"),
            Error::Db(m) => write!(f, "database: {m}"),
            Error::Platform(m) => write!(f, "platform: {m}"),
            Error::Conflict(m) => write!(f, "conflict: {m}"),
            Error::PartialSend {
                sent,
                total,
                source,
            } => write!(f, "sent {sent} of {total} messages, then: {source}"),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Error::PartialSend { source, .. } => Some(source.as_ref()),
            _ => None,
        }
    }
}

impl Error {
    /// The error underneath a [`Error::PartialSend`] (itself otherwise), so
    /// callers can match `Unreachable` or `RateLimited` either way.
    pub fn root(&self) -> &Error {
        match self {
            Error::PartialSend { source, .. } => source.root(),
            e => e,
        }
    }
}

/// A part [`crate::DeploymentBuilder`] needs before it can build.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Part {
    /// `.config(..)`.
    Config,
    /// `.runtime(..)`: the runtime's [`crate::Services`].
    Runtime,
    /// `.verification(..)`.
    Verification,
    /// At least one `.channel(..)`.
    Channel,
}

/// Why `Deployment::builder()...build()` refused. Every misconfiguration
/// surfaces here, at startup, never at the first request.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum BuildError {
    /// A required part was never given.
    Missing(Part),
    /// Two Channels report the same kind.
    DuplicateChannel(ChannelKind),
    /// Two handlers registered for one Action id, or an Action prefix
    /// (`ModuleRegistrar::action_prefix`) that does not end with `.`,
    /// overlaps another prefix or covers an exact Action id.
    DuplicateAction(ActionId),
    /// Two Modules (or one twice) declared the same Permission id.
    DuplicatePermission(String),
    /// Two handlers registered for one Job kind.
    DuplicateJob(String),
    /// A [`crate::JobSpec::schedule`] outside the supported cron subset.
    InvalidSchedule {
        /// The Job kind.
        kind: String,
        /// The expression as registered.
        schedule: String,
        /// What is wrong with it.
        reason: String,
    },
    /// A Role in `deployment.toml` names a Permission no Module declared.
    UnknownPermission {
        /// The Role.
        role: String,
        /// The Permission id it names.
        permission: String,
    },
    /// A back-office route is guarded by a Permission no Module declared.
    UnguardedRoute {
        /// The Module that registered the route.
        module: &'static str,
        /// The undeclared Permission id.
        permission: &'static str,
    },
    /// A Module sends `Button::MiniApp` but a Channel has no Mini App URL.
    MiniAppUnavailable {
        /// The Module that declared the Mini App button.
        module: &'static str,
        /// The Channel without a Mini App.
        channel: ChannelKind,
    },
    /// A Menu entry's label key has no text in the default Locale, or
    /// `[texts]` in the TOML overrides a key no Module declares.
    UnknownText {
        /// The Module that declared the Menu entry, or the TOML table.
        source: String,
        /// The unknown catalog key.
        key: String,
    },
    /// A configuration value is invalid (e.g. `[branding] primary_color`
    /// set in code to something that is not a CSS hex colour).
    InvalidConfig(String),
    /// Two Modules share a `name()`, or a Module is named `kernel`.
    DuplicateModule(String),
    /// Two routes would match the same requests: the same path, or paths
    /// equal once their `{param}` names are ignored (axum would panic when
    /// mounting them). Back-office paths are compared under `/admin`; the
    /// Kernel owns `/webhook/{kind}` and the stylesheet path.
    DuplicateRoute {
        /// The Module registering the second route.
        module: &'static str,
        /// The full path, `/admin` included for back-office routes.
        path: String,
        /// The Module that registered the path first.
        first: &'static str,
    },
    /// A route path axum cannot mount: it must start with `/`, must not use
    /// the old `:param` / `*rest` syntax, and a public route must not be
    /// under `/admin`.
    InvalidRoute {
        /// The Module that registered the route.
        module: &'static str,
        /// The path as registered.
        path: String,
    },
}

impl core::fmt::Display for BuildError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            BuildError::Missing(p) => write!(f, "deployment is missing its {p:?}"),
            BuildError::DuplicateChannel(k) => write!(f, "channel `{k}` is added twice"),
            BuildError::DuplicateAction(a) => write!(f, "action `{a}` has two handlers"),
            BuildError::DuplicatePermission(p) => write!(f, "permission `{p}` declared twice"),
            BuildError::DuplicateJob(j) => write!(f, "job kind `{j}` has two handlers"),
            BuildError::InvalidSchedule {
                kind,
                schedule,
                reason,
            } => write!(
                f,
                "job kind `{kind}` has an unsupported schedule `{schedule}`: {reason}"
            ),
            BuildError::UnknownPermission { role, permission } => {
                write!(f, "role `{role}` names unknown permission `{permission}`")
            }
            BuildError::UnguardedRoute { module, permission } => write!(
                f,
                "module `{module}` guards a back-office route with unknown permission `{permission}`"
            ),
            BuildError::MiniAppUnavailable { module, channel } => write!(
                f,
                "module `{module}` sends Mini App buttons but channel `{channel}` has no Mini App URL"
            ),
            BuildError::UnknownText { source, key } => {
                write!(f, "{source} uses unknown text key `{key}`")
            }
            BuildError::InvalidConfig(m) => write!(f, "invalid configuration: {m}"),
            BuildError::DuplicateModule(m) => write!(f, "module name `{m}` is used twice"),
            BuildError::DuplicateRoute {
                module,
                path,
                first,
            } => write!(
                f,
                "module `{module}` registers route `{path}`, already registered by `{first}`"
            ),
            BuildError::InvalidRoute { module, path } => {
                write!(
                    f,
                    "module `{module}` registers route `{path}`, which cannot be mounted"
                )
            }
        }
    }
}

impl std::error::Error for BuildError {}

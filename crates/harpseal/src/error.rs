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
    /// The platform refuses Pushes until its own quota period resets (LINE:
    /// the monthly message limit is used up), with the adapter's message.
    /// Waiting seconds does not help; the Kernel's Quota never sends this
    /// (it only warns Staff): the platform does.
    QuotaExhausted(String),
    /// The acting Staff member is not active or lacks this Permission (the
    /// public write functions, e.g. `harpseal::roster::upload`, check it
    /// as the pages do).
    Forbidden {
        /// The Permission id required.
        permission: String,
    },
    /// The change clashes with the current state (e.g. a live Link already
    /// exists, or the Link is already revoked).
    Conflict(String),
    /// A batch (or one multi-part Message) stopped part-way. The first
    /// `sent` of its `total` Messages went out and were recorded
    /// (`outbound_message` rows and `usage_daily`), as were the parts of
    /// Message `sent` that went out before the failure, if any. Resume with
    /// [`crate::SendOptions::resume_after`] and the same key: the Kernel
    /// sends Messages `sent..` and hands `resume` back to the Channel.
    /// Nothing before `sent` is sent again. With a `resume` token from an
    /// adapter with retry keys (LINE) no part of Message `sent` reaches the
    /// Student twice; without one (Telegram, the Fake Channel: one part per
    /// request, no retry keys) the parts of Message `sent` already
    /// delivered go again. That is the accepted trade-off there: a repeated
    /// Card beats a lost one.
    ///
    /// The variant is `#[non_exhaustive]`: build it with
    /// [`Error::partial_send`] and match it with `..`.
    #[non_exhaustive]
    PartialSend {
        /// Messages wholly sent and recorded.
        sent: usize,
        /// Messages in the batch.
        total: usize,
        /// Why it stopped (e.g. `RateLimited`).
        source: Box<Error>,
        /// The adapter's token for resuming from the failing request
        /// ([`crate::Channel::send_batch_resumed`]), if it gave one.
        resume: Option<crate::ResumeToken>,
        /// The batch key the Kernel sent with (its own fresh one for an
        /// unkeyed send), which [`crate::SendOptions::resume_after`]
        /// restores: a resume needs the same key to find the batch's rows
        /// and, on LINE, its retry keys. `None` when not built by the
        /// Kernel.
        key: Option<String>,
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
            Error::QuotaExhausted(m) => write!(f, "platform quota exhausted: {m}"),
            Error::Conflict(m) => write!(f, "conflict: {m}"),
            Error::Forbidden { permission } => {
                write!(
                    f,
                    "forbidden: needs an active Staff member holding `{permission}`"
                )
            }
            Error::PartialSend {
                sent,
                total,
                source,
                ..
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
    /// The variant's name, without the message: what may be logged where
    /// the message must not ([`crate::ErrorReport`]).
    pub fn kind(&self) -> &'static str {
        match self {
            Error::Unsupported(_) => "unsupported",
            Error::Unreachable => "unreachable",
            Error::RateLimited { .. } => "rate_limited",
            Error::BadSignature => "bad_signature",
            Error::Invalid(_) => "invalid",
            Error::Db(_) => "db",
            Error::Platform(_) => "platform",
            Error::QuotaExhausted(_) => "quota_exhausted",
            Error::Forbidden { .. } => "forbidden",
            Error::Conflict(_) => "conflict",
            Error::PartialSend { .. } => "partial_send",
        }
    }

    /// The error underneath a [`Error::PartialSend`] (itself otherwise), so
    /// callers can match `Unreachable` or `RateLimited` either way.
    pub fn root(&self) -> &Error {
        match self {
            Error::PartialSend { source, .. } => source.root(),
            e => e,
        }
    }

    /// A [`Error::PartialSend`]: `sent` of `total` Messages went out before
    /// `source`; no resume token, no batch key.
    pub fn partial_send(sent: usize, total: usize, source: Error) -> Self {
        Error::PartialSend {
            sent,
            total,
            source: Box::new(source),
            resume: None,
            key: None,
        }
    }

    /// The batch key of a [`Error::PartialSend`] the Kernel returned.
    pub fn batch_key(&self) -> Option<&str> {
        match self {
            Error::PartialSend { key, .. } => key.as_deref(),
            _ => None,
        }
    }

    /// The resume token of a [`Error::PartialSend`], if the adapter gave one.
    pub fn resume_token(&self) -> Option<&crate::ResumeToken> {
        match self {
            Error::PartialSend { resume, .. } => resume.as_ref(),
            _ => None,
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
    /// A Locale of the catalog maps to no valid [`crate::MenuKey`]
    /// ([`crate::MenuKey::for_locale`]), or two map to the same key.
    MenuKey {
        /// The Locale tag.
        locale: String,
        /// What is wrong with its key.
        reason: String,
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
    /// A Module setting (`ModuleRegistrar::setting`) is declared wrongly:
    /// its key, its default, its Permission or its texts.
    InvalidSetting {
        /// The Module that declared it.
        module: &'static str,
        /// The key as declared.
        key: String,
        /// What is wrong with it.
        reason: String,
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
            BuildError::InvalidSetting {
                module,
                key,
                reason,
            } => write!(
                f,
                "module `{module}` declares setting `{key}` wrongly: {reason}"
            ),
            BuildError::MiniAppUnavailable { module, channel } => write!(
                f,
                "module `{module}` sends Mini App buttons but channel `{channel}` has no Mini App URL"
            ),
            BuildError::UnknownText { source, key } => {
                write!(f, "{source} uses unknown text key `{key}`")
            }
            BuildError::MenuKey { locale, reason } => {
                write!(f, "Locale `{locale}` has no Menu key of its own: {reason}")
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

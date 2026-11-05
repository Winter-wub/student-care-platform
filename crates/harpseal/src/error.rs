//! One error type for every port and Channel.

use crate::delivery::Capability;

/// Every failure a Core port or Channel adapter can report.
#[derive(Debug)]
#[non_exhaustive]
pub enum Error {
    /// The Channel does not have this Channel Capability.
    Unsupported(Capability),
    /// The Channel Account blocked the bot; the Link becomes unreachable.
    Unreachable,
    /// The platform asked us to slow down.
    RateLimited { retry_after_secs: u32 },
    /// A webhook failed signature verification.
    BadSignature,
    /// Input broke an invariant (e.g. an Action id over 64 bytes).
    Invalid(String),
    /// The database refused the statement or the batch was rolled back.
    Db(String),
    /// Any other platform / IO failure, with the adapter's message.
    Platform(String),
}

pub type Result<T, E = Error> = core::result::Result<T, E>;

impl core::fmt::Display for Error {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{self:?}")
    }
}

impl std::error::Error for Error {}

/// Why `Deployment::builder()...build()` refused. Both designs return it;
/// Design B turns the "missing piece" variants into compile errors.
#[derive(Debug)]
#[non_exhaustive]
pub enum BuildError {
    MissingConfig,
    MissingRuntime,
    MissingVerification,
    NoChannel,
    DuplicateChannel(&'static str),
    DuplicateAction(String),
    DuplicatePermission(&'static str),
    /// A Role in deployment.toml names a Permission no Module defined.
    UnknownPermission {
        role: String,
        permission: String,
    },
    /// A back-office route or Menu entry refers to an undefined Permission.
    UnguardedRoute {
        module: &'static str,
        permission: &'static str,
    },
}

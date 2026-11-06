//! [`NativeSettings`]: everything the native runtime needs, read from the
//! environment. Secrets (S3 keys, SMTP password, Resend key) never come from
//! TOML.

use std::path::PathBuf;

use harpseal::{Error, Result};

use crate::s3::S3Settings;

/// Where files go.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub enum FilesSettings {
    /// A local directory.
    Local(PathBuf),
    /// An S3-compatible bucket.
    S3(S3Settings),
}

/// How email leaves.
#[derive(Clone)]
#[non_exhaustive]
pub enum EmailSettings {
    /// SMTP to a relay (`smtp://`, `smtps://` URL, may hold a password).
    Smtp {
        /// lettre SMTP URL.
        url: String,
        /// Sender address.
        from: String,
    },
    /// The Resend HTTP API.
    Resend {
        /// API key.
        api_key: String,
        /// Verified sender address.
        from: String,
    },
}

impl core::fmt::Debug for EmailSettings {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            EmailSettings::Smtp { from, .. } => f
                .debug_struct("Smtp")
                .field("url", &"<redacted>")
                .field("from", from)
                .finish(),
            EmailSettings::Resend { from, .. } => f
                .debug_struct("Resend")
                .field("api_key", &"<redacted>")
                .field("from", from)
                .finish(),
        }
    }
}

/// Native runtime settings.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct NativeSettings {
    /// SQLite file path.
    pub sqlite_path: String,
    /// FileStore backend.
    pub files: FilesSettings,
    /// EmailSender backend.
    pub email: EmailSettings,
    /// `host:port` to listen on.
    pub addr: String,
}

/// Default sender when `HARPSEAL_EMAIL_FROM` is unset.
const DEFAULT_FROM: &str = "noreply@localhost";

impl NativeSettings {
    /// A SQLite file, a local directory and SMTP to `smtp_url`, listening on
    /// `127.0.0.1:8080`.
    pub fn new(
        sqlite_path: impl Into<String>,
        files_root: impl Into<PathBuf>,
        smtp_url: impl Into<String>,
    ) -> Self {
        Self {
            sqlite_path: sqlite_path.into(),
            files: FilesSettings::Local(files_root.into()),
            email: EmailSettings::Smtp {
                url: smtp_url.into(),
                from: DEFAULT_FROM.into(),
            },
            addr: "127.0.0.1:8080".into(),
        }
    }

    /// Reads the process environment. See [`NativeSettings::from_lookup`].
    pub fn from_env() -> Result<Self> {
        Self::from_lookup(|k| std::env::var(k).ok())
    }

    /// Reads settings through `get` (empty values count as unset):
    ///
    /// | Variable | Default | Meaning |
    /// |---|---|---|
    /// | `HARPSEAL_DB` | `harpseal.sqlite3` | SQLite file |
    /// | `HARPSEAL_ADDR` | `127.0.0.1:8080` | listen address |
    /// | `HARPSEAL_FILES` | `files` | local FileStore directory |
    /// | `HARPSEAL_S3_BUCKET` | unset | set to use S3 instead of `HARPSEAL_FILES` |
    /// | `HARPSEAL_S3_ENDPOINT` | required with a bucket | e.g. `https://<id>.r2.cloudflarestorage.com` |
    /// | `HARPSEAL_S3_REGION` | `auto` | signing region |
    /// | `AWS_ACCESS_KEY_ID`, `AWS_SECRET_ACCESS_KEY` | required with a bucket | S3 credentials |
    /// | `HARPSEAL_EMAIL` | `smtp` | `smtp` or `resend` |
    /// | `HARPSEAL_SMTP` | `smtp://localhost:25` | lettre SMTP URL |
    /// | `RESEND_API_KEY` | required with `resend` | Resend API key |
    /// | `HARPSEAL_EMAIL_FROM` | `noreply@localhost` | sender address |
    pub fn from_lookup(get: impl Fn(&str) -> Option<String>) -> Result<Self> {
        let get = |k: &str| get(k).filter(|v| !v.trim().is_empty());
        let or = |k: &str, d: &str| get(k).unwrap_or_else(|| d.to_string());
        let need = |k: &str, why: &str| {
            get(k).ok_or_else(|| Error::Invalid(format!("{k} must be set {why}")))
        };

        let files = match get("HARPSEAL_S3_BUCKET") {
            Some(bucket) => {
                let why = "when HARPSEAL_S3_BUCKET is set";
                FilesSettings::S3(S3Settings::new(
                    need("HARPSEAL_S3_ENDPOINT", why)?,
                    bucket,
                    or("HARPSEAL_S3_REGION", "auto"),
                    need("AWS_ACCESS_KEY_ID", why)?,
                    need("AWS_SECRET_ACCESS_KEY", why)?,
                ))
            }
            None => FilesSettings::Local(or("HARPSEAL_FILES", "files").into()),
        };

        let from = or("HARPSEAL_EMAIL_FROM", DEFAULT_FROM);
        let email = match or("HARPSEAL_EMAIL", "smtp").as_str() {
            "smtp" => EmailSettings::Smtp {
                url: or("HARPSEAL_SMTP", "smtp://localhost:25"),
                from,
            },
            "resend" => EmailSettings::Resend {
                api_key: need("RESEND_API_KEY", "when HARPSEAL_EMAIL=resend")?,
                from,
            },
            other => {
                return Err(Error::Invalid(format!(
                    "HARPSEAL_EMAIL is {other:?}; expected `smtp` or `resend`"
                )));
            }
        };

        Ok(Self {
            sqlite_path: or("HARPSEAL_DB", "harpseal.sqlite3"),
            files,
            email,
            addr: or("HARPSEAL_ADDR", "127.0.0.1:8080"),
        })
    }
}

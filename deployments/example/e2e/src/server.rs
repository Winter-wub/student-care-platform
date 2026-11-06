//! The process under test: a temporary directory (SQLite file, files,
//! log), a free localhost port, a generated `FAKE_SECRET`, the `harpseal`
//! CLI and `harpseal-example-native` in dev mode. Dropping [`Server`]
//! kills the process.

use std::collections::hash_map::RandomState;
use std::hash::{BuildHasher, Hasher};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};

use crate::client::R;

/// Dummy Mini App ids: the example refuses to start without them, and the
/// Fake Channel never opens them.
const LIFF_ID: &str = "0000000000-e2eDummy";
const WEB_APP_URL: &str = "https://e2e.sealbay.invalid/tg";

/// 32 hex digits from the OS-seeded hasher: a throwaway dev secret.
pub fn random_hex() -> String {
    (0..2)
        .map(|i| {
            let mut h = RandomState::new().build_hasher();
            h.write_u64(i);
            h.write_u128(
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_nanos())
                    .unwrap_or_default(),
            );
            format!("{:016x}", h.finish())
        })
        .collect()
}

/// Everything the binaries share: paths, port, secret.
pub struct Env {
    /// The temporary directory.
    pub dir: PathBuf,
    /// Where `harpseal` and `harpseal-example-native` were built.
    pub bin_dir: PathBuf,
    /// `127.0.0.1:<port>`.
    pub addr: String,
    /// `FAKE_SECRET`, also the `/dev` routes' key.
    pub secret: String,
}

impl Env {
    /// A fresh directory under the system temp dir and a free port.
    pub fn new(bin_dir: PathBuf) -> R<Self> {
        let dir = std::env::temp_dir().join(format!("harpseal-e2e-{}", random_hex()));
        std::fs::create_dir_all(&dir).map_err(|e| format!("temp dir: {e}"))?;
        let port = std::net::TcpListener::bind("127.0.0.1:0")
            .and_then(|l| l.local_addr())
            .map_err(|e| format!("free port: {e}"))?
            .port();
        Ok(Self {
            dir,
            bin_dir,
            addr: format!("127.0.0.1:{port}"),
            secret: random_hex(),
        })
    }

    /// The SQLite file.
    pub fn db(&self) -> PathBuf {
        self.dir.join("e2e.sqlite3")
    }

    /// The server's log.
    pub fn log(&self) -> PathBuf {
        self.dir.join("server.log")
    }

    fn command(&self, bin: &str) -> Command {
        let mut c = Command::new(self.bin_dir.join(bin));
        c.env("HARPSEAL_DB", self.db())
            .env("HARPSEAL_FILES", self.dir.join("files"))
            .env("HARPSEAL_ADDR", &self.addr)
            .env("HARPSEAL_EMAIL", "smtp")
            .env("HARPSEAL_SMTP", "smtp://127.0.0.1:1")
            .env("FAKE_SECRET", &self.secret)
            .env("LINE_LIFF_ID", LIFF_ID)
            .env("TELEGRAM_WEB_APP_URL", WEB_APP_URL)
            .env_remove("HARPSEAL_S3_BUCKET");
        c
    }

    /// `harpseal staff create --admin --email <email>`: returns the
    /// temporary password it prints once.
    pub fn create_admin(&self, email: &str) -> R<String> {
        let out = self
            .command("harpseal")
            .args(["staff", "create", "--admin", "--email", email])
            .output()
            .map_err(|e| format!("run harpseal: {e} (did `cargo build` run?)"))?;
        let stdout = String::from_utf8_lossy(&out.stdout);
        if !out.status.success() {
            return Err(format!(
                "harpseal staff create: {}",
                String::from_utf8_lossy(&out.stderr).trim()
            ));
        }
        temporary_password(&stdout).ok_or_else(|| format!("no temporary password in: {stdout}"))
    }

    /// Starts the server in dev mode, logging to [`Env::log`].
    pub fn start(&self) -> R<Server> {
        let log = std::fs::File::create(self.log()).map_err(|e| format!("log: {e}"))?;
        let err = log.try_clone().map_err(|e| format!("log: {e}"))?;
        let child = self
            .command("harpseal-example-native")
            .env("HARPSEAL_DEV_ROUTES", "1")
            .stdin(Stdio::null())
            .stdout(log)
            .stderr(err)
            .spawn()
            .map_err(|e| format!("start server: {e}"))?;
        Ok(Server { child })
    }
}

/// The value after the CLI's "temporary password …:" label.
pub fn temporary_password(stdout: &str) -> Option<String> {
    stdout
        .lines()
        .find(|l| l.starts_with("temporary password"))
        .and_then(|l| l.rsplit(": ").next())
        .map(|p| p.trim().to_string())
        .filter(|p| !p.is_empty())
}

/// The running server; killed on drop.
pub struct Server {
    child: Child,
}

impl Server {
    /// Whether it exited already (then the log says why).
    pub fn exited(&mut self) -> bool {
        matches!(self.child.try_wait(), Ok(Some(_)))
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// The last `n` lines of `path`, for a failure report.
pub fn tail(path: &Path, n: usize) -> String {
    let text = std::fs::read_to_string(path).unwrap_or_default();
    let lines: Vec<&str> = text.lines().collect();
    lines[lines.len().saturating_sub(n)..].join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_temporary_password_is_read_from_the_cli_output() {
        let out = "created staff admin@sealbay.example (01ab)\ncreated role Administrator\n\
                   temporary password (shown once, change it at first sign-in): a1b2c3-d4e5f6-0a0b0c-1d2e3f\n";
        assert_eq!(
            temporary_password(out).as_deref(),
            Some("a1b2c3-d4e5f6-0a0b0c-1d2e3f")
        );
        assert_eq!(temporary_password("created staff x\n"), None);
    }

    #[test]
    fn secrets_are_32_hex_digits_and_differ() {
        let (a, b) = (random_hex(), random_hex());
        assert_eq!(a.len(), 32);
        assert!(a.bytes().all(|c| c.is_ascii_hexdigit()));
        assert_ne!(a, b);
    }
}

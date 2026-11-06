//! `just e2e`: boots `harpseal-example-native` (dev mode, temporary
//! SQLite file, free port, generated `FAKE_SECRET`), creates the first
//! Administrator with the `harpseal` CLI and walks the whole first slice
//! over HTTP. Prints a pass/fail table; exits 1 on any failure.
//!
//! ```text
//! cargo build -p harpseal-example-native --bins
//! cargo run -p harpseal-example-e2e -- [BIN_DIR]   # default target/debug
//! ```
//!
//! What a Student sees comes from the example's dev-only `/dev/outbox`
//! (the Fake Channel's record) and `/dev/email` (the captured OTP email);
//! Jobs run through `/dev/tick` instead of waiting for the minute ticker.
//! Database assertions read the SQLite file directly, read-only.
//!
//! The temporary directory (SQLite file, uploaded files, `server.log`) is
//! removed when every step passes and kept on failure, with its path and the
//! log's tail printed, for debugging. The server is killed when the walk
//! ends or fails; a Ctrl-C kills the walk but may leave the server running
//! (it is a plain child process, not a process group): stop it by hand.

#![forbid(unsafe_code)]

mod chat;
mod client;
mod server;
mod table;
mod walk;

use std::future::Future;
use std::path::PathBuf;
use std::time::Instant;

use client::{Client, R};
use table::{Status, Step, all_passed, format_table};
use walk::{ADMIN, World};

/// Runs steps in order; after a failure the rest are skipped.
#[derive(Default)]
struct Runner {
    steps: Vec<Step>,
}

impl Runner {
    fn failed(&self) -> bool {
        self.steps.iter().any(|s| s.status != Status::Pass)
    }

    async fn step<T: ToString>(&mut self, name: &str, f: impl Future<Output = R<T>>) -> Option<T> {
        let mut step = Step {
            name: name.into(),
            status: Status::Skip,
            ms: 0,
            detail: String::new(),
        };
        if self.failed() {
            self.steps.push(step);
            return None;
        }
        let start = Instant::now();
        let out = f.await;
        step.ms = start.elapsed().as_millis();
        let value = match out {
            Ok(v) => {
                step.status = Status::Pass;
                step.detail = v.to_string();
                Some(v)
            }
            Err(e) => {
                step.status = Status::Fail;
                step.detail = e;
                None
            }
        };
        self.steps.push(step);
        value
    }
}

#[tokio::main]
async fn main() -> std::process::ExitCode {
    let bin_dir = std::env::args()
        .nth(1)
        .map_or_else(|| PathBuf::from("target/debug"), PathBuf::from);
    let env = match server::Env::new(bin_dir) {
        Ok(e) => e,
        Err(e) => {
            eprintln!("e2e: {e}");
            return std::process::ExitCode::FAILURE;
        }
    };
    println!("e2e: {} on http://{}", env.dir.display(), env.addr);
    let mut r = Runner::default();
    let mut password = String::new();
    let created = async {
        env.create_admin(ADMIN).map(|p| {
            password = p;
            "temporary password read from stdout".to_string()
        })
    };
    r.step("CLI staff create --admin", created).await;
    let mut server = None;
    let started = async {
        env.start().map(|s| {
            server = Some(s);
            format!("log {}", env.log().display())
        })
    };
    r.step("start server (dev mode)", started).await;
    let c = Client::new(format!("http://{}", env.addr), &env.secret);
    let ok = match c {
        Ok(c) => {
            let w = World::new(c, env.db());
            let exited = std::cell::RefCell::new(server.as_mut());
            let gone = || exited.borrow_mut().as_mut().is_none_or(|s| s.exited());
            r.step("server answers /notice", w.up(gone)).await;
            walk(&mut r, &w, &password).await;
            all_passed(&r.steps)
        }
        Err(e) => {
            eprintln!("e2e: {e}");
            false
        }
    };
    drop(server);
    println!("\n{}", format_table(&r.steps));
    if ok {
        let _ = std::fs::remove_dir_all(&env.dir);
        std::process::ExitCode::SUCCESS
    } else {
        eprintln!(
            "server log ({}):\n{}",
            env.log().display(),
            server::tail(&env.log(), 30)
        );
        std::process::ExitCode::FAILURE
    }
}

async fn walk(r: &mut Runner, w: &World, password: &str) {
    r.step("Staff login (temporary password)", w.login(password))
        .await;
    r.step(
        "change password at first sign-in",
        w.change_password(password),
    )
    .await;
    r.step("roster import CSV (full, 50 rows)", w.roster_csv())
        .await;
    r.step("roster import XLSX (upsert, unchanged)", w.roster_xlsx())
        .await;
    r.step("follow -> Consent Card, Menu locked", w.follow())
        .await;
    r.step("accept Consent", w.consent()).await;
    let code = r.step("student id -> OTP email", w.student_id()).await;
    let code = code.unwrap_or_default();
    r.step("OTP -> Link, Menu unlocked", w.otp(&code)).await;
    r.step("Staff send announcement (require ack)", w.announce())
        .await;
    r.step("tick -> Card delivered", w.delivered()).await;
    r.step("ack in chat, then again", w.ack()).await;
    r.step("Mini App shows it acknowledged", w.mini_app()).await;
    r.step("admin usage / security / audit", w.admin_pages())
        .await;
    r.step("self-unlink via Menu", w.unlink()).await;
}

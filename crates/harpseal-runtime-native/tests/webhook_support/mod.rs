//! Shared by the webhook background tests: a Module whose handlers wait
//! for the test, two linked accounts, and a real TCP server.

#![allow(dead_code)]

use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use harpseal::{
    ActionContext, ActionId, Error, Inbound, Message, Module, ModuleRegistrar, ReplyToken,
    Statement, Timestamp, WebhookRequest,
};
use harpseal_channel_fake::{FakeChannel, FakeProfile};
use harpseal_runtime_native::NativeBackground;
use harpseal_testing::{Harness, START_MS, TestApp};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::{Notify, Semaphore};

/// `gate.a` .. `gate.d` log their start, wait for a permit, log their end
/// and reply (or log `dropped` when their future is dropped first);
/// `gate.fail` fails; `gate.panic` panics.
pub struct Gate {
    log: Mutex<Vec<String>>,
    pub started: Notify,
    pub dropped: Notify,
    pub release: Semaphore,
}

impl Default for Gate {
    fn default() -> Self {
        Self {
            log: Mutex::default(),
            started: Notify::new(),
            dropped: Notify::new(),
            release: Semaphore::new(0),
        }
    }
}

impl Gate {
    pub fn log(&self) -> Vec<String> {
        self.log.lock().unwrap().clone()
    }
}

struct GateModule(Arc<Gate>);

impl Module for GateModule {
    fn name(&self) -> &'static str {
        "gate"
    }
    fn register(&self, r: &mut ModuleRegistrar) {
        for name in ["a", "b", "c", "d"] {
            let gate = self.0.clone();
            r.action(
                ActionId::new(format!("gate.{name}")).unwrap(),
                move |_: ActionContext| {
                    let gate = gate.clone();
                    async move {
                        gate.log.lock().unwrap().push(format!("start {name}"));
                        let mut running = Running(Some(gate.clone()));
                        gate.started.notify_one();
                        gate.release.acquire().await.unwrap().forget();
                        running.0 = None;
                        gate.log.lock().unwrap().push(format!("end {name}"));
                        Ok(vec![Message::text("ห้องสมุดอ่าวแมวน้ำเปิดแล้ว")])
                    }
                },
            );
        }
        r.action(
            ActionId::new("gate.fail").unwrap(),
            |_: ActionContext| async move { Err(Error::Conflict("the fictional desk is shut".into())) },
        )
        .action(
            ActionId::new("gate.panic").unwrap(),
            |_: ActionContext| async move { panic!("gate.panic handler panicked") },
        );
    }
}

/// Logs `dropped` when a gated handler's future is dropped mid-wait.
struct Running(Option<Arc<Gate>>);

impl Drop for Running {
    fn drop(&mut self) {
        if let Some(gate) = self.0.take() {
            gate.log.lock().unwrap().push("dropped".into());
            gate.dropped.notify_one();
        }
    }
}

/// `user` presses `gate.{action}`, with the platform event id `event`.
pub fn press(user: &str, action: &str, event: &str) -> Inbound {
    Inbound::action(
        FakeChannel::account(user),
        ActionId::new(format!("gate.{action}")).unwrap(),
        Some(ReplyToken {
            token: format!("r-{event}"),
            expires_at: Timestamp(START_MS + 600_000),
        }),
    )
    .with_event_id(Some(event.into()), false)
}

pub struct Setup {
    pub h: Harness,
    pub app: TestApp,
    pub gate: Arc<Gate>,
}

/// The Gate Module, with `u1` and `u2` linked to Students.
pub async fn setup() -> Setup {
    let h = Harness::new().with_fake_profile(FakeProfile::Full);
    let gate = Arc::new(Gate::default());
    let d = h
        .builder()
        .module(GateModule(gate.clone()))
        .build()
        .unwrap();
    let app = h.boot(d).await;
    for n in ["1", "2"] {
        for sql in [
            format!(
                "INSERT INTO student (id, student_code, display_name, locale, created_at, updated_at) \
                 VALUES ('s{n}', '6801000{n}', 'Nam Talay {n}', 'th', 1, 1)"
            ),
            format!(
                "INSERT INTO channel_account (id, channel, external_id, first_seen_at, updated_at) \
                 VALUES ('a{n}', 'fake', 'u{n}', 1, 1)"
            ),
            format!(
                "INSERT INTO link (id, student_id, channel_account_id, channel, status, method, linked_at, updated_at) \
                 VALUES ('l{n}', 's{n}', 'a{n}', 'fake', 'active', 'email_otp', 1, 1)"
            ),
        ] {
            h.db().execute(Statement::new(sql)).await.unwrap();
        }
    }
    Setup { h, app, gate }
}

/// Serves the app's Router on a free local port, with `bg` on every
/// request when given (as `serve` does), else without (inline handling).
pub async fn listen(app: &TestApp, bg: Option<&NativeBackground>) -> SocketAddr {
    let mut router = app.router();
    if let Some(bg) = bg {
        router = router.layer(axum::Extension(bg.background()));
    }
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(axum::serve(listener, router).into_future());
    addr
}

/// Writes `call` as `POST /webhook/fake` and returns the open connection.
pub async fn post(addr: SocketAddr, call: &WebhookRequest) -> TcpStream {
    let mut head = format!(
        "POST /webhook/fake HTTP/1.1\r\nhost: localhost\r\nconnection: close\r\ncontent-length: {}\r\n",
        call.body.len()
    );
    for (name, value) in &call.headers {
        head.push_str(&format!("{name}: {}\r\n", value.to_str().unwrap()));
    }
    head.push_str("\r\n");
    let mut s = TcpStream::connect(addr).await.unwrap();
    s.write_all(head.as_bytes()).await.unwrap();
    s.write_all(&call.body).await.unwrap();
    s
}

/// The status line of the answer to `call`, within five seconds (the
/// handlers in these tests wait for a permit, so an answer that waited
/// for the handling never comes).
pub async fn status(addr: SocketAddr, call: &WebhookRequest) -> String {
    let mut s = post(addr, call).await;
    let mut answer = Vec::new();
    tokio::time::timeout(Duration::from_secs(5), s.read_to_end(&mut answer))
        .await
        .expect("answered without waiting for the handling")
        .unwrap();
    let answer = String::from_utf8_lossy(&answer);
    answer.lines().next().unwrap_or_default().to_string()
}

pub async fn open_claims(app: &TestApp) -> i64 {
    app.count("SELECT count(*) FROM inbound_event WHERE done_at IS NULL")
        .await
}

pub async fn wait_started(gate: &Gate) {
    tokio::time::timeout(Duration::from_secs(5), gate.started.notified())
        .await
        .expect("the handler started");
}

/// No handler starts within 200 ms (a correct run never starts one, so a
/// loaded machine cannot make this fail).
pub async fn assert_none_starts(gate: &Gate) {
    let started = tokio::time::timeout(Duration::from_millis(200), gate.started.notified()).await;
    assert!(started.is_err(), "a handler started: {:?}", gate.log());
}

/// One tick at the Harness clock; returns the claims it logged as lost.
pub async fn tick_lost(s: &Setup) -> u64 {
    let now = Timestamp(s.h.clock().now_ms());
    s.app.deployment().tick(now).await.unwrap().events_lost
}

pub const OK: &str = "HTTP/1.1 200 OK";
pub const LOST: &str = "SELECT count(*) FROM security_event WHERE kind = 'inbound.lost'";

//! Webhooks answered 200 after the claim and handled on a
//! [`NativeBackground`] (#97), over a real TCP connection: handling
//! survives the client hanging up, runs one at a time per chat account,
//! records failures, and a crash mid-handling is still caught by the
//! lost-claim sweep (ADR-0009).

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

/// `gate.a` .. `gate.c` log their start, wait for a permit, log their end
/// and reply; `gate.fail` fails; `gate.panic` panics.
struct Gate {
    log: Mutex<Vec<String>>,
    started: Notify,
    release: Semaphore,
}

impl Default for Gate {
    fn default() -> Self {
        Self {
            log: Mutex::default(),
            started: Notify::new(),
            release: Semaphore::new(0),
        }
    }
}

impl Gate {
    fn log(&self) -> Vec<String> {
        self.log.lock().unwrap().clone()
    }
}

struct GateModule(Arc<Gate>);

impl Module for GateModule {
    fn name(&self) -> &'static str {
        "gate"
    }
    fn register(&self, r: &mut ModuleRegistrar) {
        for name in ["a", "b", "c"] {
            let gate = self.0.clone();
            r.action(
                ActionId::new(format!("gate.{name}")).unwrap(),
                move |_: ActionContext| {
                    let gate = gate.clone();
                    async move {
                        gate.log.lock().unwrap().push(format!("start {name}"));
                        gate.started.notify_one();
                        gate.release.acquire().await.unwrap().forget();
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

/// `user` presses `gate.{action}`, with the platform event id `event`.
fn press(user: &str, action: &str, event: &str) -> Inbound {
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

struct Setup {
    h: Harness,
    app: TestApp,
    gate: Arc<Gate>,
}

/// The Gate Module, with `u1` and `u2` linked to Students.
async fn setup() -> Setup {
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
async fn listen(app: &TestApp, bg: Option<&NativeBackground>) -> SocketAddr {
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
async fn post(addr: SocketAddr, call: &WebhookRequest) -> TcpStream {
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

/// The status line of the answer to `call`, within one second.
async fn status(addr: SocketAddr, call: &WebhookRequest) -> String {
    let mut s = post(addr, call).await;
    let mut answer = Vec::new();
    tokio::time::timeout(Duration::from_secs(1), s.read_to_end(&mut answer))
        .await
        .expect("answered without waiting for the handling")
        .unwrap();
    let answer = String::from_utf8_lossy(&answer);
    answer.lines().next().unwrap_or_default().to_string()
}

async fn open_claims(app: &TestApp) -> i64 {
    app.count("SELECT count(*) FROM inbound_event WHERE done_at IS NULL")
        .await
}

async fn wait_started(gate: &Gate) {
    tokio::time::timeout(Duration::from_secs(5), gate.started.notified())
        .await
        .expect("the handler started");
}

#[tokio::test]
async fn background_handling_finishes_after_the_client_hangs_up() {
    let Setup { h, app, gate } = setup().await;
    let bg = NativeBackground::new(4);
    let addr = listen(&app, Some(&bg)).await;
    let call = h.fake_channel().webhook(&[press("u1", "a", "evt-1")]);
    // The platform hangs up while the handler is still waiting.
    let conn = post(addr, &call).await;
    wait_started(&gate).await;
    drop(conn);
    tokio::time::sleep(Duration::from_millis(50)).await;
    gate.release.add_permits(1);
    assert!(bg.wait_idle(Duration::from_secs(5)).await);
    assert_eq!(gate.log(), ["start a", "end a"]);
    assert_eq!(open_claims(&app).await, 0, "done_at is set");
    let sent = h.fake_channel().sent_to(&FakeChannel::account("u1"));
    assert_eq!(sent.len(), 1, "the reply went out");
}

#[tokio::test]
async fn inline_handling_stops_when_the_client_hangs_up() {
    // Without a Background the handling is the request's own future, so a
    // hang-up drops it half way: the failure #97 fixes.
    let Setup { h, app, gate } = setup().await;
    let addr = listen(&app, None).await;
    let call = h.fake_channel().webhook(&[press("u1", "a", "evt-1")]);
    let conn = post(addr, &call).await;
    wait_started(&gate).await;
    drop(conn);
    tokio::time::sleep(Duration::from_millis(100)).await;
    gate.release.add_permits(1);
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(gate.log(), ["start a"]);
    assert_eq!(open_claims(&app).await, 1, "claim left unfinished");
}

#[tokio::test]
async fn background_events_of_one_account_run_one_at_a_time_in_order() {
    let Setup { h, app, gate } = setup().await;
    let bg = NativeBackground::new(4);
    let addr = listen(&app, Some(&bg)).await;
    let fake = h.fake_channel();
    let ok = "HTTP/1.1 200 OK";
    assert_eq!(
        status(addr, &fake.webhook(&[press("u1", "a", "e1")])).await,
        ok
    );
    wait_started(&gate).await;
    // u1's second event waits behind the first; u2's runs alongside.
    assert_eq!(
        status(addr, &fake.webhook(&[press("u1", "b", "e2")])).await,
        ok
    );
    assert_eq!(
        status(addr, &fake.webhook(&[press("u2", "c", "e3")])).await,
        ok
    );
    wait_started(&gate).await;
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert_eq!(gate.log(), ["start a", "start c"]);
    gate.release.add_permits(3);
    assert!(bg.wait_idle(Duration::from_secs(5)).await);
    let log = gate.log();
    let at = |e: &str| log.iter().position(|l| l == e).unwrap();
    assert!(at("end a") < at("start b"), "{log:?}");
    assert_eq!(log.len(), 6);
    assert_eq!(open_claims(&app).await, 0);
}

#[tokio::test]
async fn background_a_duplicate_and_a_bad_signature_are_answered_before_handling() {
    let Setup { h, app, gate } = setup().await;
    let bg = NativeBackground::new(4);
    let addr = listen(&app, Some(&bg)).await;
    let fake = h.fake_channel();
    let call = fake.webhook(&[press("u1", "a", "e1")]);
    assert_eq!(status(addr, &call).await, "HTTP/1.1 200 OK");
    wait_started(&gate).await;
    // The redelivery finds the claim taken: skipped, never handled twice.
    assert_eq!(status(addr, &call).await, "HTTP/1.1 200 OK");
    let mut forged = fake.webhook(&[press("u1", "b", "e2")]);
    forged.body.push(b' ');
    assert_eq!(status(addr, &forged).await, "HTTP/1.1 401 Unauthorized");
    gate.release.add_permits(2);
    assert!(bg.wait_idle(Duration::from_secs(5)).await);
    assert_eq!(gate.log(), ["start a", "end a"]);
    assert_eq!(app.count("SELECT count(*) FROM inbound_event").await, 1);
}

#[tokio::test]
async fn background_a_failure_is_logged_and_a_panic_is_left_to_the_sweep() {
    let Setup { h, app, gate } = setup().await;
    let bg = NativeBackground::new(4);
    let addr = listen(&app, Some(&bg)).await;
    let fake = h.fake_channel();
    let calls = [
        fake.webhook(&[press("u1", "fail", "e1")]),
        fake.webhook(&[press("u1", "panic", "e2")]),
        fake.webhook(&[press("u1", "a", "e3")]),
    ];
    for call in &calls {
        assert_eq!(status(addr, call).await, "HTTP/1.1 200 OK");
    }
    gate.release.add_permits(1);
    assert!(bg.wait_idle(Duration::from_secs(5)).await);
    let failed = "SELECT count(*) FROM security_event WHERE kind = 'inbound.failed'";
    assert_eq!(app.count(failed).await, 1);
    // The panic did not stall u1: its next event was still handled.
    assert_eq!(gate.log(), ["start a", "end a"]);
    // The panicked event's claim stays open, as for a killed process,
    // and the tick logs it once the lease has passed.
    assert_eq!(open_claims(&app).await, 1);
    h.clock().advance_ms(61_000);
    let tick = app.deployment().tick(Timestamp(h.clock().now_ms())).await;
    assert_eq!(tick.unwrap().events_lost, 1);
}

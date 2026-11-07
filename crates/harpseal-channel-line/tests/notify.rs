//! A Notice (#73) on the real LINE adapter: a run cut off part-way is
//! retried by the next Job run with the whole batch and the same retry
//! keys (ADR-0010: LINE drops what it has), and a used-up monthly quota
//! (also part-way through a batch) sends the Students email instead, stops
//! calling LINE, and leaves a Student without an address for later.

mod common;

use std::sync::{Arc, Mutex};

use common::{SECRET, TOKEN, USER, json};
use harpseal::{
    Card, CardList, ChannelKind, Error, Job, JobContext, JobSpec, Locale, Message, Module,
    ModuleRegistrar, Notice, NotifyDefer, NotifyOutcome, NotifyReport, NotifyTo, Statement,
    Timestamp,
};
use harpseal_channel_line::{LineChannel, LineConfig, RETRY_KEY_HEADER};
use harpseal_testing::{Harness, RecordedRequest, TestApp};
use serde_json::Value;

type Reports = Arc<Mutex<Vec<NotifyReport>>>;

struct Probe(Reports);

/// [X(1), A(61 Cards)]: push 1 = X + A1..A4, push 2 = A5 A6.
fn notice() -> Notice {
    let cards = (0..61)
        .map(|i| Card {
            title: format!("ค่าธรรมเนียม {i}"),
            body: "มหาวิทยาลัยอ่าวแมวน้ำ".into(),
            buttons: Vec::new(),
        })
        .collect();
    let chat = vec![
        Message::text("เตือนชำระค่าธรรมเนียม"),
        Message::CardList(CardList(cards)),
    ];
    Notice::new("probe", "w45")
        .locale(Locale::th(), chat, "เตือนชำระค่าธรรมเนียม", "ชำระภายในวันศุกร์")
        .budget(500)
}

impl Module for Probe {
    fn name(&self) -> &'static str {
        "probe"
    }
    fn register(&self, r: &mut ModuleRegistrar) {
        let reports = self.0.clone();
        r.job(
            JobSpec::new("probe.notify"),
            move |cx: JobContext, _: Job| {
                let reports = reports.clone();
                async move {
                    let to = [NotifyTo::code("68010001"), NotifyTo::code("68010002")];
                    let r = cx.notify(&notice(), &to).await?;
                    reports.lock().unwrap().push(r);
                    Ok::<(), Error>(())
                }
            },
        );
    }
}

async fn run(h: &Harness, sql: String) {
    h.db().execute(Statement::new(sql)).await.unwrap();
}

/// Two Students with an email; `linked` of them on LINE.
async fn boot(linked: u8) -> (Harness, TestApp, Reports) {
    let h = Harness::new();
    let line = LineChannel::new(
        LineConfig::new(SECRET, TOKEN, ""),
        Arc::new(h.http().clone()),
    );
    let reports = Reports::default();
    let d = h
        .builder()
        .channel(line)
        .module(Probe(reports.clone()))
        .build()
        .unwrap();
    let app = h.boot(d).await;
    for n in 1..=2u8 {
        run(
            &h,
            format!(
                "INSERT INTO student (id, student_code, display_name, email, locale, created_at, \
             updated_at) VALUES ('s{n}', '6801000{n}', 'Seal {n}', 's{n}@seal-bay.invalid', \
             'th', 1, 1)"
            ),
        )
        .await;
        if n > linked {
            continue;
        }
        let user = if n == 1 {
            USER.to_string()
        } else {
            format!("U{n:032}")
        };
        run(
            &h,
            format!(
                "INSERT INTO channel_account (id, channel, external_id, first_seen_at, flow_state, \
             updated_at) VALUES ('a{n}', 'line', '{user}', 1, 'linked', 1)"
            ),
        )
        .await;
        run(
            &h,
            format!(
                "INSERT INTO link (id, student_id, channel_account_id, channel, status, method, \
             linked_at, updated_at) VALUES ('l{n}', 's{n}', 'a{n}', 'line', 'active', \
             'email_otp', 1, 1)"
            ),
        )
        .await;
    }
    (h, app, reports)
}

async fn no_email(h: &Harness, n: u8) {
    run(
        h,
        format!("UPDATE student SET email = NULL WHERE id = 's{n}'"),
    )
    .await;
}

async fn enqueue(app: &TestApp) {
    app.deployment()
        .enqueue("probe.notify", serde_json::json!({}), None)
        .await
        .unwrap();
}

fn last(reports: &Reports) -> NotifyReport {
    reports.lock().unwrap().last().cloned().unwrap()
}

const USED_UP: &str = r#"{"message":"You have reached your monthly limit."}"#;

async fn tick(h: &Harness, app: &TestApp) -> harpseal::TickReport {
    let now = Timestamp(h.clock().now_ms());
    app.deployment().tick(now).await.unwrap()
}

/// `(retry key, objects)` of each Push.
fn pushes(reqs: &[RecordedRequest]) -> Vec<(String, Vec<Value>)> {
    reqs.iter()
        .map(|r| {
            let key = r.headers[RETRY_KEY_HEADER].to_str().unwrap().to_string();
            let objects = json(&r.body)["messages"].as_array().unwrap().clone();
            (key, objects)
        })
        .collect()
}

const METERED: &str = "SELECT coalesce(sum(metered), 0) FROM usage_daily WHERE channel = 'line'";

#[tokio::test]
async fn a_run_cut_off_part_way_is_retried_whole_with_the_same_retry_keys() {
    let (h, app, reports) = boot(1).await;
    h.http().respond_with(200, "{}");
    h.http().respond_with(500, r#"{"message":"boom"}"#);
    app.deployment()
        .enqueue("probe.notify", serde_json::json!({}), None)
        .await
        .unwrap();
    let t = tick(&h, &app).await;
    assert_eq!(t.retried, 1, "a partial send fails the run: {t:?}");
    let first = pushes(&h.http().take_requests());
    assert_eq!(first.len(), 2);
    assert_eq!(app.count(METERED).await, 1);

    // The Job's retry: the whole batch with the same key. LINE answers 409
    // to push 1 (it has it) and takes push 2.
    h.http()
        .respond_with(409, r#"{"message":"The retry key is already accepted"}"#);
    h.http().respond_with(200, "{}");
    h.clock().advance_minutes(1);
    let t = tick(&h, &app).await;
    assert_eq!(t.done, 1, "{:?}", t.errors);
    let again = pushes(&h.http().take_requests());
    assert_eq!(again, first, "the same requests, the same retry keys");
    let r = reports.lock().unwrap().last().cloned().unwrap();
    assert_eq!(r.students[0].outcome, NotifyOutcome::Sent);
    assert_eq!(r.students[1].outcome, NotifyOutcome::Emailed);
    assert_eq!(app.count(METERED).await, 2, "each Push request once");
    assert_eq!(h.email().to("s1@seal-bay.invalid").len(), 0);
}

#[tokio::test]
async fn a_used_up_monthly_quota_emails_instead_and_stops_calling_line() {
    let (h, app, reports) = boot(2).await;
    let used_up = r#"{"message":"You have reached your monthly limit."}"#;
    h.http().respond_with(429, used_up);
    app.deployment()
        .enqueue("probe.notify", serde_json::json!({}), None)
        .await
        .unwrap();
    let t = tick(&h, &app).await;
    assert_eq!(t.done, 1, "{:?}", t.errors);
    assert_eq!(h.http().take_requests().len(), 1, "LINE called once only");
    let r = reports.lock().unwrap().last().cloned().unwrap();
    assert_eq!((r.sent, r.emailed), (0, 2), "{r:?}");
    assert_eq!(r.exhausted, [ChannelKind("line")]);
    for n in 1..=2 {
        let to = format!("s{n}@seal-bay.invalid");
        assert_eq!(h.email().to(&to).len(), 1);
    }
    // The refused attempt still counts in usage; the Link is kept.
    let failed = "SELECT coalesce(sum(failed), 0) FROM usage_daily WHERE channel = 'line'";
    assert_eq!(app.count(failed).await, 2);
    let live = "SELECT count(*) FROM link WHERE status = 'active'";
    assert_eq!(app.count(live).await, 2);
}

#[tokio::test]
async fn a_quota_used_up_part_way_through_the_batch_still_emails() {
    let (h, app, reports) = boot(1).await;
    // Push 1 (X + A1..A4) goes; push 2 meets the used-up quota.
    h.http().respond_with(200, "{}");
    h.http().respond_with(429, USED_UP);
    enqueue(&app).await;
    let t = tick(&h, &app).await;
    assert_eq!(t.done, 1, "not retried until it gives up: {:?}", t.errors);
    let r = last(&reports);
    assert_eq!(r.students[0].outcome, NotifyOutcome::Emailed, "{r:?}");
    assert_eq!(r.exhausted, [ChannelKind("line")]);
    assert_eq!(h.email().to("s1@seal-bay.invalid").len(), 1);
    assert_eq!(h.http().take_requests().len(), 2);
}

#[tokio::test]
async fn a_quota_blocked_student_without_email_waits_and_is_sent_after_the_reset() {
    let (h, app, reports) = boot(2).await;
    no_email(&h, 2).await;
    h.http().respond_with(429, USED_UP);
    enqueue(&app).await;
    tick(&h, &app).await;
    let r = last(&reports);
    assert_eq!(r.students[0].outcome, NotifyOutcome::Emailed);
    let waits = NotifyOutcome::Deferred(NotifyDefer::Quota);
    assert_eq!(r.students[1].outcome, waits, "not Skipped(NoAddress)");
    assert_eq!(h.http().take_requests().len(), 1, "LINE called once");

    // The quota has reset: the next call with the key sends the chat.
    h.clock().advance_minutes(60);
    enqueue(&app).await;
    tick(&h, &app).await;
    let r = last(&reports);
    assert!(r.students[0].again);
    assert_eq!(r.students[1].outcome, NotifyOutcome::Sent);
    assert_eq!(h.http().take_requests().len(), 2, "s2's two Push requests");
}

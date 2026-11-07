//! A probe Module that sends a Notice (#73) from a Job (payload: module,
//! key, codes, budget) and from a public page (`/probe/notify`, the same
//! as a query string), and Seal Bay University Students to notify.

use std::sync::{Arc, Mutex};

use axum::extract::{Query, State};
use axum::routing::get;
use harpseal::{
    AppState, Job, JobContext, JobSpec, Locale, Message, Module, ModuleRegistrar, Notice,
    NotifyReport, NotifyTo, Statement, Timestamp,
};
use harpseal_channel_fake::{FakeChannel, FakeProfile};
use harpseal_testing::{Harness, TestApp};
use serde_json::{Value as Json, json};

/// The probe's Job kind.
pub const KIND: &str = "probe.notify";

/// Every report the probe got, in order.
pub type Reports = Arc<Mutex<Vec<NotifyReport>>>;

/// The probe Module.
pub struct Probe(pub Reports);

/// The Notice: two Messages in Thai (one LINE request), one in English.
pub fn notice(module: &str, key: &str) -> Notice {
    let th = vec![
        Message::text("แจ้งเตือน: ชำระค่าธรรมเนียมภายในวันศุกร์"),
        Message::text("สอบถามได้ที่กองคลัง"),
    ];
    Notice::new(module, key)
        .locale(Locale::th(), th, "แจ้งเตือนค่าธรรมเนียม", "ชำระภายในวันศุกร์")
        .locale(
            Locale::en(),
            vec![Message::text("Reminder: fees are due Friday")],
            "Fee reminder",
            "Fees are due Friday",
        )
}

/// The Notice and recipients a payload asks for.
fn request(p: &Json) -> (Notice, Vec<NotifyTo>) {
    let module = p["module"].as_str().unwrap_or("probe");
    let mut n = notice(module, p["key"].as_str().unwrap_or_default());
    if let Some(b) = p["budget"].as_u64() {
        n = n.budget(usize::try_from(b).unwrap());
    }
    let codes = p["codes"].as_str().unwrap_or_default();
    let to = codes
        .split(',')
        .filter(|c| !c.is_empty())
        .map(NotifyTo::code)
        .collect();
    (n, to)
}

impl Module for Probe {
    fn name(&self) -> &'static str {
        "probe"
    }

    fn register(&self, r: &mut ModuleRegistrar) {
        let reports = self.0.clone();
        r.job(JobSpec::new(KIND), move |cx: JobContext, job: Job| {
            let reports = reports.clone();
            async move {
                let p: Json = serde_json::from_str(&job.payload).unwrap();
                let (n, to) = request(&p);
                let report = cx.notify(&n, &to).await?;
                reports.lock().unwrap().push(report);
                Ok(())
            }
        });
        let reports = self.0.clone();
        let page = move |State(s): State<AppState>, Query(q): Query<Json>| {
            let reports = reports.clone();
            async move {
                let (n, to) = request(&q);
                let r = s.notify(&n, &to).await;
                let text = format!("{r:?}");
                if let Ok(r) = r {
                    reports.lock().unwrap().push(r);
                }
                text
            }
        };
        r.public("/probe/notify", get(page));
    }
}

/// A booted Deployment with the probe.
pub struct Setup {
    pub h: Harness,
    pub app: TestApp,
    pub reports: Reports,
}

impl Setup {
    pub async fn new(profile: FakeProfile) -> Self {
        let h = Harness::new().with_fake_profile(profile);
        let reports = Reports::default();
        let d = h.builder().module(Probe(reports.clone())).build().unwrap();
        let app = h.boot(d).await;
        Self { h, app, reports }
    }

    pub fn fake(&self) -> FakeChannel {
        self.h.fake_channel()
    }

    pub async fn run(&self, sql: &str) {
        self.h
            .db()
            .execute(Statement::new(sql.to_string()))
            .await
            .unwrap();
    }

    /// A Student `s<n>` with code `6801000<n>`, and with `link` a Link
    /// (`active`, `unreachable` or `revoked`) of Fake account `u<n>`.
    pub async fn student(&self, n: u8, email: Option<&str>, link: Option<&str>) {
        let email = email.map_or("NULL".into(), |e| format!("'{e}'"));
        self.run(&format!(
            "INSERT INTO student (id, student_code, display_name, email, locale, created_at, \
             updated_at) VALUES ('s{n}', '6801000{n}', 'Seal {n}', {email}, 'th', 1, 1)"
        ))
        .await;
        let Some(status) = link else { return };
        self.run(&format!(
            "INSERT INTO channel_account (id, channel, external_id, first_seen_at, flow_state, \
             updated_at) VALUES ('a{n}', 'fake', 'u{n}', 1, 'linked', 1)"
        ))
        .await;
        self.run(&format!(
            "INSERT INTO link (id, student_id, channel_account_id, channel, status, method, \
             linked_at, updated_at) VALUES ('l{n}', 's{n}', 'a{n}', 'fake', '{status}', \
             'email_otp', 1, 1)"
        ))
        .await;
    }

    /// Enqueues the probe's Job for `payload` and ticks once.
    pub async fn job(&self, payload: Json) -> harpseal::TickReport {
        let d = self.app.deployment();
        d.enqueue(KIND, payload, None).await.unwrap();
        self.tick().await
    }

    /// [`Setup::job`] by the probe Module with `key` to `codes`.
    pub async fn notify(&self, key: &str, codes: &[&str]) -> harpseal::TickReport {
        self.job(json!({ "key": key, "codes": codes.join(",") }))
            .await
    }

    /// Ticks at the clock's time.
    pub async fn tick(&self) -> harpseal::TickReport {
        let now = Timestamp(self.h.clock().now_ms());
        self.app.deployment().tick(now).await.unwrap()
    }

    /// The last report.
    pub fn last(&self) -> NotifyReport {
        self.reports.lock().unwrap().last().cloned().unwrap()
    }
}

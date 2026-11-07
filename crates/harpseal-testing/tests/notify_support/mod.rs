//! A probe Module whose Job notifies the Students in its payload (#73),
//! and Seal Bay University Students to notify.

use std::sync::{Arc, Mutex};

use harpseal::{
    Job, JobContext, JobSpec, Locale, Message, Module, ModuleRegistrar, Notice, NotifyReport,
    NotifyTo, Statement, Timestamp,
};
use harpseal_channel_fake::{FakeChannel, FakeProfile};
use harpseal_testing::{Harness, TestApp};
use serde_json::{Value as Json, json};

/// The probe's Job kind.
pub const KIND: &str = "probe.notify";

/// Every report the probe's Job got, in order.
pub type Reports = Arc<Mutex<Vec<NotifyReport>>>;

/// The probe Module.
pub struct Probe(pub Reports);

/// The notice: two Messages in Thai (one LINE request), one in English.
pub fn notice(key: &str) -> Notice {
    let th = vec![
        Message::text("แจ้งเตือน: ชำระค่าธรรมเนียมภายในวันศุกร์"),
        Message::text("สอบถามได้ที่กองคลัง"),
    ];
    Notice::new("probe", key)
        .locale(Locale::th(), th, "แจ้งเตือนค่าธรรมเนียม", "ชำระภายในวันศุกร์")
        .locale(
            Locale::en(),
            vec![Message::text("Reminder: fees are due Friday")],
            "Fee reminder",
            "Fees are due Friday",
        )
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
                let key = p["key"].as_str().unwrap();
                let to: Vec<NotifyTo> = p["codes"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|c| NotifyTo::code(c.as_str().unwrap()))
                    .collect();
                let report = cx.notify(&notice(key), &to).await?;
                reports.lock().unwrap().push(report);
                Ok(())
            }
        });
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

    /// Enqueues the probe's Job and ticks once.
    pub async fn notify(&self, key: &str, codes: &[&str]) -> harpseal::TickReport {
        let d = self.app.deployment();
        d.enqueue(KIND, json!({ "key": key, "codes": codes }), None)
            .await
            .unwrap();
        self.tick().await
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

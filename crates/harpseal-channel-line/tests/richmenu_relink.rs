//! The Menu re-link after a label save (#66) on recorded LINE HTTP: a
//! Deployment with the LINE Channel and linked accounts; a Staff member
//! saves a Menu label; the Kernel Job bulk-links every account to the new
//! revision (calls of at most 500), then retires the previous salted
//! revision through batch control, deleting it only once the progress
//! endpoint says `succeeded`. Revision 0 (shared by every Deployment on
//! the token with the default labels) and another Deployment's revisions
//! are never moved or deleted; batches never overlap; a failed bulk link
//! retries without deleting anything; a refused user id is skipped.

mod common;

use std::sync::Arc;

use common::{LineApi, SECRET, TOKEN, png};
use harpseal::{
    Actor, ChannelAccountId, Deployment, DeploymentConfig, Locale, RequestCtx, Services, Statement,
    Timestamp,
};
use harpseal_channel_line::{KIND, LineChannel, LineConfig};
use harpseal_testing::{Harness, RecordedRequest, SeqRandom, TestApp};

const CONFIG: &str = r#"
name = "Seal Bay University (LINE re-link)"
default_locale = "th"
utc_offset = "+07:00"

[roles]
Editor = ["kernel.setting.edit"]
"#;
const BULK: &str = "https://api.line.me/v2/bot/richmenu/bulk/link";
const PROGRESS: &str = "https://api.line.me/v2/bot/richmenu/progress/batch";
const EDITED: &str = "เลิกเชื่อมบัญชี";
const FIXED: &str = "ยกเลิกการเชื่อมบัญชี";

struct Setup {
    h: Harness,
    app: TestApp,
    api: LineApi,
    editor: Actor,
}

/// The LINE user id of account `i`.
fn user(i: usize) -> String {
    format!("U{i:032x}")
}

/// A Deployment on `api` (random seed `seed`) with accounts `from..to`
/// linked in Thai, all on the Menu shown before any label edit (revision
/// 0, the same rich menu for every Deployment with the default labels).
async fn deployment(api: &LineApi, seed: u64, from: usize, to: usize) -> Setup {
    let h = Harness::new();
    let cfg = LineConfig::new(SECRET, TOKEN, "").with_menu_images(png(2500, 843), png(2500, 843));
    let line = LineChannel::new(cfg, Arc::new(api.clone()));
    let config = DeploymentConfig::from_toml(CONFIG).unwrap();
    let services = Services {
        random: Arc::new(SeqRandom::new(seed)),
        ..h.services()
    };
    let d: Deployment = h
        .builder()
        .runtime(services)
        .config(config)
        .channel(line)
        .build()
        .unwrap();
    let app = h.boot(d).await;
    let rows = [
        "INSERT INTO student (id, student_code, display_name, locale, created_at, updated_at) \
         SELECT 'st' || i, printf('6803%05d', i), 'Seal ' || i, 'th', 1, 1 FROM n",
        "INSERT INTO channel_account (id, channel, external_id, first_seen_at, flow_state, \
         updated_at) SELECT printf('ca%05d', i), 'line', 'U' || printf('%032x', i), 1, 'linked', 1 \
         FROM n",
        "INSERT INTO link (id, student_id, channel_account_id, channel, status, method, \
         linked_at, updated_at) SELECT 'ln' || i, 'st' || i, printf('ca%05d', i), 'line', \
         'active', 'manual', 1, 1 FROM n",
    ];
    for sql in rows {
        let sql = format!(
            "WITH RECURSIVE n(i) AS (SELECT {from} UNION ALL SELECT i + 1 FROM n WHERE i < {}) {sql}",
            to - 1
        );
        h.db().execute(Statement::new(sql)).await.unwrap();
    }
    let d = app.deployment();
    let line = d.channel("line").unwrap();
    let first = ChannelAccountId::new(KIND, user(from));
    line.set_menu(&first, Some(&d.menu_for(&Locale::th())))
        .await
        .unwrap();
    let rev0 = api.linked(&user(from)).unwrap();
    for i in from + 1..to {
        api.link_user(&user(i), &rev0);
    }
    let id = h
        .create_staff("editor@sealbay.example", "kelp forest at dawn", &["Editor"])
        .await;
    Setup {
        h,
        app,
        api: api.clone(),
        editor: Actor::Staff(id),
    }
}

async fn setup(n: usize) -> Setup {
    deployment(&LineApi::default(), harpseal_testing::SEED, 0, n).await
}

impl Setup {
    async fn save(&self, value: &str) {
        let saved = harpseal::texts::set_override(
            self.app.deployment(),
            "kernel.menu.unlink",
            "th",
            value,
            &self.editor,
            &RequestCtx::default(),
        )
        .await
        .unwrap();
        assert!(saved);
    }
    async fn tick(&self) -> harpseal::TickReport {
        let now = Timestamp(self.h.clock().now_ms());
        self.app.deployment().tick(now).await.unwrap()
    }
    fn minutes(&self, n: i64) {
        self.h.clock().advance_minutes(n);
    }
    /// Ticks an hour apart until no re-link Job is queued.
    async fn drain(&self) {
        for _ in 0..20 {
            self.tick().await;
            let rows = self
                .h
                .db()
                .query(Statement::new(
                    "SELECT count(*) AS n FROM job WHERE kind = 'kernel.menu.relink' \
                     AND state = 'queued'",
                ))
                .await
                .unwrap();
            if rows.get(0, "n").and_then(|v| v.as_i64()) == Some(0) {
                return;
            }
            self.minutes(61);
        }
        panic!("re-link Jobs never drained");
    }
    fn deletes(&self) -> Vec<String> {
        self.api
            .requests()
            .iter()
            .filter(|r| r.method == http::Method::DELETE)
            .map(|r| r.uri.rsplit('/').next().unwrap_or_default().to_string())
            .collect()
    }
    /// The rich menu currently linked to account `i`, by name.
    fn shown(&self, i: usize) -> String {
        let id = self.api.linked(&user(i)).unwrap();
        self.api.created(&id).unwrap()["name"]
            .as_str()
            .unwrap()
            .to_string()
    }
}

fn bulk_calls(reqs: &[RecordedRequest]) -> Vec<(String, usize)> {
    reqs.iter()
        .filter(|r| r.uri == BULK)
        .map(|r| {
            let v = common::json(&r.body);
            let id = v["richMenuId"].as_str().unwrap_or_default().to_string();
            (id, v["userIds"].as_array().map_or(0, Vec::len))
        })
        .collect()
}

fn is_rev(name: &str, rev: u32) -> bool {
    let parts: Vec<&str> = name.split('-').collect();
    parts.len() == 6 && parts[3] == format!("r{rev}") && parts[4].len() == 8
}

#[tokio::test]
async fn a_label_save_relinks_1200_accounts_in_3_bulk_calls_then_retires_the_previous_revision() {
    let s = setup(1200).await;
    let rev0 = s.api.linked(&user(0)).unwrap();
    s.save(EDITED).await;
    s.tick().await;
    let bulk = bulk_calls(&s.api.take());
    let sizes: Vec<usize> = bulk.iter().map(|(_, n)| *n).collect();
    assert_eq!(sizes, [500, 500, 200], "3 bulk calls");
    let rev1 = bulk[0].0.clone();
    assert!(bulk.iter().all(|(id, _)| *id == rev1));
    let created = s.api.created(&rev1).unwrap();
    let name = created["name"].as_str().unwrap().to_string();
    assert!(is_rev(&name, 1), "{name}");
    assert!(created["areas"].to_string().contains(EDITED));
    for i in [0, 499, 500, 1199] {
        assert_eq!(s.api.linked(&user(i)), Some(rev1.clone()), "account {i}");
    }
    // Revision 0 has no salt: it may be another Deployment's too.
    assert!(s.api.batches().is_empty() && s.deletes().is_empty());

    s.save(FIXED).await;
    s.api.queue_phase("ongoing");
    s.minutes(1);
    s.tick().await;
    let rev2 = bulk_calls(&s.api.requests())[0].0.clone();
    assert!(is_rev(
        s.api.created(&rev2).unwrap()["name"].as_str().unwrap(),
        2
    ));
    assert_eq!(s.api.batches().len(), 1);
    assert_eq!(s.api.batches()[0].1, [(rev1.clone(), rev2.clone())]);
    // The first poll waits 5 minutes, and "ongoing" deletes nothing.
    s.minutes(4);
    s.tick().await;
    assert!(!s.api.requests().iter().any(|r| r.uri.starts_with(PROGRESS)));
    s.minutes(1);
    s.tick().await;
    assert!(s.deletes().is_empty(), "still ongoing");
    s.minutes(10);
    s.tick().await;
    assert_eq!(s.deletes(), [rev1], "only the previous salted revision");
    assert!(s.api.live_names().len() == 2, "revision 0 and 2 stay");
    assert!(s.api.id_of(&s.shown(0)).is_some());
    assert_eq!(s.api.linked(&user(0)), Some(rev2));
    assert_ne!(s.api.linked(&user(0)), Some(rev0));
}

#[tokio::test]
async fn a_failed_bulk_link_retries_and_never_deletes_early() {
    let s = setup(1200).await;
    s.save(EDITED).await;
    s.drain().await;
    let rev1 = s.api.linked(&user(1199)).unwrap();
    s.api.take();
    s.save(FIXED).await;
    // list, create, upload, bulk 1: fine; bulk 2: LINE fails.
    for _ in 0..4 {
        s.api.http().respond_with(200, "{}");
    }
    s.api
        .http()
        .respond_with(500, r#"{"message":"An error occurred in the backend"}"#);
    let r = s.tick().await;
    assert_eq!(r.retried, 1, "{r:?}");
    let reqs = s.api.take();
    assert_eq!(bulk_calls(&reqs).len(), 2);
    assert!(s.api.batches().is_empty(), "no retire yet");
    assert!(s.deletes().is_empty());
    assert_eq!(
        s.api.linked(&user(1199)),
        Some(rev1.clone()),
        "not re-linked yet"
    );

    s.minutes(1);
    s.tick().await;
    let sizes: Vec<usize> = bulk_calls(&s.api.requests())
        .iter()
        .map(|(_, n)| *n)
        .collect();
    assert_eq!(sizes, [500, 500, 200], "the retry re-links every account");
    assert_eq!(s.api.batches().len(), 1);
    s.drain().await;
    assert_eq!(s.deletes(), [rev1]);
}

#[tokio::test]
async fn two_deployments_on_one_token_never_touch_each_others_menus() {
    let api = LineApi::default();
    let prod = deployment(&api, 0x5EA1, 0, 3).await;
    let staging = deployment(&api, 0xBA7, 100, 102).await;
    let rev0 = api.linked(&user(0)).unwrap();
    assert_eq!(
        api.linked(&user(100)),
        Some(rev0.clone()),
        "one shared rich menu"
    );
    // Staging saves the very label prod will save.
    staging.save(EDITED).await;
    staging.drain().await;
    let staging_r1 = api.linked(&user(100)).unwrap();

    prod.save(EDITED).await;
    prod.drain().await;
    let prod_r1 = api.linked(&user(0)).unwrap();
    assert_ne!(
        prod_r1, staging_r1,
        "same labels, other salt: other rich menus"
    );
    prod.save(FIXED).await;
    prod.drain().await;

    let touched: Vec<String> = api
        .batches()
        .iter()
        .flat_map(|(_, ops)| ops.iter().map(|(from, _)| from.clone()))
        .chain(prod.deletes())
        .collect();
    assert_eq!(touched, [prod_r1.clone(), prod_r1], "batch from + DELETE");
    assert!(api.id_of(&staging.shown(100)).is_some());
    assert_eq!(api.linked(&user(100)), Some(staging_r1.clone()));
    assert_eq!(api.linked(&user(101)), Some(staging_r1));
    let live = api.live_names();
    assert!(
        live.iter().any(|n| api.id_of(n) == Some(rev0.clone())),
        "revision 0 stays: {live:?}"
    );
}

#[tokio::test]
async fn a_second_batch_waits_for_the_first_to_finish() {
    let s = setup(3).await;
    s.save(EDITED).await;
    s.drain().await;
    let rev1 = s.api.linked(&user(0)).unwrap();
    // Save 2: batch A (rev1 -> rev2) stays ongoing for a while.
    s.save(FIXED).await;
    for _ in 0..3 {
        s.api.queue_phase("ongoing");
    }
    s.minutes(1);
    s.tick().await;
    let rev2 = s.api.linked(&user(0)).unwrap();
    assert_eq!(s.api.batches().len(), 1);
    // Save 3 (a typo fix) while A is ongoing: run B re-links to rev3 and
    // takes A over; it sends nothing new while A is ongoing.
    s.save("ยกเลิกการเชื่อมต่อบัญชี").await;
    s.minutes(1);
    s.tick().await;
    let rev3 = s.api.linked(&user(0)).unwrap();
    assert_ne!(rev3, rev2);
    assert_eq!(s.api.batches().len(), 1, "A ongoing: B sends no batch");
    for _ in 0..2 {
        s.minutes(61);
        s.tick().await;
        assert_eq!(s.api.batches().len(), 1, "A still ongoing");
        assert!(s.deletes().is_empty());
    }
    s.drain().await;
    let batches = s.api.batches();
    assert_eq!(batches.len(), 2, "{batches:?}");
    assert_eq!(batches[0].1, [(rev1.clone(), rev2.clone())]);
    assert_eq!(batches[1].1, [(rev2.clone(), rev3.clone())]);
    assert_eq!(s.deletes(), [rev1, rev2]);
    // The second batch went out only after A's progress said succeeded.
    let reqs = s.api.requests();
    let polls = reqs.iter().filter(|r| r.uri.starts_with(PROGRESS)).count();
    assert_eq!(polls, 5, "4 for A (3 ongoing, then succeeded), 1 for B");
}

#[tokio::test]
async fn a_refused_or_malformed_user_id_is_skipped_and_the_rest_relinked() {
    let s = setup(4).await;
    let sql = "UPDATE channel_account SET external_id = 'not-a-line-id' WHERE id = 'ca00001'";
    s.h.db().execute(Statement::new(sql)).await.unwrap();
    s.api.refuse_user(&user(2));
    s.save(EDITED).await;
    s.drain().await;
    let rev1 = s.api.linked(&user(0)).unwrap();
    assert!(is_rev(&s.shown(0), 1));
    assert_eq!(s.api.linked(&user(3)), Some(rev1));
    let bulk = bulk_calls(&s.api.requests());
    let sizes: Vec<usize> = bulk.iter().map(|(_, n)| *n).collect();
    assert_eq!(
        sizes,
        [3, 2],
        "the malformed id never sent; the refused one dropped"
    );
    let live =
        s.h.db()
            .query(Statement::new(
                "SELECT count(*) AS n FROM menu_revision WHERE revision = 1 AND state = 'live'",
            ))
            .await
            .unwrap();
    assert_eq!(live.get(0, "n").and_then(|v| v.as_i64()), Some(1));
}

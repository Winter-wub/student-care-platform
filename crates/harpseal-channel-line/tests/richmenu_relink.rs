//! The Menu re-link after a label save (#66) on recorded LINE HTTP: a
//! Deployment with the LINE Channel and 1200 linked accounts; a Staff
//! member saves a Menu label; the Kernel Job bulk-links every account to
//! the new revision (3 calls of at most 500), then retires the previous
//! revision through batch control, deleting it only once the progress
//! endpoint says `succeeded`. Rich menus of another revision or another
//! Deployment are never deleted, and a failed bulk link retries without
//! deleting anything.

mod common;

use std::sync::Arc;

use common::{LineApi, SECRET, TOKEN, png};
use harpseal::{
    Actor, ChannelAccountId, DeploymentConfig, Locale, RequestCtx, Statement, Timestamp,
};
use harpseal_channel_line::{KIND, LineChannel, LineConfig};
use harpseal_testing::{Harness, RecordedRequest, TestApp};
use serde_json::json;

const CONFIG: &str = r#"
name = "Seal Bay University (LINE re-link)"
default_locale = "th"
utc_offset = "+07:00"

[roles]
Editor = ["kernel.setting.edit"]
"#;
const BULK: &str = "https://api.line.me/v2/bot/richmenu/bulk/link";
const BATCH: &str = "https://api.line.me/v2/bot/richmenu/batch";
const HASH_A: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const EDITED: &str = "เลิกเชื่อมบัญชี";

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

/// A Deployment on LINE with `n` linked Thai accounts, all on the Menu
/// shown before any label edit (revision 0), plus rich menus of another
/// Deployment on the same channel: same key and generation, other hash,
/// at revision 0 and at revision 1.
async fn setup(n: usize) -> Setup {
    let h = Harness::new();
    let api = LineApi::default();
    let cfg = LineConfig::new(SECRET, TOKEN, "").with_menu_images(png(2500, 843), png(2500, 843));
    let line = LineChannel::new(cfg, Arc::new(api.clone()));
    let config = DeploymentConfig::from_toml(CONFIG).unwrap();
    let d = h.builder().config(config).channel(line).build().unwrap();
    let app = h.boot(d).await;
    let rows = [
        "INSERT INTO student (id, student_code, display_name, locale, created_at, updated_at) \
         SELECT 'st' || i, printf('6803%04d', i), 'Seal ' || i, 'th', 1, 1 FROM n",
        "INSERT INTO channel_account (id, channel, external_id, first_seen_at, flow_state, \
         updated_at) SELECT printf('ca%05d', i), 'line', 'U' || printf('%032x', i), 1, 'linked', 1 \
         FROM n",
        "INSERT INTO link (id, student_id, channel_account_id, channel, status, method, \
         linked_at, updated_at) SELECT 'ln' || i, 'st' || i, printf('ca%05d', i), 'line', \
         'active', 'manual', 1, 1 FROM n",
    ];
    for sql in rows {
        let sql = format!(
            "WITH RECURSIVE n(i) AS (SELECT 0 UNION ALL SELECT i + 1 FROM n WHERE i < {}) {sql}",
            n - 1
        );
        h.db().execute(Statement::new(sql)).await.unwrap();
    }
    // Revision 0 on LINE, as `set_menu` left it for the first account.
    let d = app.deployment();
    let th = Locale::th();
    let line = d.channel("line").unwrap();
    let first = ChannelAccountId::new(KIND, user(0));
    line.set_menu(&first, Some(&d.menu_for(&th))).await.unwrap();
    let rev0 = api.linked(&user(0)).unwrap();
    for i in 1..n {
        api.link_user(&user(i), &rev0);
    }
    for (id, name) in [
        ("richmenu-other0", format!("harpseal-th-1-{HASH_A}")),
        ("richmenu-other1", format!("harpseal-th-1-r1-{HASH_A}")),
    ] {
        api.preexisting(
            id,
            json!({ "name": name, "size": { "width": 2500, "height": 843 } }),
        );
    }
    let id = h
        .create_staff("editor@sealbay.example", "kelp forest at dawn", &["Editor"])
        .await;
    Setup {
        h,
        app,
        api,
        editor: Actor::Staff(id),
    }
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
    fn minute(&self) {
        self.h.clock().advance_minutes(1);
    }
    fn deletes(&self) -> Vec<String> {
        self.api
            .requests()
            .iter()
            .filter(|r| r.method == http::Method::DELETE)
            .map(|r| r.uri.rsplit('/').next().unwrap_or_default().to_string())
            .collect()
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

#[tokio::test]
async fn a_label_save_relinks_1200_accounts_in_3_bulk_calls_then_retires_the_previous_revision() {
    let s = setup(1200).await;
    let rev0 = s.api.linked(&user(0)).unwrap();
    s.save(EDITED).await;
    s.api.queue_phase("ongoing");
    s.tick().await;

    let reqs = s.api.requests();
    let bulk = bulk_calls(&reqs);
    let sizes: Vec<usize> = bulk.iter().map(|(_, n)| *n).collect();
    assert_eq!(sizes, [500, 500, 200], "3 bulk calls");
    let rev1 = bulk[0].0.clone();
    assert!(bulk.iter().all(|(id, _)| *id == rev1));
    let created = s.api.created(&rev1).unwrap();
    let name = created["name"].as_str().unwrap();
    assert!(name.starts_with("harpseal-th-1-r1-"), "{name}");
    let labels = created["areas"].to_string();
    assert!(labels.contains(EDITED), "the new label: {labels}");
    for i in [0, 499, 500, 1199] {
        assert_eq!(s.api.linked(&user(i)), Some(rev1.clone()), "account {i}");
    }
    // The batch moves whoever is still on revision 0; nothing is deleted
    // while LINE reports it ongoing.
    let batches = s.api.batches();
    assert_eq!(batches.len(), 1);
    assert_eq!(batches[0].1, [(rev0.clone(), rev1.clone())]);
    let batch = reqs.iter().find(|r| r.uri == BATCH).unwrap();
    let key = common::json(&batch.body)["resumeRequestKey"].clone();
    assert!(key.as_str().is_some_and(|k| k.starts_with("harpseal-")));
    assert!(s.deletes().is_empty());
    s.minute();
    s.tick().await;
    assert!(s.deletes().is_empty(), "still ongoing");
    s.minute();
    s.tick().await;

    assert_eq!(s.deletes(), [rev0], "only the previous revision");
    let mut live = s.api.live_names();
    live.sort();
    assert_eq!(live.len(), 3, "{live:?}");
    assert!(live.contains(&format!("harpseal-th-1-{HASH_A}")));
    assert!(live.contains(&format!("harpseal-th-1-r1-{HASH_A}")));
    assert!(live.contains(&name.to_string()));
    s.minute();
    assert_eq!(s.tick().await.claimed, 0, "nothing left to do");
}

#[tokio::test]
async fn a_failed_bulk_link_retries_and_never_deletes_early() {
    let s = setup(1200).await;
    let rev0 = s.api.linked(&user(0)).unwrap();
    s.save(EDITED).await;
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
    assert!(reqs.iter().all(|r| r.uri != BATCH), "no retire yet");
    assert!(s.deletes().is_empty());
    assert_eq!(
        s.api.linked(&user(1199)),
        Some(rev0.clone()),
        "not re-linked yet"
    );

    s.minute();
    s.tick().await;
    let reqs = s.api.requests();
    let sizes: Vec<usize> = bulk_calls(&reqs).iter().map(|(_, n)| *n).collect();
    assert_eq!(sizes, [500, 500, 200], "the retry re-links every account");
    assert_eq!(s.api.batches().len(), 1);
    s.minute();
    s.tick().await;
    assert_eq!(s.deletes(), [rev0]);
}

#[tokio::test]
async fn a_second_save_retires_both_older_revisions_and_keeps_the_others() {
    let s = setup(3).await;
    let rev0 = s.api.linked(&user(0)).unwrap();
    s.save(EDITED).await;
    s.save("ยกเลิกการเชื่อมบัญชี").await;
    for _ in 0..4 {
        s.tick().await;
        s.minute();
    }
    let bulk = bulk_calls(&s.api.requests());
    assert_eq!(bulk.len(), 1, "revision 1 was never linked: {bulk:?}");
    let rev2 = bulk[0].0.clone();
    let name = s.api.created(&rev2).unwrap()["name"].clone();
    assert!(name.as_str().unwrap().starts_with("harpseal-th-1-r2-"));
    assert_eq!(s.deletes(), [rev0], "revision 1 has no rich menu");
    assert!(
        s.api
            .live_names()
            .contains(&format!("harpseal-th-1-r1-{HASH_A}")),
        "another Deployment's revision 1 stays"
    );
}

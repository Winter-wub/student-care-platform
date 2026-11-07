//! A Deployment on the recorded LINE API for the re-link tests (#66).

#![allow(dead_code)] // each test binary uses part of this

use std::sync::Arc;

use harpseal::{
    Actor, ChannelAccountId, Deployment, DeploymentConfig, Locale, RequestCtx, Services, Statement,
    Timestamp,
};
use harpseal_channel_line::{KIND, LineChannel, LineConfig};
use harpseal_testing::{Harness, RecordedRequest, SeqRandom, TestApp};

use super::{LineApi, SECRET, TOKEN, png};

pub const CONFIG: &str = r#"
name = "Seal Bay University (LINE re-link)"
default_locale = "th"
utc_offset = "+07:00"

[roles]
Editor = ["kernel.setting.edit"]
"#;
pub const BULK: &str = "https://api.line.me/v2/bot/richmenu/bulk/link";
pub const PROGRESS: &str = "https://api.line.me/v2/bot/richmenu/progress/batch";
pub const EDITED: &str = "เลิกเชื่อมบัญชี";
pub const FIXED: &str = "ยกเลิกการเชื่อมบัญชี";

pub struct Setup {
    pub h: Harness,
    pub app: TestApp,
    pub api: LineApi,
    pub editor: Actor,
}

/// The LINE user id of account `i`.
pub fn user(i: usize) -> String {
    format!("U{i:032x}")
}

/// A Deployment on `api` (random seed `seed`) with accounts `from..to`
/// linked in Thai, all on the Menu shown before any label edit (revision
/// 0, the same rich menu for every Deployment with the default labels).
pub async fn deployment(api: &LineApi, seed: u64, from: usize, to: usize) -> Setup {
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

pub async fn setup(n: usize) -> Setup {
    deployment(&LineApi::default(), harpseal_testing::SEED, 0, n).await
}

impl Setup {
    pub async fn save(&self, value: &str) {
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
    pub async fn tick(&self) -> harpseal::TickReport {
        let now = Timestamp(self.h.clock().now_ms());
        self.app.deployment().tick(now).await.unwrap()
    }
    pub fn minutes(&self, n: i64) {
        self.h.clock().advance_minutes(n);
    }
    /// Ticks an hour apart until no re-link Job is queued.
    pub async fn drain(&self) {
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
    pub fn deletes(&self) -> Vec<String> {
        self.api
            .requests()
            .iter()
            .filter(|r| r.method == http::Method::DELETE)
            .map(|r| r.uri.rsplit('/').next().unwrap_or_default().to_string())
            .collect()
    }
    /// The rich menu currently linked to account `i`, by name.
    pub fn shown(&self, i: usize) -> String {
        let id = self.api.linked(&user(i)).unwrap();
        self.api.created(&id).unwrap()["name"]
            .as_str()
            .unwrap()
            .to_string()
    }
}

pub fn bulk_calls(reqs: &[RecordedRequest]) -> Vec<(String, usize)> {
    reqs.iter()
        .filter(|r| r.uri == BULK)
        .map(|r| {
            let v = super::json(&r.body);
            let id = v["richMenuId"].as_str().unwrap_or_default().to_string();
            (id, v["userIds"].as_array().map_or(0, Vec::len))
        })
        .collect()
}

pub fn is_rev(name: &str, rev: u32) -> bool {
    let parts: Vec<&str> = name.split('-').collect();
    parts.len() == 6 && parts[3] == format!("r{rev}") && parts[4].len() == 8
}

pub fn per_user_links(reqs: &[RecordedRequest]) -> usize {
    reqs.iter()
        .filter(|r| {
            r.method == http::Method::POST && r.uri.starts_with("https://api.line.me/v2/bot/user/")
        })
        .count()
}

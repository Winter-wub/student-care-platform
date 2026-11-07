//! Setup for the backup tests (#53): a Deployment with real data made
//! through the public paths (Staff, a roster import whose file stays in the
//! FileStore, a Manual Link, a sent announcement, their audit rows).

#![allow(dead_code)]

pub mod nightly;

use std::collections::BTreeMap;

use harpseal::modules::announcements::{self, Announcements, NewAnnouncement};
use harpseal::roster::{self, Confirmed, Uploaded};
use harpseal::{
    Actor, Db, Deployment, DeploymentConfig, Inbound, RequestCtx, Rows, Statement, Timestamp,
    Value, links,
};
use harpseal_channel_fake::FakeChannel;
use harpseal_testing::{Harness, TestApp};

pub const PASSWORD: &str = "kelp forest at dawn";
pub const OFFICER: &str = "officer@sealbay.example";

const CONFIG: &str = r#"
name = "Seal Bay University (backup)"
default_locale = "th"
utc_offset = "+07:00"

[roles]
Officer = ["kernel.student.view", "kernel.link.manage", "kernel.roster.view",
           "kernel.roster.import", "announcements.view", "announcements.send"]

[roster]
student_code = "รหัสนักศึกษา"
display_name = "ชื่อ-สกุล"
email = "อีเมล"

[roster.attributes.year]
column = "ชั้นปี"
type = "integer"
"#;

/// The roster file uploaded (kept in the FileStore).
pub const ROSTER: &str = "รหัสนักศึกษา,ชื่อ-สกุล,อีเมล,ชั้นปี\r\n\
    68010001,นกน้อย ทะเลใส,s68010001@student.sealbay.example,1\r\n\
    68010002,แมวน้ำ อ่าวซีล,s68010002@student.sealbay.example,2\r\n\
    68010003,ปลาดาว หาดทราย,s68010003@student.sealbay.example,3\r\n";

/// The test Deployment on `services` (wrapping `h`'s), not migrated.
pub fn deployment_with(h: &Harness, services: harpseal::Services) -> Deployment {
    let config = DeploymentConfig::from_toml(CONFIG).unwrap();
    h.builder()
        .config(config)
        .runtime(services)
        .module(Announcements)
        .build()
        .unwrap()
}

/// The test Deployment on `h`'s Services, not migrated.
pub fn deployment(h: &Harness) -> Deployment {
    let config = DeploymentConfig::from_toml(CONFIG).unwrap();
    h.builder()
        .config(config)
        .module(Announcements)
        .build()
        .unwrap()
}

/// A Deployment full of data, and the Officer who made it.
pub async fn populated() -> (Harness, TestApp) {
    populated_on(Harness::new()).await
}

/// [`populated`] on `h`, whose database may already hold migrations.
pub async fn populated_on(h: Harness) -> (Harness, TestApp) {
    let app = h.boot(deployment(&h)).await;
    let d = app.deployment();
    let ctx = RequestCtx::cli();
    let officer = Actor::Staff(h.create_staff(OFFICER, PASSWORD, &["Officer"]).await);

    let up = roster::upload(
        d,
        "roster.csv",
        ROSTER.as_bytes().to_vec(),
        "upsert",
        &officer,
        &ctx,
    )
    .await
    .unwrap();
    let Uploaded::Previewed(id) = up else {
        panic!("previewed: {up:?}")
    };
    assert_eq!(
        roster::confirm(d, &id, &officer, &ctx).await.unwrap(),
        Confirmed::Queued
    );
    run_jobs(&h, &app).await;
    assert_eq!(app.count("SELECT count(*) FROM student").await, 3);

    let who = FakeChannel::account("u-backup");
    let r = app.fake_webhook([Inbound::follow(who, None)]).await;
    assert_eq!(r.status, 200, "{}", r.body);
    let student = text(&h, "SELECT id FROM student WHERE student_code = '68010001'").await;
    links::manual_link(d, &student, "fake", "u-backup", &officer, &ctx)
        .await
        .unwrap();

    let new = NewAnnouncement::new("ปิดอาคารเรียน 2", "อาคารเรียน 2 ปิดซ่อมวันศุกร์นี้");
    let a = announcements::create_draft(d, &new, &officer, &ctx)
        .await
        .unwrap();
    announcements::confirm(d, &a, 0, &officer, &ctx)
        .await
        .unwrap();
    run_jobs(&h, &app).await;
    for (table, at_least) in [
        ("link", 1),
        ("announcement", 1),
        ("announcement_delivery", 1),
        ("roster_import", 1),
        ("audit_log", 5),
    ] {
        let n = count(h.db().as_ref(), table).await;
        assert!(n >= at_least, "{table}: {n}");
    }
    (h, app)
}

/// Ticks (a minute apart) until no Job is due.
pub async fn run_jobs(h: &Harness, app: &TestApp) {
    for _ in 0..50 {
        let report = app
            .deployment()
            .tick(Timestamp(h.clock().now_ms()))
            .await
            .unwrap();
        h.clock().advance_minutes(1);
        if report.claimed == 0 {
            break;
        }
    }
}

/// Runs `sql` on `db`.
pub async fn query(db: &dyn Db, sql: &str) -> Rows {
    db.query(Statement::new(sql.to_string())).await.unwrap()
}

/// The first column of the first row of `sql`, as text.
pub async fn text(h: &Harness, sql: &str) -> String {
    let rows = query(h.db().as_ref(), sql).await;
    rows.rows
        .first()
        .and_then(|r| r.first())
        .and_then(Value::as_text)
        .unwrap_or_default()
        .to_string()
}

/// Rows in `table`.
pub async fn count(db: &dyn Db, table: &str) -> i64 {
    let rows = query(db, &format!("SELECT count(*) FROM \"{table}\"")).await;
    rows.rows[0][0].as_i64().unwrap()
}

/// Rows per user table (not `migration`), by name.
pub async fn counts(db: &dyn Db) -> BTreeMap<String, i64> {
    let names = query(
        db,
        "SELECT name FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%' \
         AND name <> 'migration'",
    )
    .await;
    let mut out = BTreeMap::new();
    for r in &names.rows {
        let name = r[0].as_text().unwrap().to_string();
        let n = count(db, &name).await;
        out.insert(name, n);
    }
    out
}

/// Every trigger as `(name, sql)`, by name.
pub async fn triggers(db: &dyn Db) -> Vec<(String, String)> {
    let rows = query(
        db,
        "SELECT name, sql FROM sqlite_master WHERE type = 'trigger' ORDER BY name",
    )
    .await;
    rows.rows
        .iter()
        .map(|r| {
            (
                r[0].as_text().unwrap().to_string(),
                r[1].as_text().unwrap().to_string(),
            )
        })
        .collect()
}

/// One row of `sql` as `column -> value`.
pub async fn row(db: &dyn Db, sql: &str) -> BTreeMap<String, Value> {
    let rows = query(db, sql).await;
    assert_eq!(rows.len(), 1, "{sql}");
    rows.columns
        .iter()
        .cloned()
        .zip(rows.rows[0].iter().cloned())
        .collect()
}

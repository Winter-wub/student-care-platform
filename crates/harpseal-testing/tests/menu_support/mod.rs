//! Shared by the Menu re-link seam tests (#66): the Texts form, linked
//! accounts inserted by SQL, Job draining and the Fake's Menu per account.

#![allow(dead_code)] // each test binary uses part of this

use harpseal::{ChannelAccountId, Statement, Timestamp};
use harpseal_channel_fake::FakeChannel;
use harpseal_testing::{Harness, TestApp, urlencode};

pub const UNLINK: &str = "/admin/texts/kernel.menu.unlink";

pub fn save(locale: &str, value: &str) -> String {
    format!(
        "locale={locale}&value={}&version=-&op=save",
        urlencode(value)
    )
}

/// `n` more linked accounts of Students in `locale` (`u00`, `u01`, ...).
pub async fn linked(h: &Harness, n: usize, locale: &str) -> Vec<ChannelAccountId> {
    let mut out = Vec::new();
    for i in 0..n {
        let id = format!("{locale}{i:02}");
        let sql = [
            format!(
                "INSERT INTO student (id, student_code, display_name, locale, status, created_at, \
                 updated_at) VALUES ('st-{id}', '6802{i:04}{locale}', 'Seal {id}', '{locale}', \
                 'active', 1, 1)"
            ),
            format!(
                "INSERT INTO channel_account (id, channel, external_id, first_seen_at, flow_state, \
                 updated_at) VALUES ('ca-{id}', 'fake', 'u-{id}', 1, 'linked', 1)"
            ),
            format!(
                "INSERT INTO link (id, student_id, channel_account_id, channel, status, method, \
                 linked_at, updated_at) VALUES ('ln-{id}', 'st-{id}', 'ca-{id}', 'fake', 'active', \
                 'manual', 1, 1)"
            ),
        ];
        for s in sql {
            h.db().execute(Statement::new(s)).await.unwrap();
        }
        out.push(FakeChannel::account(format!("u-{id}")));
    }
    out
}

pub async fn count(h: &Harness, sql: &str) -> i64 {
    let rows = h.db().query(Statement::new(sql.to_string())).await.unwrap();
    rows.get(0, "n").and_then(|v| v.as_i64()).unwrap_or(-1)
}

pub async fn relink_jobs(h: &Harness) -> i64 {
    count(
        h,
        "SELECT count(*) AS n FROM job WHERE kind = 'kernel.menu.relink' \
         AND json_extract(payload, '$.revision') IS NULL",
    )
    .await
}

/// Ticks a minute apart until no re-link Job is queued (at most 30).
pub async fn drain(h: &Harness, app: &TestApp) {
    for _ in 0..30 {
        let now = Timestamp(h.clock().now_ms());
        app.deployment().tick(now).await.unwrap();
        let left = count(
            h,
            "SELECT count(*) AS n FROM job WHERE kind = 'kernel.menu.relink' AND state = 'queued'",
        )
        .await;
        if left == 0 {
            return;
        }
        h.clock().advance_minutes(1);
    }
    panic!("re-link Jobs never drained");
}

pub fn label_of(h: &Harness, to: &ChannelAccountId) -> Vec<String> {
    let menu = h.fake_channel().menu(to).flatten().expect("Menu unlocked");
    menu.entries.iter().map(|e| e.label.clone()).collect()
}

pub fn revision_of(h: &Harness, to: &ChannelAccountId) -> u32 {
    h.fake_channel()
        .menu(to)
        .flatten()
        .expect("Menu unlocked")
        .revision()
}

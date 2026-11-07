//! Helpers for the nightly backup tests (#78): the clock at the scheduled
//! minute, ticks, an Administrator who may manage backups, `.tar` entries.

use harpseal::{Actor, RequestCtx, Timestamp};
use harpseal_testing::{Browser, Harness, START_MS, TestApp};

use super::{PASSWORD, query};

pub const HOUR: i64 = 3_600_000;
pub const DAY: i64 = 24 * HOUR;
/// `[backup] schedule` by default: 02:00 in Bangkok, 19:00 UTC.
pub const NIGHT: i64 = START_MS + 19 * HOUR;
pub const ADMIN: &str = "admin@sealbay.example";

/// `20261105T190000000Z` for `ms` (as the Job names a backup).
pub fn stamp_of(ms: i64) -> String {
    assert_eq!((ms - START_MS) % HOUR, 0, "whole hours only");
    let hours = (ms - START_MS) / HOUR;
    let (day, hour) = (5 + hours / 24, hours % 24);
    format!("202611{day:02}T{hour:02}0000000Z")
}

/// Ticks once a minute for `minutes` minutes, from now.
pub async fn ticks(h: &Harness, app: &TestApp, minutes: i64) {
    for _ in 0..minutes {
        let r = app
            .deployment()
            .tick(Timestamp(h.clock().now_ms()))
            .await
            .unwrap();
        assert!(r.errors.is_empty(), "{:?}", r.errors);
        h.clock().advance_minutes(1);
    }
}

/// Sets the clock to the scheduled minute of night `n` (0 = the first
/// one after the harness starts) and ticks for ten minutes.
pub async fn night(h: &Harness, app: &TestApp, n: i64) {
    h.clock().set_ms(NIGHT + n * DAY);
    ticks(h, app, 10).await;
}

/// A signed-in Administrator: Roles granting `kernel.staff.manage` (so
/// the failure emails reach them) and `kernel.backup.manage`.
pub async fn admin<'a>(h: &Harness, app: &'a TestApp) -> Browser<'a> {
    let perms = [
        "kernel.staff.manage",
        "kernel.backup.manage",
        "kernel.backup.download",
    ];
    harpseal::ensure_role(
        &h.services(),
        &Actor::System,
        "Backup Admin",
        &perms,
        &RequestCtx::cli(),
    )
    .await
    .unwrap();
    h.create_staff(ADMIN, PASSWORD, &["Backup Admin"]).await;
    let b = app.browser();
    let r = b.login(ADMIN, PASSWORD).await;
    assert_eq!(r.status, 303, "{}", r.body);
    b
}

/// `backup_run` rows as `(state, prefix)`, oldest first.
pub async fn runs(h: &Harness) -> Vec<(String, String)> {
    let rows = query(
        h.db().as_ref(),
        "SELECT state, prefix FROM backup_run ORDER BY started_at",
    )
    .await;
    rows.rows
        .iter()
        .map(|r| {
            let t = |i: usize| r[i].as_text().unwrap().to_string();
            (t(0), t(1))
        })
        .collect()
}

/// `(path, bytes)` of every file in a ustar archive.
pub fn untar(mut tar: &[u8]) -> Vec<(String, Vec<u8>)> {
    let mut out = Vec::new();
    while tar.len() >= 512 && tar[..512].iter().any(|&b| b != 0) {
        let field = |a: usize, b: usize| {
            let f = &tar[a..b];
            let end = f.iter().position(|&c| c == 0).unwrap_or(f.len());
            String::from_utf8(f[..end].to_vec()).unwrap()
        };
        let (name, prefix) = (field(0, 100), field(345, 500));
        let size = usize::from_str_radix(field(124, 135).trim(), 8).unwrap();
        let path = if prefix.is_empty() {
            name
        } else {
            format!("{prefix}/{name}")
        };
        out.push((path, tar[512..512 + size].to_vec()));
        tar = &tar[512 + size.div_ceil(512) * 512..];
    }
    out
}

//! The Kernel's status cards on the term status home (#123): the last
//! roster import, the message quota this month and the last backup. One
//! query each, run only on the home for Staff holding the card's
//! Permission. (Announcements add their own card from their Module.)

use crate::kernel::students::view::{channel_label, format_ms};
use crate::kernel::text::fill;
use crate::kernel::usage::{self, Level};
use crate::port_types::{Rows, Statement, Value};
use crate::{AppState, CardState, CurrentStaff, Result, StatusCard};

/// Who sees each card.
pub(super) const ROSTER: &str = "kernel.roster.view";
pub(super) const QUOTA: &str = usage::VIEW;
pub(super) const BACKUP: &str = crate::kernel::admin::backups::MANAGE;

struct Cx {
    s: AppState,
    staff: CurrentStaff,
}

impl Cx {
    fn t(&self, key: &str) -> String {
        self.s.catalog.t(&self.staff.locale, key)
    }
    fn time(&self, ms: i64) -> String {
        format_ms(ms, self.s.config.utc_offset_minutes().unwrap_or(0))
    }
    fn card(&self, state: CardState, prefix: &str, fact: String) -> StatusCard {
        StatusCard::new(state, self.t(&format!("{prefix}.title")), fact)
    }
}

fn text(rows: &Rows, c: &str) -> String {
    rows.get(0, c)
        .and_then(Value::as_text)
        .unwrap_or_default()
        .to_string()
}

fn int(rows: &Rows, c: &str) -> Option<i64> {
    rows.get(0, c).and_then(Value::as_i64)
}

/// The latest roster import that was not cancelled.
pub(super) async fn roster(s: AppState, staff: CurrentStaff) -> Result<StatusCard> {
    let cx = Cx { s, staff };
    let p = "kernel.home.roster";
    let rows = cx
        .s
        .services
        .db
        .query(Statement::new(
            "SELECT id, state, file_name, total_rows, created_at, finished_at FROM roster_import \
             WHERE state != 'cancelled' ORDER BY created_at DESC LIMIT 1",
        ))
        .await?;
    if rows.is_empty() {
        let card = cx.card(CardState::Empty, p, cx.t("kernel.home.roster.empty"));
        return Ok(card.action(cx.t("kernel.home.roster.start"), "/admin/roster"));
    }
    let file = text(&rows, "file_name");
    let at = cx.time(
        int(&rows, "finished_at")
            .or(int(&rows, "created_at"))
            .unwrap_or(0),
    );
    let open = format!("/admin/roster/{}", text(&rows, "id"));
    let fill_t = |key: &str, args: &[(&str, &str)]| fill(&cx.t(key), args);
    Ok(match text(&rows, "state").as_str() {
        "done" => {
            let n = int(&rows, "total_rows").unwrap_or(0).to_string();
            let fact = fill_t("kernel.home.roster.done", &[("time", &at), ("rows", &n)]);
            cx.card(CardState::Ok, p, fact)
                .action(cx.t("kernel.home.roster.new"), "/admin/roster")
        }
        "queued" | "running" => {
            let fact = fill_t("kernel.home.roster.running", &[("file", &file)]);
            cx.card(CardState::Ok, p, fact)
                .action(cx.t("kernel.home.roster.open"), open)
        }
        "previewed" => {
            let fact = fill_t("kernel.home.roster.waiting", &[("file", &file)]);
            cx.card(CardState::Warn, p, fact)
                .action(cx.t("kernel.home.roster.open"), open)
        }
        _ => {
            let fact = fill_t(
                "kernel.home.roster.failed",
                &[("file", &file), ("time", &at)],
            );
            cx.card(CardState::Warn, p, fact)
                .action(cx.t("kernel.home.roster.open"), open)
        }
    })
}

/// This month's metered messages of the Channel nearest its Quota.
pub(super) async fn quota(s: AppState, staff: CurrentStaff) -> Result<StatusCard> {
    let cx = Cx { s, staff };
    let p = "kernel.home.quota";
    let kinds: Vec<String> =
        cx.s.deployment
            .channel_kinds()
            .iter()
            .map(|k| k.as_str().to_string())
            .collect();
    let months = usage::month_summaries(&cx.s.deployment, &kinds).await?;
    let capped = months
        .iter()
        .filter_map(|(kind, m)| m.cap.filter(|c| *c > 0).map(|cap| (kind, m, cap)))
        .max_by_key(|(_, m, cap)| (m.level, m.used.max(0) as u64 * 1000 / cap));
    let Some((kind, m, cap)) = capped else {
        let card = cx.card(CardState::Empty, p, cx.t("kernel.home.quota.empty"));
        return Ok(card.action(cx.t("kernel.home.quota.set"), usage::FULL_PATH));
    };
    let percent = (m.used.max(0) as u64 * 100 / cap).to_string();
    let args = [
        ("channel", channel_label(kind)),
        ("used", m.used.to_string()),
        ("cap", cap.to_string()),
        ("percent", percent),
    ];
    let args: Vec<(&str, &str)> = args.iter().map(|(k, v)| (*k, v.as_str())).collect();
    let fact = fill(&cx.t("kernel.home.quota.used"), &args);
    let state = if m.level >= Level::Warn {
        CardState::Warn
    } else {
        CardState::Ok
    };
    Ok(cx
        .card(state, p, fact)
        .action(cx.t("kernel.home.quota.open"), usage::FULL_PATH))
}

/// The latest backup run, and when one last finished.
pub(super) async fn backup(s: AppState, staff: CurrentStaff) -> Result<StatusCard> {
    let cx = Cx { s, staff };
    let p = "kernel.home.backup";
    let rows =
        cx.s.services
            .db
            .query(Statement::new(
                "SELECT state, started_at, finished_at, \
             (SELECT max(finished_at) FROM backup_run WHERE state = 'done') AS last_done \
             FROM backup_run ORDER BY started_at DESC LIMIT 1",
            ))
            .await?;
    let open = "/admin/backups";
    if rows.is_empty() {
        let card = cx.card(CardState::Empty, p, cx.t("kernel.home.backup.empty"));
        return Ok(card.action(cx.t("kernel.home.backup.start"), open));
    }
    let at = |c: &str| cx.time(int(&rows, c).unwrap_or(0));
    let one = |key: &str, time: &str| fill(&cx.t(key), &[("time", time)]);
    let (state, fact) = match text(&rows, "state").as_str() {
        "failed" => (
            CardState::Warn,
            one("kernel.home.backup.failed", &at("started_at")),
        ),
        "running" => (
            CardState::Ok,
            one("kernel.home.backup.running", &at("started_at")),
        ),
        _ => (
            CardState::Ok,
            one("kernel.home.backup.done", &at("last_done")),
        ),
    };
    Ok(cx
        .card(state, p, fact)
        .action(cx.t("kernel.home.backup.open"), open))
}

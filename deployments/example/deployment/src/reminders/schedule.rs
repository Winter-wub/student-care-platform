//! The weekly schedule on the reminders page (#135): when the Job runs, to
//! whom, how many Students that is now, and the next run.

use harpseal::{AppState, CurrentStaff, Statement, Value, ui};

#[cfg(test)]
use super::SCHEDULE;
use super::{DAY_MS, HOUR, TAG};

/// The weekly schedule (#135): when it runs, to whom, how many now, and
/// the next run. A count that fails to load is left out, not guessed.
pub(super) async fn card(s: &AppState, staff: &CurrentStaff) -> maud::Markup {
    let t = |k: &str| s.catalog.t(&staff.locale, k);
    let q = Statement::new(
        "SELECT count(DISTINCT s.id) AS n FROM student s JOIN student_tag st \
         ON st.student_id = s.id JOIN tag t ON t.id = st.tag_id \
         WHERE t.name = ? AND s.status = 'active'",
    )
    .bind(TAG);
    let count = match s.services.db.query(q).await {
        Ok(r) => r.get(0, "n").and_then(Value::as_i64),
        Err(_) => None,
    };
    let next = s
        .config
        .utc_offset_minutes()
        .ok()
        .map(|o| next_run(s.services.now_ms(), i64::from(o) * 60_000));
    let rule = t("reminders.schedule")
        .replace("{hour}", &format!("{HOUR:02}"))
        .replace("{tag}", &s.catalog.tag_name(&staff.locale, TAG));
    maud::html! {
        section class="card" id="schedule" {
            h2 { (t("reminders.schedule_title")) }
            p { (rule) }
            @if let Some(n) = count {
                p { strong { (t("reminders.schedule_count").replace("{n}", &ui::number(n))) } }
            }
            @if let Some(at) = next {
                p class="muted" { (t("reminders.schedule_next").replace("{time}", &ui::local_datetime(s, at))) }
            }
        }
    }
}

/// The next Monday [`HOUR`]:00 local after `now` (UTC ms), `offset` ms
/// east of UTC. Day 0 of the epoch was a Thursday, so a day `d` is a
/// Monday when `(d + 3) % 7 == 0`.
fn next_run(now: i64, offset: i64) -> i64 {
    let local = now + offset;
    let today = local.div_euclid(DAY_MS);
    (today..today + 8)
        .filter(|d| (d + 3).rem_euclid(7) == 0)
        .map(|d| d * DAY_MS + HOUR * 3_600_000)
        .find(|at| *at > local)
        .unwrap_or(local)
        - offset
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Monday 2026-11-09 09:00 in Bangkok (02:00 UTC).
    const MONDAY_9AM: i64 = 1_794_189_600_000;
    const BANGKOK: i64 = 7 * 3_600_000;

    #[test]
    fn the_card_and_next_run_follow_the_cron_schedule() {
        // The card says "every Monday at HOUR:00" and next_run counts to
        // it: both break if SCHEDULE moves without them.
        assert_eq!(SCHEDULE, format!("0 {HOUR} * * 1"));
    }

    #[test]
    fn the_next_run_is_the_coming_monday_at_nine_local() {
        assert_eq!(next_run(MONDAY_9AM - 1, BANGKOK), MONDAY_9AM);
        assert_eq!(next_run(MONDAY_9AM, BANGKOK), MONDAY_9AM + 7 * DAY_MS);
        assert_eq!(
            next_run(MONDAY_9AM - 3 * DAY_MS, BANGKOK),
            MONDAY_9AM,
            "from Friday"
        );
    }
}

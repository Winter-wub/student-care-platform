//! A nightly backup that fails (#78): retried like any Job, then marked
//! failed on its last attempt, shown on the page, and emailed to every
//! active Administrator at most once per failure day. Retention does not
//! run after a failure.

mod backup_support;

use backup_support::nightly::{ADMIN, DAY, NIGHT, admin, runs, stamp_of, ticks};
use backup_support::{OFFICER, PASSWORD, populated, query};
use harpseal_testing::Harness;

/// Makes the run started at `ms` fail on every attempt (its progress
/// object cannot be read or written).
fn break_run_at(h: &Harness, ms: i64) {
    h.files()
        .fail_on(&format!("backups/{}.progress.json", stamp_of(ms)));
}

/// Ticks through the retries: 1, 5 and 30 minutes apart.
async fn through_retries(h: &Harness, app: &harpseal_testing::TestApp, from: i64) {
    h.clock().set_ms(from);
    ticks(h, app, 45).await;
}

#[tokio::test]
async fn a_failure_is_retried_then_shown_and_emailed_once_a_day() {
    let (h, app) = populated().await;
    let b = admin(&h, &app).await;
    let before = h.emails().len();

    break_run_at(&h, NIGHT);
    through_retries(&h, &app, NIGHT).await;
    let state = runs(&h).await;
    assert_eq!(state.len(), 1);
    assert_eq!(state[0].0, "failed");
    let job = query(
        h.db().as_ref(),
        "SELECT state, attempts FROM job WHERE kind = 'kernel.backup.run' \
         AND payload LIKE '%\"run\"%'",
    )
    .await;
    assert_eq!(job.rows[0][0].as_text(), Some("failed"));
    assert_eq!(
        job.rows[0][1].as_i64(),
        Some(4),
        "the first run and three retries"
    );

    let mail: Vec<_> = h.emails()[before..].to_vec();
    assert_eq!(mail.len(), 1, "only the Administrator: {mail:?}");
    assert_eq!(mail[0].to, ADMIN);
    assert!(!h.emails().iter().any(|m| m.to == OFFICER));
    assert!(
        mail[0].subject.contains("2026-11-06"),
        "{}",
        mail[0].subject
    );
    assert!(mail[0].text.contains("/admin/backups"));

    assert_eq!(
        b.login(ADMIN, PASSWORD).await.status,
        303,
        "the clock moved past the session"
    );
    let page = b.get("/admin/backups").await;
    assert!(page.body.contains("data-state=\"failed\""));
    assert!(page.body.contains("injected failure"), "the error is shown");
    assert!(
        page.body.contains("id=\"backup-failed\""),
        "the latest backup failed"
    );
    assert!(page.body.contains("id=\"backup-job-errors\""));

    // A second failure the same (local) day: no second email.
    let later = NIGHT + 3 * 3_600_000;
    break_run_at(&h, later);
    h.clock().set_ms(later);
    assert_eq!(b.login(ADMIN, PASSWORD).await.status, 303);
    assert_eq!(b.get("/admin/backups").await.status, 200, "a CSRF token");
    assert_eq!(b.post("/admin/backups/now", "").await.status, 303);
    through_retries(&h, &app, later).await;
    assert_eq!(runs(&h).await.len(), 2);
    assert!(runs(&h).await.iter().all(|(s, _)| s == "failed"));
    assert_eq!(h.emails().len(), before + 1, "one email per failure day");

    // The next night succeeds: the alert goes, nothing is pruned.
    h.clock().set_ms(NIGHT + DAY);
    ticks(&h, &app, 10).await;
    let last = runs(&h).await;
    assert_eq!(last.len(), 3);
    assert_eq!(last[2].0, "done");
    assert_eq!(
        b.login(ADMIN, PASSWORD).await.status,
        303,
        "the clock moved past the session"
    );
    let page = b.get("/admin/backups").await;
    assert!(!page.body.contains("id=\"backup-failed\""), "{}", page.body);

    // A failure on another day emails again.
    break_run_at(&h, NIGHT + 2 * DAY);
    through_retries(&h, &app, NIGHT + 2 * DAY).await;
    assert_eq!(h.emails().len(), before + 2);
    let notified = query(
        h.db().as_ref(),
        "SELECT count(*) FROM audit_log WHERE action = 'backup.notify'",
    )
    .await;
    assert_eq!(notified.rows[0][0].as_i64(), Some(2));
}

#[tokio::test]
async fn a_run_that_stops_moving_is_failed_by_its_watch() {
    let (h, app) = populated().await;
    admin(&h, &app).await;
    let before = h.emails().len();
    // A run whose Worker died for good: running, untouched for hours.
    h.clock().set_ms(NIGHT);
    ticks(&h, &app, 1).await;
    let id = backup_support::text(&h, "SELECT id FROM backup_run").await;
    h.db()
        .execute(harpseal::Statement::new(
            "DELETE FROM job WHERE kind = 'kernel.backup.run' AND payload LIKE '%\"run\"%'",
        ))
        .await
        .unwrap();
    h.clock().set_ms(NIGHT + 2 * 3_600_000 + 60_000);
    ticks(&h, &app, 2).await;
    let row = query(
        h.db().as_ref(),
        &format!("SELECT state, error FROM backup_run WHERE id = '{id}'"),
    )
    .await;
    assert_eq!(row.rows[0][0].as_text(), Some("failed"));
    assert!(row.rows[0][1].as_text().unwrap().contains("stalled"));
    assert_eq!(h.emails().len(), before + 1);
}

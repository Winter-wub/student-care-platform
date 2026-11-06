//! Roster import recovery at the Deployment seam (#24 review): an import
//! whose Job died gives the slot up, Staff can mark a stuck one failed,
//! the left phase runs one batch of 100 per Job, a Staff edit made while a
//! chunk runs survives, and a revoked notice that fails is counted.

mod roster_support;

use harpseal::Timestamp;
use harpseal_channel_fake::FakeChannel;
use roster_support::*;

async fn tick(h: &harpseal_testing::Harness, app: &harpseal_testing::TestApp) -> usize {
    app.deployment()
        .tick(Timestamp(h.clock().now_ms()))
        .await
        .unwrap()
        .claimed
}

#[tokio::test]
async fn an_import_whose_job_died_gives_the_slot_up() {
    let (h, app) = boot().await;
    let b = signed_in(&h, &app, "Officer").await;
    let id = previewed(&b, "a.csv", &csv(0..600), "full").await;
    confirm(&b, &id).await;
    tick(&h, &app).await;
    assert_eq!(import_text(&h, &id, "state").await, "running");
    // The runner gave up on the second chunk (a Worker killed past its
    // CPU limit, its lock then expired with no retries left).
    run(
        &h,
        "UPDATE job SET state = 'failed', last_error = 'lock expired' \
         WHERE unique_key LIKE 'roster:%:500'",
    )
    .await;
    let again = upload(&b, "b.csv", &csv(0..3), "full").await;
    assert_eq!(flash(&b), "ok:kernel.roster.previewed", "the slot is free");
    assert_ne!(import_id(&again), id);
    assert_eq!(import_text(&h, &id, "state").await, "failed");
    let error = import_text(&h, &id, "error").await;
    assert!(error.contains("ran out of time"), "{error}");
    // The polling fragment shows why, as the full page does.
    let req = http::Request::get(format!("/admin/roster/{id}"))
        .header("hx-request", "true")
        .body(axum::body::Body::empty())
        .unwrap();
    let fragment = b.send(req).await.body;
    assert!(fragment.contains("ran out of time"), "{fragment}");
    assert!(fragment.contains("การนำเข้าหยุดกลางทาง"));
}

#[tokio::test]
async fn staff_can_mark_a_stuck_import_failed_after_the_lock_window() {
    let (h, app) = boot().await;
    let b = signed_in(&h, &app, "Officer").await;
    let id = previewed(&b, "a.csv", &csv(0..3), "full").await;
    confirm(&b, &id).await;
    let page = b.get(&format!("/admin/roster/{id}")).await.body;
    assert!(!page.contains("/fail"), "not before the lock window");
    let early = b.post(&format!("/admin/roster/{id}/fail"), "").await;
    assert_eq!(early.status, 303);
    assert_eq!(flash(&b), "err:kernel.roster.not_stuck");
    h.clock().advance_minutes(16);
    let page = b.get(&format!("/admin/roster/{id}")).await.body;
    assert!(page.contains(&format!("/admin/roster/{id}/fail")), "{page}");
    b.post(&format!("/admin/roster/{id}/fail"), "").await;
    assert_eq!(flash(&b), "ok:kernel.roster.marked_failed");
    assert_eq!(import_text(&h, &id, "state").await, "failed");
    let audit = "SELECT count(*) FROM audit_log WHERE action = 'roster.import.fail' \
                 AND target_id = ? AND actor_kind = 'staff'";
    assert_eq!(int(&h, audit, &[&id]).await, 1);
    // Its queued Job finds it failed and writes nothing.
    run_jobs(&h, &app).await;
    assert_eq!(int(&h, "SELECT count(*) FROM student", &[]).await, 0);
    let viewer = signed_in(&h, &app, "Viewer").await;
    let r = viewer.post(&format!("/admin/roster/{id}/fail"), "").await;
    assert_eq!(r.status, 403);
}

#[tokio::test]
async fn two_hundred_fifty_leaving_students_take_three_left_jobs() {
    let (h, app) = boot().await;
    let b = signed_in(&h, &app, "Officer").await;
    let first = previewed(&b, "a.csv", &csv(0..260), "full").await;
    confirm(&b, &first).await;
    run_jobs(&h, &app).await;
    let id = previewed(&b, "b.csv", &csv(0..10), "full").await;
    assert_eq!(counts(&h, &id).await["preview"]["will_become_left"], 250);
    confirm(&b, &id).await;
    assert_eq!(tick(&h, &app).await, 1, "the chunk");
    let mut left = Vec::new();
    for _ in 0..3 {
        assert_eq!(tick(&h, &app).await, 1, "one left batch per Job");
        let c = counts(&h, &id).await;
        left.push((
            c["left"].as_i64().unwrap(),
            import_text(&h, &id, "state").await,
        ));
    }
    assert_eq!(
        left,
        vec![
            (100, "running".to_string()),
            (200, "running".to_string()),
            (250, "done".to_string())
        ]
    );
    assert_eq!(tick(&h, &app).await, 0, "nothing more");
    let keys = "SELECT count(*) FROM job WHERE unique_key LIKE ?";
    assert_eq!(int(&h, keys, &[&format!("roster:{id}:left:%")]).await, 3);
    assert_eq!(
        int(
            &h,
            "SELECT count(*) FROM student WHERE status = 'left'",
            &[]
        )
        .await,
        250
    );
}

#[tokio::test]
async fn a_staff_edit_made_while_the_chunk_runs_survives() {
    let (h, app) = boot().await;
    let b = signed_in(&h, &app, "Officer").await;
    let first = previewed(&b, "a.csv", &csv(0..3), "full").await;
    confirm(&b, &first).await;
    run_jobs(&h, &app).await;
    run(
        &h,
        "UPDATE student SET display_name = 'แก้ครั้งแรก', \
         attributes = json_set(attributes, '$.year', 5), \
         manual_overrides = '[\"display_name\",\"attributes.year\"]' \
         WHERE student_code = '68000001'",
    )
    .await;
    let mut file = String::from_utf8(csv(0..3)).unwrap();
    file = file.replace("ปี1;DDS,DDS,1,3.0", "ปี2;DDS,DDS,2,3.5");
    let id = previewed(&b, "b.csv", file.as_bytes(), "full").await;
    confirm(&b, &id).await;
    // Staff edit again after the Job read the row: the trigger runs
    // inside the chunk's batch, right after its compare-and-set.
    run(
        &h,
        "CREATE TRIGGER race AFTER UPDATE OF applied_rows ON roster_import BEGIN \
         UPDATE student SET display_name = 'แก้ระหว่างนำเข้า', \
         attributes = json_set(attributes, '$.year', 6) WHERE student_code = '68000001'; END",
    )
    .await;
    run_jobs(&h, &app).await;
    assert_eq!(import_text(&h, &id, "state").await, "done");
    let kept = "SELECT count(*) FROM student WHERE student_code = '68000001' \
                AND display_name = 'แก้ระหว่างนำเข้า' AND json_extract(attributes, '$.year') = 6 \
                AND json_extract(attributes, '$.gpa') = 3.5";
    assert_eq!(int(&h, kept, &[]).await, 1, "both edits kept, gpa imported");
    let others = "SELECT count(*) FROM student WHERE json_extract(attributes, '$.year') = 2";
    assert_eq!(int(&h, others, &[]).await, 2);
}

#[tokio::test]
async fn a_revoked_notice_that_fails_is_counted() {
    let (h, app) = boot().await;
    let b = signed_in(&h, &app, "Officer").await;
    linked_student(&h, "gone", "68000900", "active").await;
    h.fake_channel()
        .set_unreachable(&FakeChannel::account("u-gone"), true);
    let id = previewed(&b, "a.csv", &csv(0..2), "full").await;
    confirm(&b, &id).await;
    run_jobs(&h, &app).await;
    assert_eq!(import_text(&h, &id, "state").await, "done");
    let c = counts(&h, &id).await;
    assert_eq!(c["links_revoked"], 1);
    assert_eq!(c["notices_failed"], 1);
    let link = "SELECT count(*) FROM link WHERE id = 'l-gone' AND status = 'revoked'";
    assert_eq!(int(&h, link, &[]).await, 1, "the revoke stands");
}

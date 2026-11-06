//! Roster import guards at the Deployment seam (#24): one import at a
//! time, permissions, CSRF, cancel, double submits, the progress fragment
//! and the audited file download.

mod roster_support;

use axum::body::Body;
use harpseal::Timestamp;
use http::Request;
use roster_support::*;

#[tokio::test]
async fn a_second_upload_is_refused_while_one_is_previewed_or_running() {
    let (h, app) = boot().await;
    let b = signed_in(&h, &app, "Officer").await;
    let first = previewed(&b, "a.csv", &csv(0..600), "full").await;
    let r = upload(&b, "b.csv", &csv(0..3), "full").await;
    assert_eq!(
        r.header("location"),
        Some(format!("/admin/roster/{first}").as_str())
    );
    assert_eq!(flash(&b), "err:kernel.roster.busy");
    assert_eq!(int(&h, "SELECT count(*) FROM roster_import", &[]).await, 1);

    confirm(&b, &first).await;
    // Running after the first chunk; still one at a time.
    app.deployment()
        .tick(Timestamp(h.clock().now_ms()))
        .await
        .unwrap();
    assert_eq!(import_text(&h, &first, "state").await, "running");
    h.clock().advance_minutes(120);
    let r = upload(&b, "b.csv", &csv(0..3), "full").await;
    assert_eq!(flash(&b), "err:kernel.roster.busy");
    assert_eq!(import_id(&r), first);

    run_jobs(&h, &app).await;
    previewed(&b, "b.csv", &csv(0..3), "full").await;
}

#[tokio::test]
async fn a_preview_older_than_an_hour_is_replaced() {
    let (h, app) = boot().await;
    let b = signed_in(&h, &app, "Officer").await;
    let old = previewed(&b, "a.csv", &csv(0..3), "full").await;
    h.clock().advance_minutes(61);
    let new = previewed(&b, "b.csv", &csv(0..3), "upsert").await;
    assert_ne!(old, new);
    assert_eq!(import_text(&h, &old, "state").await, "cancelled");
    let r = confirm(&b, &old).await;
    assert_eq!(r.status, 303);
    assert_eq!(flash(&b), "err:kernel.roster.not_previewed");
}

#[tokio::test]
async fn a_viewer_sees_imports_but_cannot_upload_confirm_or_cancel() {
    let (h, app) = boot().await;
    let officer = signed_in(&h, &app, "Officer").await;
    let id = previewed(&officer, "a.csv", &csv(0..3), "full").await;
    let viewer = signed_in(&h, &app, "Viewer").await;
    assert_eq!(viewer.get("/admin/roster").await.status, 200);
    let page = viewer.get(&format!("/admin/roster/{id}")).await;
    assert_eq!(page.status, 200);
    assert!(!page.body.contains("/confirm"), "no buttons for a Viewer");
    assert_eq!(
        upload(&viewer, "b.csv", &csv(0..3), "full").await.status,
        403
    );
    for action in ["confirm", "cancel"] {
        let r = viewer
            .post(&format!("/admin/roster/{id}/{action}"), "")
            .await;
        assert_eq!(r.status, 403, "{action}");
    }
    assert_eq!(import_text(&h, &id, "state").await, "previewed");
    assert_eq!(int(&h, "SELECT count(*) FROM roster_import", &[]).await, 1);
}

#[tokio::test]
async fn posts_without_the_csrf_token_are_refused() {
    let (h, app) = boot().await;
    let b = signed_in(&h, &app, "Officer").await;
    let r = upload_with(&b, "a.csv", &csv(0..3), "full", Some("")).await;
    assert_eq!(r.status, 403);
    let r = upload_with(&b, "a.csv", &csv(0..3), "full", Some("0000")).await;
    assert_eq!(r.status, 403);
    assert_eq!(int(&h, "SELECT count(*) FROM roster_import", &[]).await, 0);
    let id = previewed(&b, "a.csv", &csv(0..3), "full").await;
    let r = b.post_raw(&format!("/admin/roster/{id}/confirm"), "").await;
    assert_eq!(r.status, 403);
    assert_eq!(import_text(&h, &id, "state").await, "previewed");
}

#[tokio::test]
async fn cancel_frees_the_slot_and_a_double_confirm_is_one_job() {
    let (h, app) = boot().await;
    let b = signed_in(&h, &app, "Officer").await;
    let id = previewed(&b, "a.csv", &csv(0..3), "full").await;
    b.get(&format!("/admin/roster/{id}")).await;
    let r = b.post(&format!("/admin/roster/{id}/cancel"), "").await;
    assert_eq!(r.status, 303);
    assert_eq!(flash(&b), "ok:kernel.roster.cancelled");
    assert_eq!(import_text(&h, &id, "state").await, "cancelled");
    let cancel = "SELECT count(*) FROM audit_log WHERE action = 'roster.import.cancel'";
    assert_eq!(int(&h, cancel, &[]).await, 1);

    let id = previewed(&b, "b.csv", &csv(0..3), "full").await;
    assert_eq!(confirm(&b, &id).await.status, 303);
    let again = b.post(&format!("/admin/roster/{id}/confirm"), "").await;
    assert_eq!(
        again.header("location"),
        Some(format!("/admin/roster/{id}").as_str())
    );
    assert_eq!(flash(&b), "err:kernel.roster.not_previewed");
    let jobs = "SELECT count(*) FROM job WHERE kind = 'kernel.roster.import'";
    assert_eq!(int(&h, jobs, &[]).await, 1);
    let audit = "SELECT count(*) FROM audit_log WHERE action = 'roster.import'";
    assert_eq!(int(&h, audit, &[]).await, 1);
}

async fn fragment(b: &harpseal_testing::Browser<'_>, id: &str) -> harpseal_testing::TestResponse {
    let req = Request::get(format!("/admin/roster/{id}"))
        .header("hx-request", "true")
        .body(Body::empty())
        .unwrap();
    b.send(req).await
}

#[tokio::test]
async fn the_progress_fragment_polls_until_the_import_is_done() {
    let (h, app) = boot().await;
    let b = signed_in(&h, &app, "Officer").await;
    let id = previewed(&b, "a.csv", &csv(0..700), "full").await;
    confirm(&b, &id).await;
    app.deployment()
        .tick(Timestamp(h.clock().now_ms()))
        .await
        .unwrap();
    let r = fragment(&b, &id).await;
    assert_eq!(r.header("vary"), Some("HX-Request"));
    assert!(
        r.body.starts_with("<div id=\"roster-progress\""),
        "{}",
        r.body
    );
    assert!(r.body.contains("hx-trigger=\"every 2s\""));
    assert!(r.body.contains("value=\"500\""), "{}", r.body);
    assert!(!r.body.contains("<html"), "fragment only");
    run_jobs(&h, &app).await;
    let r = fragment(&b, &id).await;
    assert!(!r.body.contains("hx-trigger"), "stops polling once done");
    assert!(r.body.contains("700 / 700"), "{}", r.body);
}

#[tokio::test]
async fn downloading_the_original_file_is_an_audited_read() {
    let (h, app) = boot().await;
    let b = signed_in(&h, &app, "Viewer").await;
    let officer = signed_in(&h, &app, "Officer").await;
    let id = previewed(&officer, "รายชื่อ.xlsx", XLSX_50, "full").await;
    let r = b.get(&format!("/admin/roster/{id}/file")).await;
    assert_eq!(r.status, 200);
    assert_eq!(r.header("cache-control"), Some("no-store"));
    let disposition = r.header("content-disposition").unwrap();
    assert!(
        disposition.contains("filename*=UTF-8''%E0%B8%A3"),
        "{disposition}"
    );
    let audit = "SELECT count(*) FROM audit_log WHERE action = 'roster.file.download' \
                 AND target_id = ? AND actor_kind = 'staff'";
    assert_eq!(int(&h, audit, &[&id]).await, 1);
    assert_eq!(b.get("/admin/roster/nope/file").await.status, 404);
}

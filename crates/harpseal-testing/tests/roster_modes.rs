//! `[roster] modes` and `default_mode` (#79): a Deployment whose files
//! never list every Student (one file per cohort) allows `upsert` only, so
//! a missing or forged `full` can never mark the other cohorts left.

mod roster_support;

use harpseal::DeploymentConfig;
use harpseal_testing::{Harness, TestApp};
use roster_support::*;

const ACTIVE: &str = "SELECT count(*) FROM student WHERE status = 'active'";
const LIVE_LINKS: &str = "SELECT count(*) FROM link WHERE status = 'active'";

/// [`CONFIG`] with `extra` lines at the top of `[roster]`, booted on `h`
/// (a second boot on the same Harness is a config change on a live
/// database).
async fn boot_in(h: &Harness, extra: &str) -> TestApp {
    let toml = CONFIG.replacen("[roster]\n", &format!("[roster]\n{extra}\n"), 1);
    let config = DeploymentConfig::from_toml(&toml).unwrap();
    h.boot(h.builder().config(config).build().unwrap()).await
}

async fn boot_with(extra: &str) -> (Harness, TestApp) {
    let h = Harness::new();
    let app = boot_in(&h, extra).await;
    (h, app)
}

const UPSERT_ONLY: &str = "modes = [\"upsert\"]\ndefault_mode = \"upsert\"";

/// A request with no `mode` part at all.
async fn upload_without_mode(b: &harpseal_testing::Browser<'_>, bytes: &[u8]) -> String {
    let body = format!(
        "--x\r\nContent-Disposition: form-data; name=\"_csrf\"\r\n\r\n{}\r\n\
         --x\r\nContent-Disposition: form-data; name=\"file\"; filename=\"a.csv\"\r\n\
         Content-Type: text/csv\r\n\r\n{}\r\n--x--\r\n",
        b.csrf().unwrap(),
        String::from_utf8(bytes.to_vec()).unwrap()
    );
    let req = http::Request::post("/admin/roster/upload")
        .header(
            http::header::CONTENT_TYPE,
            "multipart/form-data; boundary=x",
        )
        .body(axum::body::Body::from(body))
        .unwrap();
    let r = b.send(req).await;
    assert_eq!(r.status, 303, "{}", r.body);
    r.header("location").unwrap().to_string()
}

#[tokio::test]
async fn the_default_config_keeps_both_modes_with_upsert_first_and_preselected() {
    // #120: a full replace is a deliberate choice.
    let (h, app) = boot().await;
    let b = signed_in(&h, &app, "Officer").await;
    let page = b.get("/admin/roster").await.body;
    assert!(page.contains(r#"value="upsert" checked"#), "{page}");
    assert!(page.contains(r#"value="full""#));
    assert!(!page.contains(r#"value="full" checked"#));
    let (up, full) = (page.find(r#"value="upsert""#), page.find(r#"value="full""#));
    assert!(up < full, "upsert is offered first");
    // A request without a mode is an `upsert` import: nobody becomes left.
    linked_student(&h, "a1", "68000900", "active").await;
    let to = upload_without_mode(&b, &csv(0..3)).await;
    let id = to.strip_prefix("/admin/roster/").unwrap();
    assert_eq!(import_text(&h, id, "mode").await, "upsert");
    assert_eq!(counts(&h, id).await["preview"]["will_become_left"], 0);
}

#[tokio::test]
async fn an_upsert_only_form_offers_only_upsert_preselected() {
    let (h, app) = boot_with(UPSERT_ONLY).await;
    let b = signed_in(&h, &app, "Officer").await;
    let page = b.get("/admin/roster").await.body;
    assert!(page.contains(r#"value="upsert" checked"#), "{page}");
    assert!(!page.contains(r#"value="full""#), "{page}");
}

#[tokio::test]
async fn a_second_cohort_file_leaves_the_first_cohort_and_its_links_alone() {
    let (h, app) = boot_with(UPSERT_ONLY).await;
    let b = signed_in(&h, &app, "Officer").await;
    // Cohort A is on the roster, one of them linked; the cohort B file
    // lists only the others.
    linked_student(&h, "a1", "68000001", "active").await;
    linked_student(&h, "a2", "68000002", "active").await;
    // No mode in the request: the configured default, never `full`.
    let to = upload_without_mode(&b, &csv(10..13)).await;
    let id = to.strip_prefix("/admin/roster/").unwrap().to_string();
    assert_eq!(flash(&b), "ok:kernel.roster.previewed");
    assert_eq!(import_text(&h, &id, "mode").await, "upsert");
    let page = b.get(&format!("/admin/roster/{id}")).await.body;
    assert!(page.contains("เพิ่ม/แก้ไข"), "the preview names the mode");
    assert!(!page.contains(r#"data-count="will_become_left""#));
    confirm(&b, &id).await;
    run_jobs(&h, &app).await;
    assert_eq!(import_text(&h, &id, "state").await, "done");
    assert_eq!(
        int(&h, ACTIVE, &[]).await,
        5,
        "2 before + 3 new, nobody left"
    );
    assert_eq!(int(&h, LIVE_LINKS, &[]).await, 2, "their Links stay");
    assert_eq!(counts(&h, &id).await["left"].as_i64().unwrap_or(0), 0);
}

#[tokio::test]
async fn a_forged_full_request_is_refused_and_imports_nothing() {
    let (h, app) = boot_with(UPSERT_ONLY).await;
    let b = signed_in(&h, &app, "Officer").await;
    linked_student(&h, "a1", "68000001", "active").await;
    for mode in ["full", "FULL", "merge"] {
        let r = upload(&b, "a.csv", &csv(10..13), mode).await;
        assert_eq!(r.status, 400, "{mode}: {}", r.body);
        assert!(r.body.contains("ไม่เปิดให้นำเข้าแบบนี้"), "{}", r.body);
    }
    assert_eq!(int(&h, "SELECT count(*) FROM roster_import", &[]).await, 0);
    assert_eq!(h.files().keys(), Vec::<String>::new());
    assert_eq!(int(&h, ACTIVE, &[]).await, 1);
    assert_eq!(int(&h, LIVE_LINKS, &[]).await, 1);
    // The same file as `upsert` goes through.
    previewed(&b, "a.csv", &csv(10..13), "upsert", false).await;
}

#[tokio::test]
async fn the_code_path_refuses_a_mode_the_config_does_not_allow() {
    let (h, app) = boot_with(UPSERT_ONLY).await;
    let id = h
        .create_staff("o@sealbay.example", PASSWORD, &["Officer"])
        .await;
    let actor = harpseal::Actor::Staff(id);
    let ctx = harpseal::RequestCtx::default();
    let d = app.deployment();
    let err = harpseal::roster::upload(d, "a.csv", csv(0..2), "full", &actor, &ctx).await;
    assert!(err.is_err());
    assert_eq!(int(&h, "SELECT count(*) FROM roster_import", &[]).await, 0);
    let ok = harpseal::roster::upload(d, "a.csv", csv(0..2), "upsert", &actor, &ctx).await;
    assert!(
        matches!(ok, Ok(harpseal::roster::Uploaded::Previewed(_))),
        "{ok:?}"
    );
}

#[tokio::test]
async fn with_both_modes_and_an_upsert_default_a_full_preview_keeps_the_leave_warning() {
    // `full` allowed (both modes, default upsert): a full preview still
    // lists who will leave.
    let (h, app) = boot_with("default_mode = \"upsert\"").await;
    let b = signed_in(&h, &app, "Officer").await;
    let page = b.get("/admin/roster").await.body;
    assert!(page.contains(r#"value="upsert" checked"#), "{page}");
    assert!(page.contains(r#"value="full""#));
    linked_student(&h, "gone", "68000900", "active").await;
    // The flash names the warning: it is the one message announced (#129).
    let r = upload(&b, "a.csv", &csv(0..3), "full").await;
    assert_eq!(r.status, 303, "{}", r.body);
    assert_eq!(flash(&b), "err:kernel.roster.previewed_warning");
    let id = import_id(&r);
    let page = b.get(&format!("/admin/roster/{id}")).await.body;
    assert!(page.contains("ทั้งทะเบียน"), "the preview names the mode");
    assert!(page.contains("ตรวจไฟล์แล้ว ระวัง:"), "{page}");
    assert!(page.contains(r#"data-count="will_become_left""#), "{page}");
    assert!(page.contains("Old gone"), "{page}");
    // The warning stays on the page, not a second live region under the
    // flash; opened again later, it is the page's one alert (#129).
    // In numbers (#134): who leaves, of how many, and the Links cut.
    assert!(
        page.contains(r#"<div class="warn" id="roster-alarm">1 จาก 1 คน (100%) จะพ้นสภาพ และถูกตัดการเชื่อมบัญชีแชท 1 บัญชี"#),
        "the warning stays: {page}"
    );
    assert_eq!(
        harpseal_testing::a11y::problems(&page),
        Vec::<String>::new()
    );
    let again = b.get(&format!("/admin/roster/{id}")).await.body;
    assert!(
        again.contains(r#"<div class="warn" id="roster-alarm" role="alert">1 จาก 1 คน"#),
        "{again}"
    );
}

#[tokio::test]
async fn a_full_only_deployment_offers_and_accepts_only_full() {
    let (h, app) = boot_with("modes = [\"full\"]").await;
    let b = signed_in(&h, &app, "Officer").await;
    let page = b.get("/admin/roster").await.body;
    assert!(page.contains(r#"value="full" checked"#), "{page}");
    assert!(!page.contains(r#"value="upsert""#), "{page}");
    let r = upload(&b, "a.csv", &csv(0..3), "upsert").await;
    assert_eq!(r.status, 400, "{}", r.body);
    assert_eq!(int(&h, "SELECT count(*) FROM roster_import", &[]).await, 0);
    previewed(&b, "a.csv", &csv(0..3), "full", false).await;
}

#[tokio::test]
async fn a_full_preview_cannot_be_confirmed_after_the_deployment_went_upsert_only() {
    // A cohort file previewed as `full` while both modes were allowed…
    let (h, first) = boot_with("").await;
    let b = signed_in(&h, &first, "Officer").await;
    linked_student(&h, "other", "68000900", "active").await;
    let id = previewed(&b, "a.csv", &csv(0..3), "full", true).await;
    // …then the Deployment turns `full` off, and the preview is still there.
    let app = boot_in(&h, UPSERT_ONLY).await;
    let b = app.browser();
    assert_eq!(
        b.login("officer@sealbay.example", PASSWORD).await.status,
        303
    );
    let page = b.get(&format!("/admin/roster/{id}")).await.body;
    assert!(page.contains("ผลตรวจไฟล์นี้ใช้แบบการนำเข้า"), "{page}");
    // A plain GET has no flash: the refusal is the page's one alert (#129).
    assert!(
        page.contains(r#"<div class="warn" role="alert">ผลตรวจไฟล์นี้"#),
        "{page}"
    );
    assert_eq!(
        harpseal_testing::a11y::problems(&page),
        Vec::<String>::new()
    );
    assert!(
        !page.contains(&format!("/admin/roster/{id}/confirm")),
        "{page}"
    );
    let r = b.post(&format!("/admin/roster/{id}/confirm"), "").await;
    assert_eq!(r.status, 400, "{}", r.body);
    assert!(r.body.contains("กรุณาอัปโหลดไฟล์ใหม่"), "{}", r.body);
    assert_eq!(import_text(&h, &id, "state").await, "previewed");
    assert_eq!(run_jobs(&h, &app).await, 0, "no Job was queued");
    assert_eq!(int(&h, ACTIVE, &[]).await, 1, "nobody was imported or left");
    assert_eq!(int(&h, LIVE_LINKS, &[]).await, 1);
    // The same from code, and the preview can still be cancelled.
    let rows = h
        .db()
        .query(harpseal::Statement::new(
            "SELECT id FROM staff WHERE email = 'officer@sealbay.example'",
        ))
        .await
        .unwrap();
    let staff = rows
        .get(0, "id")
        .and_then(|v| v.as_text())
        .unwrap()
        .to_string();
    let actor = harpseal::Actor::Staff(staff);
    let ctx = harpseal::RequestCtx::default();
    let out = harpseal::roster::confirm(app.deployment(), &id, &actor, &ctx).await;
    assert_eq!(out.unwrap(), harpseal::roster::Confirmed::ModeNotAllowed);
    b.get(&format!("/admin/roster/{id}")).await;
    b.post(&format!("/admin/roster/{id}/cancel"), "").await;
    assert_eq!(import_text(&h, &id, "state").await, "cancelled");
}

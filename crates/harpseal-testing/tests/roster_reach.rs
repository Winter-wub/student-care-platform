//! Confirmation in proportion to reach (#120): a `full` preview's confirm
//! names how many Students will become left in the danger style, and a
//! warned one needs that number typed (only the number since #135, never
//! the word "ยืนยัน"), checked on the server.

mod roster_support;

use harpseal::DeploymentConfig;
use harpseal_testing::{Browser, Harness, TestApp};
use roster_support::*;

const ACTIVE: &str = "SELECT count(*) FROM student WHERE status = 'active'";
const LIVE_LINKS: &str = "SELECT count(*) FROM link WHERE status = 'active'";
const DANGER: &str = r#"<button class="danger" type="submit">"#;
const TYPED: &str = r#"name="typed""#;

/// Students `68000000 + n` for `n` in `rows`, each with a live Link.
async fn on_roster(h: &Harness, rows: std::ops::Range<usize>) {
    for n in rows {
        linked_student(
            h,
            &format!("s{n}"),
            &format!("{}", 68_000_000 + n),
            "active",
        )
        .await;
    }
}

async fn preview_page(b: &Browser<'_>, id: &str) -> String {
    b.get(&format!("/admin/roster/{id}")).await.body
}

/// Nothing changed: still previewed, no Job, the roster and Links intact.
async fn unchanged(h: &Harness, app: &TestApp, id: &str, active: i64) {
    assert_eq!(import_text(h, id, "state").await, "previewed");
    assert_eq!(run_jobs(h, app).await, 0, "no Job was queued");
    assert_eq!(int(h, ACTIVE, &[]).await, active);
    assert_eq!(int(h, LIVE_LINKS, &[]).await, active);
    let audited = "SELECT count(*) FROM audit_log WHERE action = 'roster.import'";
    assert_eq!(int(h, audited, &[]).await, 0, "no confirm audit row");
}

#[tokio::test]
async fn an_upsert_preview_keeps_the_plain_confirm() {
    let (h, app) = boot().await;
    let b = signed_in(&h, &app, "Officer").await;
    on_roster(&h, 0..3).await;
    let id = previewed(&b, "a.csv", &csv(0..1), "upsert", false).await;
    let page = preview_page(&b, &id).await;
    assert!(page.contains(">ยืนยันนำเข้า</button>"), "{page}");
    assert!(!page.contains(DANGER) && !page.contains(TYPED), "{page}");
    assert_eq!(confirm_typed(&b, &id, None).await.status, 303);
    assert_eq!(import_text(&h, &id, "state").await, "queued");
}

#[tokio::test]
async fn a_full_replace_that_makes_nobody_left_keeps_the_plain_confirm() {
    let (h, app) = boot().await;
    let b = signed_in(&h, &app, "Officer").await;
    on_roster(&h, 0..3).await;
    let id = previewed(&b, "a.csv", &csv(0..4), "full", false).await;
    let page = preview_page(&b, &id).await;
    assert!(page.contains(">ยืนยันนำเข้า</button>"), "{page}");
    assert!(!page.contains(DANGER) && !page.contains(TYPED), "{page}");
}

#[tokio::test]
async fn a_full_replace_names_its_reach_in_the_danger_style() {
    // 10 on the roster, the file keeps 9 and adds 2: 1 leaves, under the
    // 20 % warning, so no typing.
    let (h, app) = boot().await;
    let b = signed_in(&h, &app, "Officer").await;
    on_roster(&h, 0..10).await;
    let id = previewed(&b, "a.csv", &csv(1..12), "full", false).await;
    let page = preview_page(&b, &id).await;
    assert!(
        page.contains(&format!("{DANGER}ยืนยัน: 1 คนจะพ้นสภาพ</button>")),
        "{page}"
    );
    assert!(!page.contains(TYPED), "{page}");
    assert!(page.contains("Old s0"), "the left list stays");
    assert_eq!(confirm_typed(&b, &id, None).await.status, 303);
    run_jobs(&h, &app).await;
    assert_eq!(import_text(&h, &id, "state").await, "done");
    assert_eq!(int(&h, ACTIVE, &[]).await, 11);
}

#[tokio::test]
async fn a_warned_full_replace_needs_the_reach_typed_and_refuses_anything_else() {
    // 5 on the roster, the file keeps 2: 3 leave (60 %), so it is warned.
    let (h, app) = boot().await;
    let b = signed_in(&h, &app, "Officer").await;
    on_roster(&h, 0..5).await;
    let id = previewed(&b, "a.csv", &csv(0..2), "full", true).await;
    let page = preview_page(&b, &id).await;
    assert!(
        page.contains(&format!("{DANGER}ยืนยัน: 3 คนจะพ้นสภาพ</button>")),
        "{page}"
    );
    assert!(page.contains(TYPED) && page.contains("required"), "{page}");
    assert!(page.contains("พิมพ์ตัวเลข 3 เพื่อยืนยัน"), "{page}");
    // No field, an empty one, a wrong number: refused, nothing changes.
    for typed in [
        None,
        Some(""),
        Some("2"),
        Some("4"),
        Some("yes"),
        Some(TYPED_WORD),
    ] {
        let r = confirm_typed(&b, &id, typed).await;
        assert_eq!(r.status, 303, "{typed:?}: {}", r.body);
        let to = r.header("location").unwrap();
        assert_eq!(to, format!("/admin/roster/{id}?typed=wrong#roster-typed"));
        assert_eq!(flash(&b), "err:kernel.roster.typed_refused");
        unchanged(&h, &app, &id, 5).await;
    }
    // The page says what to do, next to the field.
    let page = preview_page(&b, &format!("{id}?typed=wrong")).await;
    assert!(page.contains(r#"aria-invalid="true""#), "{page}");
    assert!(page.contains("ตัวเลขไม่ตรง พิมพ์ตัวเลข 3"), "{page}");
    // The number confirms.
    let r = confirm_typed(&b, &id, Some("3")).await;
    assert_eq!(
        r.header("location"),
        Some(format!("/admin/roster/{id}").as_str())
    );
    assert_eq!(flash(&b), "ok:kernel.roster.confirmed");
    run_jobs(&h, &app).await;
    assert_eq!(import_text(&h, &id, "state").await, "done");
    assert_eq!(int(&h, ACTIVE, &[]).await, 2);
    assert_eq!(
        int(&h, LIVE_LINKS, &[]).await,
        2,
        "the 3 left lost their Links"
    );
}

#[tokio::test]
async fn the_word_no_longer_confirms_a_warned_full_replace() {
    // #135: "ยืนยัน" and "confirm" are refused; only the number confirms.
    let (h, app) = boot().await;
    let b = signed_in(&h, &app, "Officer").await;
    on_roster(&h, 0..5).await;
    let id = previewed(&b, "a.csv", &csv(0..2), "full", true).await;
    for word in [TYPED_WORD, "confirm", "Confirm"] {
        assert_eq!(confirm_typed(&b, &id, Some(word)).await.status, 303);
        assert_eq!(flash(&b), "err:kernel.roster.typed_refused", "{word}");
        unchanged(&h, &app, &id, 5).await;
    }
    assert_eq!(confirm_typed(&b, &id, Some("3")).await.status, 303);
    assert_eq!(flash(&b), "ok:kernel.roster.confirmed");
    assert_eq!(import_text(&h, &id, "state").await, "queued");
}

#[tokio::test]
async fn a_full_only_deployment_without_a_default_mode_still_builds_and_asks() {
    // An existing `modes = ["full"]` config without `default_mode` keeps
    // building (#120 is not a breaking change): its default is `full`.
    let toml = CONFIG.replacen("[roster]\n", "[roster]\nmodes = [\"full\"]\n", 1);
    let config = DeploymentConfig::from_toml(&toml).unwrap();
    assert_eq!(config.roster.default_mode, "full");
    let h = Harness::new();
    let app: TestApp = h.boot(h.builder().config(config).build().unwrap()).await;
    let b = signed_in(&h, &app, "Officer").await;
    let page = b.get("/admin/roster").await.body;
    assert!(page.contains(r#"value="full" checked"#), "{page}");
    // The full replace still names its reach and asks for it typed.
    on_roster(&h, 0..5).await;
    let id = previewed(&b, "a.csv", &csv(0..2), "full", true).await;
    let page = preview_page(&b, &id).await;
    assert!(
        page.contains(&format!("{DANGER}ยืนยัน: 3 คนจะพ้นสภาพ</button>")),
        "{page}"
    );
    assert!(page.contains(TYPED), "{page}");
    assert_eq!(confirm_typed(&b, &id, None).await.status, 303);
    assert_eq!(flash(&b), "err:kernel.roster.typed_refused");
    unchanged(&h, &app, &id, 5).await;
    confirm_typed(&b, &id, Some("3")).await;
    assert_eq!(import_text(&h, &id, "state").await, "queued");
}

//! The Quota banner (#26): on every back-office page for any Staff member
//! once a Channel reaches 80 %, red at 100 %, linking to the usage page
//! only for holders of `kernel.usage.view`, gone in a new month.

mod usage_support;

use usage_support::*;

#[tokio::test]
async fn appears_at_80_percent_for_any_staff_and_links_only_with_the_permission() {
    let (h, app) = boot().await;
    let viewer = signed_in(&h, &app, "viewer", "Viewer").await;
    let officer = signed_in(&h, &app, "officer", "Officer").await;

    use_percent(&h, 79).await;
    for b in [&viewer, &officer] {
        let r = b.get("/admin").await;
        assert_eq!(banner(&r.body), "<div id=\"usage-banner\"></div>");
    }

    use_percent(&h, 80).await;
    // The Viewer cannot open the usage page but sees the banner, as text.
    let r = viewer.get("/admin/students").await;
    assert_eq!(r.status, 200);
    let v = banner(&r.body);
    assert!(v.contains("class=\"usage-banner warn\""), "{v}");
    assert!(v.contains("fake ใช้ไป 80% ของโควตาเดือนนี้"), "{v}");
    assert!(!v.contains("<a "));
    // Also on the 403 page of the usage page itself.
    let r = viewer.get("/admin/usage").await;
    assert_eq!(r.status, 403);
    assert!(banner(&r.body).contains("usage-line warn"));
    // The Officer's banner links to the page.
    let o = officer.get("/admin").await;
    assert!(banner(&o.body).contains("<a href=\"/admin/usage\">fake ใช้ไป 80%"));

    use_percent(&h, 100).await;
    let r = viewer.get("/admin").await;
    let v = banner(&r.body);
    assert!(v.contains("class=\"usage-banner over\""), "{v}");
    assert!(v.contains("usage-line over") && v.contains("ใช้ไป 100% ของโควตาเดือนนี้ เกินโควตาแล้ว"));
}

#[tokio::test]
async fn shows_one_line_per_channel_and_the_highest_level() {
    let (h, app) = boot().await;
    let b = signed_in(&h, &app, "viewer", "Viewer").await;
    use_percent(&h, 85).await;
    seed(&h, "fake2", TODAY, 1_000_000, 0, 0).await; // no Quota: never shown
    let r = b.get("/admin").await;
    let v = banner(&r.body);
    assert_eq!(v.matches("usage-line").count(), 1);
    assert!(v.contains("data-channel=\"fake\"") && !v.contains("fake2"));
    assert!(v.contains("data-level=\"warn\""));
}

#[tokio::test]
async fn a_new_local_month_clears_the_banner() {
    let (h, app) = boot().await;
    seed(&h, "fake", "2026-11-30", CAP, 0, 0).await;
    // 23:30 on 30 November in +07:00 (16:30 UTC): still November.
    h.clock().set_ms(DEC_1_0030 - 3_600_000);
    let b = signed_in(&h, &app, "viewer", "Viewer").await;
    assert!(banner(&b.get("/admin").await.body).contains("usage-line over"));
    // 00:30 on 1 December locally, still 30 November in UTC.
    h.clock().set_ms(DEC_1_0030);
    let r = b.get("/admin").await;
    assert_eq!(r.status, 200, "{}", r.body);
    assert_eq!(banner(&r.body), "<div id=\"usage-banner\"></div>");
}

#[tokio::test]
async fn a_failing_read_shows_no_banner_and_the_page_still_renders() {
    let (h, app) = boot().await;
    let b = signed_in(&h, &app, "viewer", "Viewer").await;
    use_percent(&h, 100).await;
    run(&h, "ALTER TABLE usage_daily RENAME TO usage_daily_gone").await;
    let r = b.get("/admin").await;
    assert_eq!(r.status, 200);
    assert_eq!(banner(&r.body), "<div id=\"usage-banner\"></div>");
    run(&h, "ALTER TABLE usage_daily_gone RENAME TO usage_daily").await;
    assert!(banner(&b.get("/admin").await.body).contains("usage-line over"));
}

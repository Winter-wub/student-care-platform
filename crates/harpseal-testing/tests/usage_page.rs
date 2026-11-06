//! The usage page (#26) at the Deployment seam: this month's metered
//! Pushes against the cap, levels, the 30-day chart and its table, the
//! last 6 months, a Channel without a Quota, and who may open it.

mod usage_support;

use usage_support::*;

/// The `<section>` of Channel `kind` on the page.
fn card<'a>(body: &'a str, kind: &str) -> &'a str {
    let key = format!("id=\"usage-{kind}\"");
    let at = body.find(&key).unwrap_or_else(|| panic!("no card for {kind}"));
    let rest = &body[at..];
    &rest[..rest.find("</section>").unwrap()]
}

#[tokio::test]
async fn shows_this_month_against_the_cap_with_levels() {
    let (h, app) = boot().await;
    let b = signed_in(&h, &app, "officer", "Officer").await;
    for (percent, used, level, width) in [
        (79, "237", "ok", "79"),
        (80, "240", "warn", "80"),
        (100, "300", "over", "100"),
    ] {
        use_percent(&h, percent).await;
        let r = b.get("/admin/usage").await;
        assert_eq!(r.status, 200, "{}", r.body);
        let c = card(&r.body, "fake");
        assert!(c.contains(&format!("data-level=\"{level}\"")), "{percent}: {c}");
        assert!(
            c.contains(&format!("ใช้ไป {used} จาก 300 ({percent}%)")),
            "{percent}: {c}"
        );
        assert!(c.contains(&format!("class=\"meter {level}\"")));
        assert!(c.contains(&format!("style=\"width:{width}%\"")));
    }
    // Over 100 %: the number goes on, the bar stops at full.
    use_percent(&h, 150).await;
    let r = b.get("/admin/usage").await;
    let c = card(&r.body, "fake");
    assert!(c.contains("ใช้ไป 450 จาก 300 (150%)"));
    assert!(c.contains("style=\"width:100%\""));
}

#[tokio::test]
async fn sums_only_the_local_calendar_month_with_free_and_failed() {
    let (h, app) = boot().await;
    seed(&h, "fake", "2026-10-31", 100, 1, 1).await;
    seed(&h, "fake", "2026-11-01", 20, 3, 2).await;
    seed(&h, "fake", TODAY, 10, 4, 5).await;
    let b = signed_in(&h, &app, "officer", "Officer").await;
    let r = b.get("/admin/usage").await;
    let c = card(&r.body, "fake");
    assert!(c.contains("เดือนนี้ (2026-11)"));
    assert!(c.contains("ใช้ไป 30 จาก 300 (10%)"));
    assert!(c.contains("<dd class=\"metered\">30</dd>"));
    assert!(c.contains("<dd class=\"free\">7</dd>"));
    assert!(c.contains("<dd class=\"failed\">7</dd>"));
    // The last 6 months, newest first, zeros for months without rows.
    for (month, row) in [
        ("2026-11", "30</td><td data-label=\"ข้อความตอบกลับ (ฟรี)\">7"),
        ("2026-10", "100</td><td data-label=\"ข้อความตอบกลับ (ฟรี)\">1"),
        ("2026-06", "0</td><td data-label=\"ข้อความตอบกลับ (ฟรี)\">0"),
    ] {
        let at = c.find(&format!("<tr data-month=\"{month}\">")).unwrap();
        assert!(c[at..].contains(row), "{month}");
    }
    assert_eq!(c.matches("<tr data-month=").count(), 6);
    assert!(!c.contains("data-month=\"2026-05\""));
}

#[tokio::test]
async fn the_chart_has_thirty_local_days_and_a_matching_table() {
    let (h, app) = boot().await;
    seed(&h, "fake", "2026-10-06", 99, 0, 0).await; // 31 days ago: not drawn
    seed(&h, "fake", "2026-10-07", 12, 1, 2).await; // the oldest bar
    seed(&h, "fake", TODAY, 30, 0, 10).await;
    let b = signed_in(&h, &app, "officer", "Officer").await;
    let r = b.get("/admin/usage").await;
    let c = card(&r.body, "fake");
    assert_eq!(c.matches("class=\"bar-m\"").count(), 30);
    assert_eq!(c.matches("class=\"bar-f\"").count(), 30);
    assert_eq!(c.matches("<tr data-day=").count(), 30);
    assert!(!c.contains("2026-10-06"));
    assert!(c.contains("<g data-day=\"2026-10-07\">"));
    assert!(c.contains("<g data-day=\"2026-11-05\">"));
    let row = |day: &str| {
        let at = c.find(&format!("<tr data-day=\"{day}\">")).unwrap();
        c[at..at + c[at..].find("</tr>").unwrap()].to_string()
    };
    assert!(row(TODAY).contains(">30</td>") && row(TODAY).contains(">10</td>"));
    assert!(row("2026-10-07").contains(">12</td>"));
    assert!(row("2026-10-20").matches(">0</td>").count() == 3);
    // The highest day (30 + 10) fills the plot; failed is stacked on top.
    assert!(c.contains("x=\"291\" y=\"25.0\" width=\"8\" height=\"75.0\""));
    assert!(c.contains("aria-label=\"30 วันล่าสุด (fake)\""));
}

#[tokio::test]
async fn a_channel_without_a_quota_shows_usage_and_never_warns() {
    let (h, app) = boot().await;
    seed(&h, "fake2", TODAY, 5_000, 2, 1).await;
    let b = signed_in(&h, &app, "officer", "Officer").await;
    let r = b.get("/admin/usage").await;
    let c = card(&r.body, "fake2");
    assert!(c.contains("ไม่ได้ตั้งโควตา: ใช้ไป 5000"), "{c}");
    assert!(c.contains("data-level=\"ok\"") && !c.contains("role=\"progressbar\""), "{c}");
    assert_eq!(banner(&r.body), "<div id=\"usage-banner\"></div>");
    // Both Channels, in registration order.
    assert!(r.body.find("usage-fake\"").unwrap() < r.body.find("usage-fake2\"").unwrap());
}

#[tokio::test]
async fn only_holders_of_the_permission_open_it_and_reads_are_not_audited() {
    let (h, app) = boot().await;
    let viewer = signed_in(&h, &app, "viewer", "Viewer").await;
    let r = viewer.get("/admin/usage").await;
    assert_eq!(r.status, 403);
    assert!(r.body.contains("kernel.usage.view"));
    assert!(!viewer.get("/admin").await.body.contains("href=\"/admin/usage\""));
    let officer = signed_in(&h, &app, "officer", "Officer").await;
    let home = officer.get("/admin").await;
    assert!(home.body.contains("href=\"/admin/usage\""));
    let before = int(&h, "SELECT count(*) FROM audit_log").await;
    assert_eq!(officer.get("/admin/usage").await.status, 200);
    assert_eq!(int(&h, "SELECT count(*) FROM audit_log").await, before);
    // Without a session: to the sign-in page.
    assert_eq!(app.get("/admin/usage").await.status, 303);
}

#[tokio::test]
async fn the_page_works_in_english() {
    let (h, app) = boot().await;
    use_percent(&h, 80).await;
    let mut s = harpseal::NewStaff::new(
        &email_of("english"),
        "English Officer",
        PASSWORD,
        harpseal::Locale::en(),
    );
    s.roles = vec!["Officer".into()];
    h.create_staff_with(s).await;
    let b = app.browser();
    b.login(&email_of("english"), PASSWORD).await;
    let r = b.get("/admin/usage").await;
    assert!(r.body.contains("240 of 300 used (80%)"), "{}", r.body);
    assert!(banner(&r.body).contains("fake has used 80% of this month's Quota"));
}

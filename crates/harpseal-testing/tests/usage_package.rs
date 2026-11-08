//! The LINE package chosen in the back office sets the message Quota
//! (#115): the TOML number is the default and the Custom value until a
//! save, a package changes the limit at once (usage page, banner, watch
//! Job, tick), and Custom is validated. The Permission, CSRF, audit and
//! backup tests are in `usage_package_guard.rs`.

mod usage_support;

use harpseal::Timestamp;
use harpseal_testing::START_MS;
use usage_support::package::*;
use usage_support::*;

#[tokio::test]
async fn the_toml_number_is_the_default_and_the_custom_value_until_a_save() {
    let (h, app) = boot_line(true).await;
    let b = admin(&h, &app).await;
    seed(&h, "line", TODAY, 240, 0, 0).await;
    let body = b.get("/admin/usage").await.body;
    let c = card(&body, "line");
    assert!(c.contains("ใช้ไป 240 จาก 300 (80%)"), "{c}");
    assert!(
        c.contains("ยังไม่ได้เลือกแพ็กเกจ ใช้ค่าจากไฟล์ตั้งค่า: 300 ข้อความต่อเดือน"),
        "{c}"
    );
    // The form starts at "use the configured value", and Custom at the TOML number.
    assert!(c.contains("<option value=\"config\" selected>"), "{c}");
    assert!(
        c.contains("name=\"custom\"") && c.contains("value=\"300\""),
        "{c}"
    );
    assert_eq!(stored(&h).await, None);

    // Without [quotas.line] there is no Quota and no number to start from.
    let (h, app) = boot_line(false).await;
    let b = admin(&h, &app).await;
    let c = b.get("/admin/usage").await.body;
    let c = card(&c, "line");
    assert!(c.contains("ยังไม่ได้เลือกแพ็กเกจ และไม่ได้ตั้งโควตา"), "{c}");
    assert!(c.contains("ไม่ได้ตั้งโควตา: ใช้ไป 0"), "{c}");
}

#[tokio::test]
async fn choosing_a_package_changes_the_limit_everywhere() {
    let (h, app) = boot_line(true).await;
    let b = admin(&h, &app).await;
    seed(&h, "line", TODAY, 1000, 0, 0).await;
    // 1000 of 300: over, and the banner says so.
    let body = b.get("/admin/usage").await.body;
    assert!(card(&body, "line").contains("data-level=\"over\""));
    assert!(banner(&body).contains("usage-banner over"));

    assert_eq!(
        choose(&b, "basic", "").await,
        "ok:kernel.usage.package.saved"
    );
    assert_eq!(stored(&h).await.as_deref(), Some("basic"));
    let body = b.get("/admin/usage").await.body;
    let c = card(&body, "line");
    assert!(c.contains("แพ็กเกจ Basic: 15000 ข้อความต่อเดือน"), "{c}");
    assert!(c.contains("ซื้อข้อความเพิ่มได้"), "{c}");
    assert!(c.contains("ใช้ไป 1000 จาก 15000 (6%)"), "{c}");
    assert!(c.contains("data-level=\"ok\""), "{c}");
    assert!(c.contains("<option value=\"basic\" selected>"), "{c}");
    assert_eq!(banner(&body), "<div id=\"usage-banner\"></div>");
    // The source and its date are on the page.
    assert!(c.contains("lineforbusiness.com/th/service/line-oa-features/broadcast-message"));
    assert!(c.contains("2026-11-08"));

    // Free has the same limit as the TOML but is another choice, and says
    // extra messages cannot be bought; Pro raises the limit.
    assert_eq!(
        choose(&b, "free", "").await,
        "ok:kernel.usage.package.saved"
    );
    let body = b.get("/admin/usage").await.body;
    let c = card(&body, "line");
    assert!(c.contains("แพ็กเกจ Free: 300 ข้อความต่อเดือน"), "{c}");
    assert!(c.contains("ซื้อข้อความเพิ่มไม่ได้"), "{c}");
    assert!(c.contains("data-level=\"over\""), "{c}");
    assert_eq!(choose(&b, "pro", "").await, "ok:kernel.usage.package.saved");
    let body = b.get("/admin/usage").await.body;
    assert!(card(&body, "line").contains("ใช้ไป 1000 จาก 35000 (2%)"));
}

#[tokio::test]
async fn custom_is_a_validated_whole_number_and_unknown_packages_are_refused() {
    let (h, app) = boot_line(true).await;
    let b = admin(&h, &app).await;
    for bad in ["", "abc", "0", "-5", "1.5", "1,500", "100000001", "๕๐๐"] {
        let f = choose(&b, "custom", bad).await;
        assert_eq!(f, "err:kernel.usage.package.err_custom", "{bad:?}");
    }
    assert_eq!(
        choose(&b, "platinum", "5").await,
        "err:kernel.usage.package.err_unknown"
    );
    // A Custom number is ignored when a package is chosen.
    assert_eq!(stored(&h).await, None);
    assert_eq!(
        choose(&b, "basic", "garbage").await,
        "ok:kernel.usage.package.saved"
    );

    assert_eq!(
        choose(&b, "custom", " 2500 ").await,
        "ok:kernel.usage.package.saved"
    );
    assert_eq!(stored(&h).await.as_deref(), Some("custom:2500"));
    let body = b.get("/admin/usage").await.body;
    let c = card(&body, "line");
    assert!(c.contains("กำหนดเอง: 2500 ข้อความต่อเดือน"), "{c}");
    assert!(c.contains("ใช้ไป 0 จาก 2500 (0%)"), "{c}");
    assert!(c.contains("<option value=\"custom\" selected>"), "{c}");
    assert!(c.contains("value=\"2500\""), "{c}");

    // Nothing changes when the choice is what is in force.
    assert_eq!(
        choose(&b, "custom", "2500").await,
        "err:kernel.usage.package.same"
    );
    assert_eq!(audits(&h, "setting.update").await, 2);
}

#[tokio::test]
async fn the_toml_number_chosen_again_as_custom_changes_nothing() {
    let (h, app) = boot_line(true).await;
    let b = admin(&h, &app).await;
    assert_eq!(
        choose(&b, "custom", "300").await,
        "err:kernel.usage.package.same"
    );
    assert_eq!(stored(&h).await, None);
    assert_eq!(audits(&h, "setting.update").await, 0);
}

#[tokio::test]
async fn switching_in_mid_month_measures_the_month_so_far_against_the_new_limit() {
    let (h, app) = boot_line(true).await;
    staff(&h, "boss", &["Administrator"]).await;
    let b = admin(&h, &app).await;
    seed(&h, "line", "2026-11-01", 700, 0, 0).await;
    seed(&h, "line", TODAY, 300, 0, 0).await; // 1000 so far, 300 allowed

    // The remaining budget under each limit: the month is not reset.
    for (package, custom, shows, level) in [
        ("basic", "", "ใช้ไป 1000 จาก 15000 (6%)", "ok"),
        ("custom", "1100", "ใช้ไป 1000 จาก 1100 (90%)", "warn"),
        ("custom", "1000", "ใช้ไป 1000 จาก 1000 (100%)", "over"),
        ("pro", "", "ใช้ไป 1000 จาก 35000 (2%)", "ok"),
    ] {
        assert_eq!(
            choose(&b, package, custom).await,
            "ok:kernel.usage.package.saved"
        );
        let body = b.get("/admin/usage").await.body;
        let c = card(&body, "line");
        assert!(c.contains(shows), "{package} {custom}: {c}");
        assert!(
            c.contains(&format!("data-level=\"{level}\"")),
            "{package}: {c}"
        );
        assert_eq!(
            banner(&body).contains("usage-line"),
            level != "ok",
            "{package} {custom}"
        );
    }

    // The watch Job reads the limit when it runs: no email under Pro, then
    // one to each of the two Administrators under a Custom 1100 (90 %), and
    // again at 100 %.
    watch(&app).await;
    assert!(h.emails().is_empty());
    choose(&b, "custom", "1100").await;
    watch(&app).await;
    assert_eq!(h.emails().len(), 2);
    choose(&b, "custom", "1000").await;
    watch(&app).await;
    assert_eq!(h.emails().len(), 4);
}

#[tokio::test]
async fn a_package_alone_starts_the_watch_on_a_deployment_without_quotas() {
    let (h, app) = boot_line(false).await;
    let b = admin(&h, &app).await;
    let d = app.deployment();
    let r = d.tick(Timestamp(START_MS + 3 * 60_000)).await.unwrap();
    assert_eq!((r.scheduled, r.claimed), (0, 0), "no Quota, no watch");
    choose(&b, "basic", "").await;
    let r = d.tick(Timestamp(START_MS + 13 * 60_000)).await.unwrap();
    assert_eq!((r.scheduled, r.done, r.failed), (1, 1, 0));
}

#[tokio::test]
async fn a_package_switch_starts_the_quota_emails_over() {
    let (h, app) = boot_line(true).await;
    let b = admin(&h, &app).await; // the only Administrator: one email per level
    // Over under Free (300 of 300) emails once.
    choose(&b, "free", "").await;
    seed(&h, "line", TODAY, 300, 0, 0).await;
    watch(&app).await;
    watch(&app).await;
    assert_eq!(h.emails().len(), 1);
    assert!(
        h.emails()[0].subject.contains("100%"),
        "{}",
        h.emails()[0].subject
    );

    // Upgrade: 300 of 15,000 is fine, and the old Over no longer blocks.
    choose(&b, "basic", "").await;
    watch(&app).await;
    assert_eq!(h.emails().len(), 1);
    seed(&h, "line", TODAY, 12_000, 0, 0).await; // 80 % of Basic
    watch(&app).await;
    watch(&app).await;
    assert_eq!(h.emails().len(), 2, "Warn of Basic");
    assert!(
        h.emails()[1].subject.contains("80%"),
        "{}",
        h.emails()[1].subject
    );
    seed(&h, "line", TODAY, 15_000, 0, 0).await;
    watch(&app).await;
    watch(&app).await;
    assert_eq!(h.emails().len(), 3, "Over of Basic");
    assert!(
        h.emails()[2].subject.contains("100%"),
        "{}",
        h.emails()[2].subject
    );

    // The record went with the switch, in the batch of the audit row.
    choose(&b, "pro", "").await;
    assert_eq!(
        int(
            &h,
            "SELECT count(*) FROM setting WHERE key = 'kernel.usage.notified.line'"
        )
        .await,
        0
    );
}

#[tokio::test]
async fn the_banner_and_the_tick_do_not_add_package_reads() {
    let (h, app, spy) = usage_support::spy::boot_spied().await;
    let b = signed_in(&h, &app, "viewer", "Viewer").await;
    use_percent(&h, 100).await;
    let before = spy.usage_reads();
    let r = b.get("/admin/students").await;
    assert!(banner(&r.body).contains("usage-line over"));
    // One query carries the usage rows and the package row (one D1 call).
    assert_eq!(spy.usage_reads(), before + 1);
    assert_eq!(spy.lone_package_reads(), 0);
    // `[quotas]` is set: the tick does not look at the package row.
    app.deployment()
        .tick(Timestamp(START_MS + 3 * 60_000))
        .await
        .unwrap();
    assert_eq!(spy.lone_package_reads(), 0);
}

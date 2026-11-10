//! Quota numbers read as Thai Staff read them (#141): `15,000`, not
//! `15000`, on the back-office home's quota card, the usage page and the
//! announcement editor's monthly count.

mod announcements_support;
mod usage_support;

use harpseal::DeploymentConfig;
use usage_support::{TODAY, seed, signed_in};

const QUOTA: &str = "[quotas.fake]\nmetered_per_month = 15000\n";

#[tokio::test]
async fn the_home_card_and_the_usage_page_group_thousands() {
    let h = harpseal_testing::Harness::new();
    let toml = usage_support::CONFIG.replace("[quotas.fake]\nmetered_per_month = 300\n", QUOTA);
    assert!(toml.contains("15000"), "the support config changed");
    let config = DeploymentConfig::from_toml(&toml).unwrap();
    let app = h.boot(h.builder().config(config).build().unwrap()).await;
    seed(&h, "fake", TODAY, 12_345, 1_500, 2_000).await;
    let b = signed_in(&h, &app, "officer", "Officer").await;

    let home = b.get("/admin").await.body;
    assert!(
        home.contains("เดือนนี้ใช้ไป 12,345 จาก 15,000 ข้อความ (82%)"),
        "{home}"
    );
    let usage = b.get("/admin/usage").await.body;
    assert!(usage.contains("ใช้ไป 12,345 จาก 15,000 (82%)"), "{usage}");
    assert!(usage.contains("<dd class=\"free\">1,500</dd>"), "{usage}");
    assert!(!home.contains("15000") && !usage.contains("15000"));
}

#[tokio::test]
async fn the_announcement_editor_and_review_group_thousands() {
    use announcements_support::notice::sent;
    use announcements_support::{CONFIG, boot_config, form, roster, signed_in};
    let src = format!("{CONFIG}\n{QUOTA}");
    let w = boot_config(&src, 500).await;
    roster(&w).await;
    let b = signed_in(&w, "Officer").await;
    let id = sent(&w, &b).await;
    // This month's usage past a thousand: 1,234 of 15,000, 13,766 left.
    seed(&w.h, "fake", TODAY, 1_234, 0, 0).await;

    // The editor's notice box (edit of a sent announcement).
    let page = b.get(&format!("/admin/announcements/{id}/edit")).await.body;
    let cost = &page[page.find("id=\"notify-cost\"").expect("cost shown")..];
    assert!(cost.contains("ใช้ไปแล้ว 1,234/15,000 เดือนนี้"), "{cost}");

    // The review step before a send to everyone.
    let r = b
        .post(
            "/admin/announcements/new",
            &form("ย้ายห้องสอบ", "ห้อง 3105", "&scope=everyone&do=confirm"),
        )
        .await;
    assert_eq!(r.status, 303, "{}", r.body);
    let review = b.get(r.header("location").unwrap()).await.body;
    assert!(review.contains("จากโควตาเดือนนี้ที่เหลือ 13,766"), "{review}");
    assert!(!review.contains("13766"), "{review}");
}

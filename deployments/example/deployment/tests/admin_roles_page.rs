//! `/admin/roles` (#110) offers every Permission the example declares, the
//! Kernel's and each Module's, grouped by Module.

use example_deployment::{Secrets, builder};
use harpseal_testing::Harness;

const PASSWORD: &str = "kelp forest at dawn";

fn secrets() -> Secrets {
    Secrets::from_lookup(|k| match k {
        "LINE_LIFF_ID" => Some("1234567890-AbCdEfGh".into()),
        "TELEGRAM_WEB_APP_URL" => Some("https://app.seal-bay.invalid/tg".into()),
        _ => None,
    })
}

#[tokio::test]
async fn the_roles_page_lists_every_declared_permission() {
    let h = Harness::new();
    let d = builder(h.services(), secrets())
        .unwrap()
        .channel(h.fake_channel())
        .build()
        .expect("the example Deployment builds");
    let app = h.boot(d).await;
    h.create_staff("admin@seal-bay.invalid", PASSWORD, &["Administrator"])
        .await;
    let b = app.browser();
    assert_eq!(
        b.login("admin@seal-bay.invalid", PASSWORD).await.status,
        303
    );
    let page = b.get("/admin/roles").await;
    assert_eq!(page.status, 200);

    let declared = app.deployment().permissions();
    let mut modules: Vec<&str> = declared
        .iter()
        .map(|p| p.id.split('.').next().unwrap())
        .collect();
    modules.dedup();
    // Each Module's Permissions sit under a Thai heading (#125): the
    // Kernel's own, else the title of the Module's first page.
    for (m, legend) in [
        ("kernel", "ระบบหลัก"),
        ("announcements", "ประกาศข่าว"),
        ("fees", "ยอดค้างชำระ"),
        ("reminders", "เตือนชำระค่าธรรมเนียม"),
    ] {
        assert!(modules.contains(&m), "{m} declares Permissions");
        let legend = format!("<legend>{legend}</legend>");
        assert!(page.body.contains(&legend), "{m} grouped under {legend}");
    }
    // Every Kernel and example Permission reads as a Thai sentence; its id
    // is only the checkbox value and in the technical details.
    let (main, tech) = page
        .body
        .split_once("<details class=\"tech\">")
        .expect("disclosure");
    for p in declared {
        let checkbox = format!("name=\"permission\" value=\"{}\"", p.id);
        assert!(main.contains(&checkbox), "no checkbox for {}", p.id);
        let thai = p
            .thai
            .unwrap_or_else(|| panic!("{} has no Thai sentence", p.id));
        assert!(
            main.contains(&format!("<span>{thai}</span>")),
            "{} reads {thai}",
            p.id
        );
        assert!(!main.contains(&format!(">{}<", p.id)), "{} shown raw", p.id);
        assert!(
            tech.contains(&format!(">{}<", p.id)),
            "{} in the details",
            p.id
        );
    }
}

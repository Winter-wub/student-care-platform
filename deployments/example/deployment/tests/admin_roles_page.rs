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
    for m in ["kernel", "announcements", "fees", "reminders"] {
        assert!(modules.contains(&m), "{m} declares Permissions");
        let legend = format!("<legend class=\"code\">{m}</legend>");
        assert!(page.body.contains(&legend), "grouped under {m}");
    }
    for p in declared {
        let checkbox = format!("name=\"permission\" value=\"{}\"", p.id);
        assert!(page.body.contains(&checkbox), "no checkbox for {}", p.id);
    }
}

//! The example Deployment boots on the harness and serves its pages.

use example_deployment::{Secrets, builder};
use harpseal::{ActionId, Inbound, Message};
use harpseal_channel_fake::FakeChannel;
use harpseal_testing::Harness;

/// Secrets with the Mini App ids the Announcements Module needs.
fn secrets() -> Secrets {
    Secrets::from_lookup(|k| match k {
        "LINE_LIFF_ID" => Some("1234567890-AbCdEfGh".into()),
        "TELEGRAM_WEB_APP_URL" => Some("https://app.seal-bay.invalid/tg".into()),
        _ => None,
    })
}

async fn boot(h: &Harness) -> harpseal_testing::TestApp {
    let d = builder(h.services(), secrets())
        .channel(h.fake_channel())
        .build()
        .expect("the example Deployment builds");
    h.boot(d).await
}

#[tokio::test]
async fn example_boots_runs_migrations_and_serves_notice() {
    let h = Harness::new();
    let app = boot(&h).await;
    let r = app.get("/notice").await;
    assert_eq!(r.status, 200);
    assert!(r.body.contains("มหาวิทยาลัยอ่าวแมวน้ำ"), "TOML text wins");
    assert!(r.body.contains("Seal Bay University"));
    let applied = app.query("SELECT count(*) FROM migration").await;
    // Twelve Kernel migrations and the Announcements Module's one.
    assert_eq!(applied.rows[0][0], harpseal::Value::Integer(13));
}

#[test]
fn example_has_three_roles_and_the_contact_menu() {
    let h = Harness::new();
    let d = builder(h.services(), secrets())
        .channel(h.fake_channel())
        .build()
        .unwrap();
    let roles: Vec<&String> = d.config().roles.keys().collect();
    assert_eq!(roles, ["Administrator", "Officer", "Viewer"]);
    let menu = d.menu_for(&harpseal::Locale::th());
    let labels: Vec<&str> = menu.entries.iter().map(|e| e.label.as_str()).collect();
    assert_eq!(labels[..2], ["ข่าวล่าสุด", "ติดต่อเจ้าหน้าที่"]);
    let en = d.menu_for(&harpseal::Locale::en());
    assert_eq!(en.entries[1].label, "Contact Staff");
}

#[tokio::test]
async fn unlinked_student_pressing_contact_gets_the_consent_card() {
    let h = Harness::new();
    let app = boot(&h).await;
    app.fake_webhook([Inbound::action(
        FakeChannel::account("u1"),
        ActionId::new("contact.show").unwrap(),
        None,
    )])
    .await;
    let seen = h.fake_channel().sent_to(&FakeChannel::account("u1"));
    // Modules never see pre-Link Inbounds: the Kernel greets and asks
    // for Consent first.
    assert!(
        matches!(&seen[..], [Message::Text(_), Message::Card(c)] if c.title == "ประกาศความเป็นส่วนตัว")
    );
}

#[test]
fn production_wiring_builds_with_its_own_fake_channel() {
    let h = Harness::new();
    let secrets = Secrets {
        fake_secret: "s3cret".into(),
        ..secrets()
    };
    assert!(example_deployment::deployment(h.services(), secrets).is_ok());
}

#[test]
fn without_mini_app_ids_the_example_refuses_to_start() {
    let h = Harness::new();
    let secrets = Secrets {
        fake_secret: "s3cret".into(),
        ..Secrets::default()
    };
    let err = example_deployment::deployment(h.services(), secrets).unwrap_err();
    assert!(
        matches!(
            err,
            example_deployment::StartError::Build(harpseal::BuildError::MiniAppUnavailable { .. })
        ),
        "{err}"
    );
}

#[test]
fn an_empty_fake_secret_refuses_to_start() {
    let h = Harness::new();
    let err = example_deployment::deployment(h.services(), Secrets::default()).unwrap_err();
    assert!(
        matches!(err, example_deployment::StartError::FakeSecret(_)),
        "{err}"
    );
}

#[tokio::test]
async fn the_example_roster_template_has_the_mapped_headers() {
    let h = Harness::new();
    let app = boot(&h).await;
    h.create_staff(
        "officer@sealbay.example",
        "kelp forest at dawn",
        &["Officer"],
    )
    .await;
    let b = app.browser();
    b.login("officer@sealbay.example", "kelp forest at dawn")
        .await;
    let r = b.get("/admin/roster/template.csv").await;
    assert_eq!(r.status, 200);
    assert_eq!(
        r.body,
        "\u{feff}รหัสนักศึกษา,ชื่อ-สกุล,อีเมล,ภาษา,กลุ่ม,หลักสูตร,ชั้นปี,เกรดเฉลี่ย\r\n"
    );
}

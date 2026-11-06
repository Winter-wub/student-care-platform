//! The example Deployment boots on the harness and serves its pages.

use example_deployment::{Secrets, builder};
use harpseal::{ActionId, Inbound, Message};
use harpseal_channel_fake::FakeChannel;
use harpseal_testing::Harness;

async fn boot(h: &Harness) -> harpseal_testing::TestApp {
    let d = builder(h.services(), Secrets::default())
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
    assert_eq!(applied.rows[0][0], harpseal::Value::Integer(6));
}

#[test]
fn example_has_three_roles_and_the_contact_menu() {
    let h = Harness::new();
    let d = builder(h.services(), Secrets::default())
        .channel(h.fake_channel())
        .build()
        .unwrap();
    let roles: Vec<&String> = d.config().roles.keys().collect();
    assert_eq!(roles, ["Administrator", "Officer", "Viewer"]);
    let menu = d.menu_for(&harpseal::Locale::th());
    assert_eq!(menu.entries[0].label, "ติดต่อเจ้าหน้าที่");
    let en = d.menu_for(&harpseal::Locale::en());
    assert_eq!(en.entries[0].label, "Contact Staff");
}

#[tokio::test]
async fn unlinked_student_pressing_contact_gets_the_consent_card() {
    let h = Harness::new();
    let app = boot(&h).await;
    app.fake_webhook([Inbound::Action {
        from: FakeChannel::account("u1"),
        action: ActionId::new("contact.show").unwrap(),
        reply: None,
        event_id: None,
        redelivery: false,
    }])
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
        ..Secrets::default()
    };
    assert!(example_deployment::deployment(h.services(), secrets).is_ok());
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

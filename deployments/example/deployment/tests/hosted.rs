//! The Worker's Deployment (#133): no Fake Channel by default; a hosted
//! demo opts in with `HARPSEAL_FAKE_DEMO=1` and a strong `FAKE_SECRET`,
//! the same rule as native dev mode.

use example_deployment::{
    FAKE_DEMO_VAR, PLACEHOLDER_FAKE_SECRET, Secrets, StartError, check_fake_secret,
    hosted_deployment,
};
use harpseal::Inbound;
use harpseal_channel_fake::FakeChannel;
use harpseal_testing::Harness;

const STRONG: &str = "kelp-forest-at-dawn-0042";

/// Secrets with the Mini App ids the Announcements Module needs, and
/// `fake` as `FAKE_SECRET`.
fn secrets(fake: &str) -> Secrets {
    Secrets {
        fake_secret: fake.into(),
        ..Secrets::from_lookup(|k| match k {
            "LINE_LIFF_ID" => Some("1234567890-AbCdEfGh".into()),
            "TELEGRAM_WEB_APP_URL" => Some("https://app.seal-bay.invalid/tg".into()),
            _ => None,
        })
    }
}

#[tokio::test]
async fn by_default_the_worker_registers_no_fake_channel() {
    let h = Harness::new();
    // Even with a strong secret set, and without one at all.
    for fake in [STRONG, ""] {
        let d = hosted_deployment(h.services(), secrets(fake), false).unwrap();
        let wired = format!("{d:?}");
        assert!(wired.contains("\"line\"") && wired.contains("\"telegram\""));
        assert!(!wired.contains("\"fake\""), "{wired}");
        assert!(d.channel("fake").is_none());
    }
    let d = hosted_deployment(h.services(), secrets(STRONG), false).unwrap();
    let app = h.boot(d).await;
    let ev = Inbound::follow(FakeChannel::account("u1"), None);
    assert_eq!(app.fake_webhook([ev]).await.status, 404);
    assert_eq!(app.count("SELECT count(*) FROM channel_account").await, 0);
}

#[test]
fn the_demo_opt_in_needs_a_strong_fake_secret() {
    let h = Harness::new();
    for weak in ["", "s3cret", PLACEHOLDER_FAKE_SECRET, "fifteen-chars-x"] {
        let err = hosted_deployment(h.services(), secrets(weak), true).unwrap_err();
        assert!(
            matches!(err, StartError::WeakFakeSecret(_)),
            "{weak}: {err}"
        );
        assert!(err.to_string().contains(FAKE_DEMO_VAR), "{err}");
    }
    assert!(check_fake_secret("sixteen-chars-xx").is_ok());
}

#[tokio::test]
async fn the_demo_opt_in_registers_the_fake_channel_in_dev_mode() {
    let h = Harness::new();
    let d = hosted_deployment(h.services(), secrets(STRONG), true).unwrap();
    assert!(d.channel("fake").is_some(), "it takes traffic");
    // The harness signs with its own Fake secret: wire one with it.
    let app = h.boot(d).await;
    let ev = Inbound::follow(FakeChannel::account("u1"), None);
    assert_eq!(app.fake_webhook([ev]).await.status, 401, "wrong secret");
}

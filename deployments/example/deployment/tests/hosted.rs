//! The Worker's Deployment (#133): no Fake Channel by default; a hosted
//! demo opts in with `HARPSEAL_FAKE_DEMO=1` and a strong `FAKE_SECRET`,
//! the same rule as native dev mode.

use example_deployment::{
    FAKE_DEMO_VAR, PLACEHOLDER_FAKE_SECRET, Secrets, StartError, check_fake_secret, fake_demo,
    hosted_deployment,
};
use harpseal::{Inbound, Statement};
use harpseal_channel_fake::FakeChannel;
use harpseal_testing::{Harness, TestApp, TestResponse};

const STRONG: &str = "kelp-forest-at-dawn-0042";
const PASSWORD: &str = "kelp forest at dawn";

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

/// A follow from `u1`, signed with `secret`, posted as is.
async fn follow_signed_with(app: &TestApp, secret: &str) -> TestResponse {
    let body = FakeChannel::encode(&[Inbound::follow(FakeChannel::account("u1"), None)]);
    let signature = FakeChannel::new(secret).unwrap().sign(&body);
    app.fake_webhook_raw(body, &signature).await
}

#[tokio::test]
async fn by_default_the_worker_registers_no_fake_channel() {
    let h = Harness::new();
    // Even with a strong secret set, and without one at all (a Fake
    // Channel refuses an empty secret: none was built).
    for fake in [STRONG, ""] {
        let d = hosted_deployment(h.services(), secrets(fake), false).unwrap();
        assert!(d.channel("fake").is_none());
        assert!(d.channel("line").is_some() && d.channel("telegram").is_some());
    }
    let d = hosted_deployment(h.services(), secrets(STRONG), false).unwrap();
    let app = h.boot(d).await;
    assert_eq!(follow_signed_with(&app, STRONG).await.status, 404);
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

#[test]
fn the_demo_variable_is_one_or_unset() {
    for off in [None, Some(""), Some("0")] {
        assert!(!fake_demo(off).unwrap(), "{off:?}");
    }
    assert!(fake_demo(Some("1")).unwrap());
    for typo in ["true", "yes", "on", "2"] {
        let err = fake_demo(Some(typo)).unwrap_err();
        assert!(matches!(err, StartError::FakeDemo(_)), "{err}");
        assert!(err.to_string().contains(FAKE_DEMO_VAR), "{err}");
    }
}

#[tokio::test]
async fn the_demo_opt_in_takes_fake_traffic() {
    let h = Harness::new();
    let d = hosted_deployment(h.services(), secrets(STRONG), true).unwrap();
    assert!(d.channel("fake").is_some());
    let app = h.boot(d).await;
    assert_eq!(
        follow_signed_with(&app, "another-strong-secret")
            .await
            .status,
        401
    );
    assert_eq!(follow_signed_with(&app, STRONG).await.status, 200);
    assert_eq!(app.count("SELECT count(*) FROM channel_account").await, 1);
}

#[tokio::test]
async fn an_existing_fake_link_still_shows_and_can_be_revoked_on_the_worker() {
    let h = Harness::new();
    let d = hosted_deployment(h.services(), secrets(""), false).unwrap();
    let app = h.boot(d).await;
    for sql in [
        "INSERT INTO student (id, student_code, display_name, locale, attributes, created_at, \
         updated_at) VALUES ('s1', '68010001', 'Karn Seal', 'th', '{}', 1, 1)",
        "INSERT INTO channel_account (id, channel, external_id, first_seen_at, flow_state, \
         updated_at) VALUES ('a-s1', 'fake', 'u-s1', 1, 'linked', 1)",
        "INSERT INTO link (id, student_id, channel_account_id, channel, status, method, \
         linked_at, updated_at) VALUES ('l-s1', 's1', 'a-s1', 'fake', 'active', 'email_otp', 1, 1)",
    ] {
        h.db().execute(Statement::new(sql)).await.unwrap();
    }
    h.create_staff("officer@sealbay.example", PASSWORD, &["Officer"])
        .await;
    let b = app.browser();
    assert_eq!(
        b.login("officer@sealbay.example", PASSWORD).await.status,
        303
    );
    let list = b.get("/admin/students").await.body;
    assert_eq!(list.matches("fake: ").count(), 1, "{list}");
    let detail = b.get("/admin/students/s1").await.body;
    let path = "/admin/students/s1/links/l-s1/revoke";
    assert!(detail.contains(&format!("action=\"{path}\"")), "{detail}");
    assert!(!detail.contains("<option value=\"fake\">"), "not offered");

    let r = b.post(path, "confirm=yes").await;
    assert_eq!(r.status, 303, "{}", r.body);
    assert_eq!(
        b.cookie("hs_flash").as_deref(),
        Some("ok:kernel.students.revoked_unnotified")
    );
    let revoked = "SELECT count(*) FROM link WHERE status = 'revoked'";
    assert_eq!(app.count(revoked).await, 1);
    assert!(h.fake_channel().sent().is_empty());
}

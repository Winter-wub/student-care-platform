//! Consent across Link changes (#20): a Manual Link asks for it, a Staff
//! revoke keeps only what was accepted on that account, and a re-consent
//! on one Channel counts for the Student's other Channel.

mod support;

use harpseal::{ActionId, ChannelAccountId, ChannelKind, Inbound, Message, RequestCtx, links};
use harpseal_channel_fake::FakeProfile;
use harpseal_testing::Harness;
use support::{Chat, HELLO, Hello, ROSTER, boot, col, live_link, officer, sql, text_of};

const NEW_VERSION: &str = "INSERT INTO setting (key, locale, value, updated_at) \
     VALUES ('kernel.consent.version', '', '2', 1)";

#[tokio::test]
async fn a_revoke_before_the_card_was_accepted_leaves_no_consent_to_inherit() {
    let (h, app) = boot(FakeProfile::Full).await;
    let staff = officer(&h).await;
    let ctx = RequestCtx::default();
    let d = app.deployment();
    let mut c = Chat::new(&h, &app, "u1");
    c.follow().await;
    links::manual_link(d, "s1", "fake", "u1", &staff, &ctx)
        .await
        .unwrap();
    // Revoked before the Student accepted the Card.
    let id = live_link(&h, "u1").await;
    links::revoke_by_staff(d, &id, &staff, &ctx).await.unwrap();
    let acct = app.query("SELECT flow_state FROM channel_account").await;
    assert_eq!(
        col(&acct, "flow_state").as_deref(),
        Some("awaiting_consent")
    );

    // Typing a student id gets the Card, not a code.
    let seen = c.text("68010001").await;
    assert!(matches!(&seen[..], [Message::Card(_)]), "{seen:?}");
    assert!(h.emails().is_empty());
    assert_eq!(app.count("SELECT count(*) FROM consent").await, 0);

    c.press("kernel.consent.accept").await;
    c.text("68010001").await;
    let code = c.code();
    assert!(text_of(&c.text(&code).await[0]).starts_with("เชื่อมต่อสำเร็จ"));
    assert_eq!(
        app.count(
            "SELECT count(*) FROM consent c JOIN channel_account ca \
                   ON ca.id = c.channel_account_id WHERE ca.external_id = 'u1'"
        )
        .await,
        1,
        "recorded once accepted, on this account"
    );
}

#[tokio::test]
async fn consent_from_another_account_skips_the_card_but_is_not_kept_there() {
    let (h, app) = boot(FakeProfile::Full).await;
    let staff = officer(&h).await;
    let ctx = RequestCtx::default();
    let d = app.deployment();
    let mut c1 = Chat::new(&h, &app, "u1");
    c1.link("68010001").await;
    let id = live_link(&h, "u1").await;
    links::revoke_by_staff(d, &id, &staff, &ctx).await.unwrap();

    // s1 accepted the current version on u1: no Card on u2, Menu at once.
    let mut c2 = Chat::new(&h, &app, "u2");
    c2.follow().await;
    h.fake_channel().take_sent();
    links::manual_link(d, "s1", "fake", "u2", &staff, &ctx)
        .await
        .unwrap();
    let seen = c2.seen_since();
    assert!(matches!(&seen[..], [Message::Text(_)]), "{seen:?}");
    assert!(matches!(c2.menu(), Some(Some(_))));
    assert_eq!(text_of(&c2.press("hello.say").await[0]), HELLO);

    // Revoked again: u2 never accepted anything itself, so whoever types a
    // student id there (here another Student) sees the Card first.
    let id = live_link(&h, "u2").await;
    links::revoke_by_staff(d, &id, &staff, &ctx).await.unwrap();
    let seen = c2.text("68010004").await;
    assert!(matches!(&seen[..], [Message::Card(_)]), "{seen:?}");
    assert_eq!(app.count("SELECT count(*) FROM consent").await, 1);
}

#[tokio::test]
async fn a_manual_link_after_a_notice_change_shows_the_updated_card() {
    let (h, app) = boot(FakeProfile::Full).await;
    let staff = officer(&h).await;
    let ctx = RequestCtx::default();
    let d = app.deployment();
    Chat::new(&h, &app, "u1").link("68010001").await;
    let id = live_link(&h, "u1").await;
    links::revoke_by_staff(d, &id, &staff, &ctx).await.unwrap();
    sql(&h, NEW_VERSION).await;
    let mut c2 = Chat::new(&h, &app, "u2");
    c2.follow().await;
    h.fake_channel().take_sent();
    links::manual_link(d, "s1", "fake", "u2", &staff, &ctx)
        .await
        .unwrap();
    let seen = c2.seen_since();
    let [Message::Text(_), Message::Card(card)] = &seen[..] else {
        panic!("{seen:?}")
    };
    assert_eq!(card.title, "ประกาศความเป็นส่วนตัวฉบับใหม่");
}

/// Sends `ev` on the second Fake Channel; what `t1` saw.
async fn on_t1(
    h: &Harness,
    app: &harpseal_testing::TestApp,
    second: &harpseal_channel_fake::FakeChannel,
    ev: Inbound,
) -> Vec<Message> {
    let to = ev.from().clone();
    h.fake_channel().take_sent();
    app.deployment()
        .handle_webhook("fake2", second.webhook(&[ev]))
        .await
        .unwrap();
    h.fake_channel().sent_to(&to)
}

#[tokio::test]
async fn re_consent_on_one_channel_counts_for_the_other() {
    let h = Harness::new().with_fake_profile(FakeProfile::Full);
    let second = h.fake_channel().with_kind("fake2");
    let d = h
        .builder()
        .channel(second.clone())
        .module(Hello)
        .build()
        .unwrap();
    let app = h.boot(d).await;
    for stmt in ROSTER {
        sql(&h, stmt).await;
    }
    let d = app.deployment();
    let t1 = ChannelAccountId::new(ChannelKind("fake2"), "t1");
    let press = |action: &str| Inbound::Action {
        from: t1.clone(),
        action: ActionId::new(action).unwrap(),
        reply: None,
    };
    let mut c = Chat::new(&h, &app, "u1");
    c.link("68010001").await;
    on_t1(
        &h,
        &app,
        &second,
        Inbound::Follow {
            from: t1.clone(),
            reply: None,
        },
    )
    .await;
    let staff = officer(&h).await;
    links::manual_link(d, "s1", "fake2", "t1", &staff, &RequestCtx::default())
        .await
        .unwrap();
    sql(&h, NEW_VERSION).await;

    // Both Channels park on the new Card.
    let seen = c.press("hello.say").await;
    assert!(matches!(&seen[..], [Message::Card(_)]), "{seen:?}");
    let seen = on_t1(&h, &app, &second, press("hello.say")).await;
    assert!(matches!(&seen[..], [Message::Card(_)]), "{seen:?}");

    // Accept on the first; the second replays its parked Action once (the
    // same press is not run twice), no Card.
    let seen = c.press("kernel.consent.accept").await;
    assert_eq!(text_of(&seen[0]), HELLO);
    let seen = on_t1(&h, &app, &second, press("hello.say")).await;
    assert!(
        matches!(&seen[..], [Message::Text(t)] if t == HELLO),
        "{seen:?}"
    );
    assert_eq!(
        app.count("SELECT count(*) FROM consent WHERE notice_version = '2'")
            .await,
        1
    );
    let state = app
        .query("SELECT flow_state FROM channel_account WHERE external_id = 't1'")
        .await;
    assert_eq!(col(&state, "flow_state").as_deref(), Some("linked"));
}

//! Link lifecycle (#20) at the Deployment seam: Staff revoke and Manual
//! Link through `harpseal::links`, re-consent across two Channels, and the
//! Menu without self-unlink. Unreachable Links: `link_unreachable.rs`.

mod support;

use harpseal::{
    Actor, ChannelAccountId, ChannelKind, Error, Inbound, Locale, Message, RequestCtx, links,
};
use harpseal_channel_fake::FakeProfile;
use harpseal_testing::Harness;
use support::{Chat, Hello, ROSTER, actions_of, boot, sql, text_of};

const REVOKED_NOTICE: &str = "เจ้าหน้าที่ยกเลิกการเชื่อมบัญชีนี้แล้ว หากต้องการเชื่อมใหม่ พิมพ์รหัสนักศึกษาได้ทุกเมื่อ";
const HELLO: &str = "สวัสดีจากโมดูล";

async fn staff(h: &Harness) -> Actor {
    Actor::Staff(
        h.create_staff("officer@sealbay.example", "correct horse battery", &[])
            .await,
    )
}

async fn link_id(app: &harpseal_testing::TestApp, status: &'static str) -> String {
    let sql = match status {
        "active" => "SELECT id FROM link WHERE status = 'active'",
        _ => "SELECT id FROM link WHERE status = 'revoked'",
    };
    let rows = app.query(sql).await;
    rows.get(0, "id").unwrap().as_text().unwrap().to_string()
}

fn text(rows: &harpseal::Rows, col: &str) -> Option<String> {
    rows.get(0, col)
        .and_then(|v| v.as_text())
        .map(str::to_string)
}

#[tokio::test]
async fn a_staff_revoke_pushes_a_notice_locks_the_menu_and_keeps_history() {
    let (h, app) = boot(FakeProfile::LineLike).await;
    let officer = staff(&h).await;
    let mut c = Chat::new(&h, &app, "u1");
    c.link("68010001").await;
    let id = link_id(&app, "active").await;
    h.fake_channel().take_sent();

    let d = app.deployment();
    let done = links::revoke_by_staff(d, &id, &officer, &RequestCtx::default())
        .await
        .unwrap();
    assert!(done.notified);
    let seen = c.seen_since();
    assert!(
        matches!(&seen[..], [Message::Text(t)] if t == REVOKED_NOTICE),
        "{seen:?}"
    );
    assert!(matches!(c.menu(), Some(None)), "Menu locked");

    let Actor::Staff(staff_id) = &officer else {
        unreachable!()
    };
    let link = app
        .query("SELECT status, revoked_at, revoked_by FROM link")
        .await;
    assert_eq!(text(&link, "status").as_deref(), Some("revoked"));
    assert!(link.get(0, "revoked_at").unwrap().as_i64().is_some());
    assert_eq!(text(&link, "revoked_by").as_ref(), Some(staff_id));
    let acct = app.query("SELECT flow_state FROM channel_account").await;
    assert_eq!(
        text(&acct, "flow_state").as_deref(),
        Some("awaiting_student_id")
    );
    let audit = app
        .query("SELECT actor_kind, actor_id, target_id FROM audit_log WHERE action = 'link.revoke'")
        .await;
    assert_eq!(audit.len(), 1);
    assert_eq!(text(&audit, "actor_kind").as_deref(), Some("staff"));
    assert_eq!(text(&audit, "actor_id").as_ref(), Some(staff_id));
    assert_eq!(text(&audit, "target_id"), Some(id.clone()));

    // A second revoke changes nothing and writes no second audit row.
    let again = links::revoke_by_staff(d, &id, &officer, &RequestCtx::default()).await;
    assert!(matches!(again, Err(Error::Conflict(_))), "{again:?}");
    assert_eq!(
        app.count("SELECT count(*) FROM audit_log WHERE action = 'link.revoke'")
            .await,
        1
    );
    let unknown = links::revoke_by_staff(d, "nope", &officer, &RequestCtx::default()).await;
    assert!(matches!(unknown, Err(Error::Invalid(_))));

    // Consent kept: straight to the student id, then a new Link; the
    // revoked row stays.
    let seen = c.press("hello.say").await;
    assert_eq!(text_of(&seen[0]), "ขอบคุณครับ กรุณาพิมพ์รหัสนักศึกษาของคุณ");
    c.text("68010001").await;
    let code = c.code();
    assert!(text_of(&c.text(&code).await[0]).starts_with("เชื่อมต่อสำเร็จ"));
    assert_eq!(app.count("SELECT count(*) FROM link").await, 2);
    assert_eq!(
        app.count("SELECT count(*) FROM link WHERE status = 'revoked'")
            .await,
        1
    );
    assert_eq!(app.count("SELECT count(*) FROM consent").await, 1);
}

#[tokio::test]
async fn a_manual_link_asks_for_consent_parks_actions_and_unlocks_after_accept() {
    let (h, app) = boot(FakeProfile::Full).await;
    let officer = staff(&h).await;
    let mut c = Chat::new(&h, &app, "u9");
    let d = app.deployment();

    // s3 has no email: the case Manual Links exist for.
    let done = links::manual_link(d, "s3", "fake", "u9", &officer, &RequestCtx::default())
        .await
        .unwrap();
    assert!(done.notified);
    let seen = c.seen_since();
    let [Message::Text(notice), Message::Card(card)] = &seen[..] else {
        panic!("{seen:?}")
    };
    assert_eq!(
        notice,
        "เจ้าหน้าที่เชื่อมบัญชีนี้กับ Hoi Nangrom ให้แล้ว หากไม่ใช่คุณ กรุณาติดต่อเจ้าหน้าที่"
    );
    assert_eq!(card.title, "ประกาศความเป็นส่วนตัว");
    assert!(actions_of(&seen[1]).contains(&"kernel.consent.accept".to_string()));
    assert!(matches!(c.menu(), Some(None)), "locked until accept");

    let Actor::Staff(staff_id) = &officer else {
        unreachable!()
    };
    let link = app
        .query("SELECT id, status, method, verified_by_staff_id FROM link")
        .await;
    assert_eq!(text(&link, "status").as_deref(), Some("active"));
    assert_eq!(text(&link, "method").as_deref(), Some("manual"));
    assert_eq!(text(&link, "verified_by_staff_id").as_ref(), Some(staff_id));
    assert_eq!(text(&link, "id"), Some(done.link_id.clone()));
    let audit = app
        .query("SELECT actor_kind, target_id FROM audit_log WHERE action = 'link.create'")
        .await;
    assert_eq!(text(&audit, "actor_kind").as_deref(), Some("staff"));
    assert_eq!(text(&audit, "target_id"), Some(done.link_id));

    // An Action before accept is parked behind the Card.
    let seen = c.press("hello.say").await;
    assert!(matches!(&seen[..], [Message::Card(_)]), "{seen:?}");
    assert_eq!(app.count("SELECT count(*) FROM consent").await, 0);

    let seen = c.press("kernel.consent.accept").await;
    assert!(
        matches!(&seen[..], [Message::Text(t)] if t == HELLO),
        "{seen:?}"
    );
    assert!(matches!(c.menu(), Some(Some(_))), "Menu unlocked");
    assert_eq!(
        app.count("SELECT count(*) FROM consent WHERE student_id = 's3'")
            .await,
        1
    );
    let seen = c.press("hello.say").await;
    assert_eq!(text_of(&seen[0]), HELLO);
}

#[tokio::test]
async fn manual_links_keep_one_live_link_per_student_channel_and_account() {
    let (h, app) = boot(FakeProfile::Full).await;
    let officer = staff(&h).await;
    let ctx = RequestCtx::default();
    let mut c = Chat::new(&h, &app, "u1");
    c.link("68010001").await;
    let d = app.deployment();

    let same_student = links::manual_link(d, "s1", "fake", "u2", &officer, &ctx).await;
    assert!(
        matches!(same_student, Err(Error::Conflict(_))),
        "{same_student:?}"
    );
    let same_account = links::manual_link(d, "s4", "fake", "u1", &officer, &ctx).await;
    assert!(
        matches!(same_account, Err(Error::Conflict(_))),
        "{same_account:?}"
    );
    let left = links::manual_link(d, "s2", "fake", "u3", &officer, &ctx).await;
    assert!(matches!(left, Err(Error::Invalid(_))));
    let no_channel = links::manual_link(d, "s4", "nope", "u3", &officer, &ctx).await;
    assert!(matches!(no_channel, Err(Error::Invalid(_))));
    assert_eq!(app.count("SELECT count(*) FROM link").await, 1);
    assert_eq!(
        app.count("SELECT count(*) FROM audit_log WHERE action = 'link.create'")
            .await,
        1,
        "only the Student's own Link"
    );

    // After a revoke the Student can be linked by hand on a new account.
    // Consent for the current version exists: no Card, the Menu unlocks.
    let id = link_id(&app, "active").await;
    links::revoke_by_staff(d, &id, &officer, &ctx)
        .await
        .unwrap();
    let mut c2 = Chat::new(&h, &app, "u2");
    links::manual_link(d, "s1", "fake", "u2", &officer, &ctx)
        .await
        .unwrap();
    let seen = c2.seen_since();
    assert!(matches!(&seen[..], [Message::Text(_)]), "{seen:?}");
    assert!(matches!(c2.menu(), Some(Some(_))));
    assert_eq!(text_of(&c2.press("hello.say").await[0]), HELLO);
    assert_eq!(app.count("SELECT count(*) FROM link").await, 2);
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
    let mut c = Chat::new(&h, &app, "u1");
    c.link("68010001").await;
    let officer = staff(&h).await;
    let d = app.deployment();
    links::manual_link(d, "s1", "fake2", "t1", &officer, &RequestCtx::default())
        .await
        .unwrap();
    let t1 = ChannelAccountId::new(ChannelKind("fake2"), "t1");
    sql(
        &h,
        "INSERT INTO setting (key, locale, value, updated_at) \
         VALUES ('kernel.consent.version', '', '2', 1)",
    )
    .await;

    // Both Channels park on the new Card.
    let seen = c.press("hello.say").await;
    assert!(matches!(&seen[..], [Message::Card(_)]), "{seen:?}");
    let on_t1 = |action: &str| Inbound::Action {
        from: t1.clone(),
        action: harpseal::ActionId::new(action).unwrap(),
        reply: None,
    };
    h.fake_channel().take_sent();
    d.handle_webhook("fake2", second.webhook(&[on_t1("hello.say")]))
        .await
        .unwrap();
    let seen = h.fake_channel().sent_to(&t1);
    assert!(matches!(&seen[..], [Message::Card(_)]), "{seen:?}");

    // Accept on the first; the second replays its parked Action, no Card.
    let seen = c.press("kernel.consent.accept").await;
    assert_eq!(text_of(&seen[0]), HELLO);
    h.fake_channel().take_sent();
    d.handle_webhook("fake2", second.webhook(&[on_t1("hello.say")]))
        .await
        .unwrap();
    let seen = h.fake_channel().sent_to(&t1);
    assert!(
        seen.iter().all(|m| !matches!(m, Message::Card(_))),
        "{seen:?}"
    );
    assert_eq!(text_of(&seen[0]), HELLO);
    assert_eq!(
        app.count("SELECT count(*) FROM consent WHERE notice_version = '2'")
            .await,
        1
    );
    let state = app
        .query("SELECT flow_state FROM channel_account WHERE external_id = 't1'")
        .await;
    assert_eq!(text(&state, "flow_state").as_deref(), Some("linked"));
}

#[tokio::test]
async fn without_self_unlink_the_menu_has_no_unlink_entry() {
    let (h, app) =
        support::boot_with(FakeProfile::Full, "[student]\nself_unlink = false", |s| s).await;
    let mut c = Chat::new(&h, &app, "u1");
    let seen = c.link("68010001").await;
    assert_eq!(actions_of(seen.last().unwrap()), ["hello.say"]);
    let Some(Some(menu)) = c.menu() else {
        panic!("Menu unlocked")
    };
    assert!(
        menu.entries
            .iter()
            .all(|e| e.action.as_str() != "kernel.link.unlink")
    );
    let seen = c.press("kernel.link.unlink").await;
    assert_eq!(
        text_of(&seen[0]),
        app.deployment()
            .catalog()
            .t(&Locale::th(), "kernel.action.expired")
    );
    assert_eq!(
        app.count("SELECT count(*) FROM link WHERE status = 'active'")
            .await,
        1
    );
}

#[tokio::test]
async fn racing_revokes_and_manual_links_change_one_link_once() {
    let (h, app) = boot(FakeProfile::Full).await;
    let officer = staff(&h).await;
    let ctx = RequestCtx::default();
    let mut c = Chat::new(&h, &app, "u1");
    c.link("68010001").await;
    let d = app.deployment();
    let id = link_id(&app, "active").await;

    let (a, b) = tokio::join!(
        links::revoke_by_staff(d, &id, &officer, &ctx),
        links::revoke_by_staff(d, &id, &officer, &ctx),
    );
    assert_eq!(
        [&a, &b].iter().filter(|r| r.is_ok()).count(),
        1,
        "{a:?} {b:?}"
    );
    assert!(
        [a, b]
            .into_iter()
            .any(|r| matches!(r, Err(Error::Conflict(_))))
    );
    assert_eq!(
        app.count("SELECT count(*) FROM audit_log WHERE action = 'link.revoke'")
            .await,
        1
    );

    let (a, b) = tokio::join!(
        links::manual_link(d, "s1", "fake", "u2", &officer, &ctx),
        links::manual_link(d, "s1", "fake", "u3", &officer, &ctx),
    );
    assert_eq!(
        [&a, &b].iter().filter(|r| r.is_ok()).count(),
        1,
        "{a:?} {b:?}"
    );
    assert_eq!(
        app.count("SELECT count(*) FROM link WHERE status <> 'revoked'")
            .await,
        1
    );
    assert_eq!(
        app.count(
            "SELECT count(*) FROM audit_log WHERE action = 'link.create' AND actor_kind = 'staff'"
        )
        .await,
        1
    );
}

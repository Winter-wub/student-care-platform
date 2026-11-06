//! Link lifecycle (#20) at the Deployment seam: Staff revoke and Manual
//! Link through `harpseal::links`, their audit rows, the one-live-Link
//! invariant under races, and the Menu without self-unlink. Consent across
//! Link changes: `link_consent.rs`; unreachable Links: `link_unreachable.rs`.

mod support;

use harpseal::{Actor, Error, Locale, Message, RequestCtx, links};
use harpseal_channel_fake::FakeProfile;
use support::{Chat, HELLO, REVOKED_NOTICE, actions_of, boot, col, live_link, officer, text_of};

#[tokio::test]
async fn a_staff_revoke_pushes_a_notice_locks_the_menu_and_keeps_history() {
    let (h, app) = boot(FakeProfile::LineLike).await;
    let staff = officer(&h).await;
    let mut c = Chat::new(&h, &app, "u1");
    c.link("68010001").await;
    let id = live_link(&h, "u1").await;
    h.fake_channel().take_sent();

    let d = app.deployment();
    let done = links::revoke_by_staff(d, &id, &staff, &RequestCtx::default())
        .await
        .unwrap();
    assert!(done.notified);
    let seen = c.seen_since();
    assert!(
        matches!(&seen[..], [Message::Text(t)] if t == REVOKED_NOTICE),
        "{seen:?}"
    );
    assert!(matches!(c.menu(), Some(None)), "Menu locked");

    let Actor::Staff(staff_id) = &staff else {
        unreachable!()
    };
    let link = app
        .query("SELECT status, revoked_at, revoked_by FROM link")
        .await;
    assert_eq!(col(&link, "status").as_deref(), Some("revoked"));
    assert!(link.get(0, "revoked_at").unwrap().as_i64().is_some());
    assert_eq!(col(&link, "revoked_by").as_ref(), Some(staff_id));
    let acct = app.query("SELECT flow_state FROM channel_account").await;
    assert_eq!(
        col(&acct, "flow_state").as_deref(),
        Some("awaiting_student_id")
    );

    // A second revoke changes nothing and writes no second audit row.
    let again = links::revoke_by_staff(d, &id, &staff, &RequestCtx::default()).await;
    assert!(matches!(again, Err(Error::Conflict(_))), "{again:?}");
    assert_eq!(
        app.count("SELECT count(*) FROM audit_log WHERE action = 'link.revoke'")
            .await,
        1
    );
    let unknown = links::revoke_by_staff(d, "nope", &staff, &RequestCtx::default()).await;
    assert!(matches!(unknown, Err(Error::Invalid(_))));

    // Consent accepted on this account is kept: straight to the student
    // id, then a new Link; the revoked row stays.
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
async fn revoke_audit_rows_carry_actor_before_after_and_the_request() {
    let (h, app) = boot(FakeProfile::Full).await;
    let staff = officer(&h).await;
    let d = app.deployment();
    let mut c = Chat::new(&h, &app, "u1");
    c.link("68010001").await;
    let mut ctx = RequestCtx::default();
    ctx.ip = Some("203.0.113.7".into());
    ctx.user_agent = Some("Seal Bay Browser".into());
    ctx.request_id = Some("req-1".into());
    let id = live_link(&h, "u1").await;
    links::revoke_by_staff(d, &id, &staff, &ctx).await.unwrap();
    let row = app
        .query(
            "SELECT actor_kind, target_kind, target_id, before, after, ip, user_agent, request_id \
             FROM audit_log WHERE action = 'link.revoke'",
        )
        .await;
    assert_eq!(col(&row, "actor_kind").as_deref(), Some("staff"));
    assert_eq!(col(&row, "target_kind").as_deref(), Some("link"));
    assert_eq!(col(&row, "target_id"), Some(id));
    assert_eq!(
        col(&row, "before").as_deref(),
        Some(r#"{"status":"active"}"#)
    );
    assert_eq!(
        col(&row, "after").as_deref(),
        Some(r#"{"status":"revoked"}"#)
    );
    assert_eq!(col(&row, "ip").as_deref(), Some("203.0.113.7"));
    assert_eq!(col(&row, "user_agent").as_deref(), Some("Seal Bay Browser"));
    assert_eq!(col(&row, "request_id").as_deref(), Some("req-1"));

    // The system (the CLI) revokes with no Staff id anywhere.
    let mut c4 = Chat::new(&h, &app, "u4");
    c4.link("68010004").await;
    let id4 = live_link(&h, "u4").await;
    links::revoke_by_staff(d, &id4, &Actor::System, &RequestCtx::default())
        .await
        .unwrap();
    assert_eq!(
        app.count(
            "SELECT count(*) FROM audit_log WHERE action = 'link.revoke' \
             AND actor_kind = 'system' AND actor_id IS NULL"
        )
        .await,
        1
    );
    let link = app
        .query("SELECT revoked_by FROM link l JOIN channel_account ca ON ca.id = l.channel_account_id WHERE ca.external_id = 'u4'")
        .await;
    assert_eq!(col(&link, "revoked_by"), None);
    // The English Student gets the English notice.
    let seen = c4.seen_since();
    assert_eq!(
        text_of(seen.last().unwrap()),
        "Staff have unlinked this account. To link it again, type your student ID at any time."
    );
}

#[tokio::test]
async fn a_manual_link_asks_for_consent_parks_actions_and_unlocks_after_accept() {
    let (h, app) = boot(FakeProfile::Full).await;
    let staff = officer(&h).await;
    let mut c = Chat::new(&h, &app, "u9");
    c.follow().await;
    h.fake_channel().take_sent();
    let d = app.deployment();

    // s3 has no email: the case Manual Links exist for. A pasted id with
    // spaces around it is trimmed.
    let done = links::manual_link(d, "s3", "fake", " u9 ", &staff, &RequestCtx::default())
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
    assert_eq!(app.count("SELECT count(*) FROM channel_account").await, 1);

    let Actor::Staff(staff_id) = &staff else {
        unreachable!()
    };
    let link = app
        .query("SELECT id, status, method, verified_by_staff_id FROM link")
        .await;
    assert_eq!(col(&link, "status").as_deref(), Some("active"));
    assert_eq!(col(&link, "method").as_deref(), Some("manual"));
    assert_eq!(col(&link, "verified_by_staff_id").as_ref(), Some(staff_id));
    assert_eq!(col(&link, "id"), Some(done.link_id.clone()));
    let audit = app
        .query("SELECT actor_kind, target_id FROM audit_log WHERE action = 'link.create'")
        .await;
    assert_eq!(col(&audit, "actor_kind").as_deref(), Some("staff"));
    assert_eq!(col(&audit, "target_id"), Some(done.link_id));

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
async fn manual_links_refuse_bad_input_and_a_second_live_link() {
    let (h, app) = boot(FakeProfile::Full).await;
    let staff = officer(&h).await;
    let ctx = RequestCtx::default();
    let mut c = Chat::new(&h, &app, "u1");
    c.link("68010001").await;
    Chat::new(&h, &app, "u2").follow().await;
    Chat::new(&h, &app, "u3").follow().await;
    let d = app.deployment();

    let invalid = [
        links::manual_link(d, "s4", "fake", "", &staff, &ctx).await,
        links::manual_link(d, "s4", "fake", "   ", &staff, &ctx).await,
        links::manual_link(d, "s4", "fake", "U123", &staff, &ctx).await,
        links::manual_link(d, "nobody", "fake", "u2", &staff, &ctx).await,
        links::manual_link(d, "s2", "fake", "u2", &staff, &ctx).await,
        links::manual_link(d, "s4", "nope", "u2", &staff, &ctx).await,
    ];
    for r in &invalid {
        assert!(matches!(r, Err(Error::Invalid(_))), "{r:?}");
    }
    let same_student = links::manual_link(d, "s1", "fake", "u2", &staff, &ctx).await;
    assert!(
        matches!(same_student, Err(Error::Conflict(_))),
        "{same_student:?}"
    );
    let same_account = links::manual_link(d, "s4", "fake", "u1", &staff, &ctx).await;
    assert!(
        matches!(same_account, Err(Error::Conflict(_))),
        "{same_account:?}"
    );
    assert_eq!(app.count("SELECT count(*) FROM link").await, 1);
    assert_eq!(app.count("SELECT count(*) FROM channel_account").await, 3);
    assert_eq!(
        app.count("SELECT count(*) FROM audit_log WHERE action = 'link.create'")
            .await,
        1,
        "only the Student's own Link"
    );
}

#[tokio::test]
async fn racing_revokes_and_manual_links_change_one_link_once() {
    let (h, app) = boot(FakeProfile::Full).await;
    let staff = officer(&h).await;
    let ctx = RequestCtx::default();
    let mut c = Chat::new(&h, &app, "u1");
    c.link("68010001").await;
    Chat::new(&h, &app, "u2").follow().await;
    Chat::new(&h, &app, "u3").follow().await;
    let d = app.deployment();
    let id = live_link(&h, "u1").await;

    let (a, b) = tokio::join!(
        links::revoke_by_staff(d, &id, &staff, &ctx),
        links::revoke_by_staff(d, &id, &staff, &ctx),
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
        links::manual_link(d, "s1", "fake", "u2", &staff, &ctx),
        links::manual_link(d, "s1", "fake", "u3", &staff, &ctx),
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

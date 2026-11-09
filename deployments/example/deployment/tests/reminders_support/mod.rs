//! Shared by the reminders tests (#73, #135): the example Deployment with
//! three Students, and a signed-in Officer.

#![allow(dead_code)]

use example_deployment::{Secrets, builder};
use harpseal::Statement;
use harpseal_channel_fake::FakeChannel;
use harpseal_testing::{Harness, TestApp};

/// Monday 2026-11-09 09:00 in Bangkok (02:00 UTC).
pub const MONDAY_9AM: i64 = 1_794_189_600_000;
pub const MIN: i64 = 60_000;

pub fn secrets() -> Secrets {
    Secrets::from_lookup(|k| match k {
        "LINE_LIFF_ID" => Some("1234567890-AbCdEfGh".into()),
        "TELEGRAM_WEB_APP_URL" => Some("https://app.seal-bay.invalid/tg".into()),
        _ => None,
    })
}

pub async fn run(h: &Harness, sql: &str) {
    h.db()
        .execute(Statement::new(sql.to_string()))
        .await
        .unwrap();
}

/// Karn (linked, tagged), Mali (email only, tagged), Niran (linked, not
/// tagged).
pub async fn boot(h: &Harness) -> TestApp {
    let d = builder(h.services(), secrets())
        .unwrap()
        .dev(true)
        .channel(h.fake_channel())
        .build()
        .expect("the example Deployment builds");
    let app = h.boot(d).await;
    for (id, code, name, email) in [
        ("s1", "68010001", "Karn Seal", "NULL"),
        ("s2", "68010002", "Mali Tern", "'mali@seal-bay.invalid'"),
        ("s3", "68010003", "Niran Gull", "NULL"),
    ] {
        run(
            h,
            &format!(
                "INSERT INTO student (id, student_code, display_name, email, locale, \
                 created_at, updated_at) VALUES ('{id}', '{code}', '{name}', {email}, 'th', 1, 1)"
            ),
        )
        .await;
    }
    for id in ["s1", "s3"] {
        run(
            h,
            &format!(
                "INSERT INTO channel_account (id, channel, external_id, first_seen_at, \
                 flow_state, updated_at) VALUES ('a-{id}', 'fake', 'u-{id}', 1, 'linked', 1)"
            ),
        )
        .await;
        run(
            h,
            &format!(
                "INSERT INTO link (id, student_id, channel_account_id, channel, status, method, \
                 linked_at, updated_at) VALUES ('l-{id}', '{id}', 'a-{id}', 'fake', 'active', \
                 'email_otp', 1, 1)"
            ),
        )
        .await;
    }
    run(
        h,
        "INSERT INTO tag (id, name, created_at) VALUES ('t1', 'fee_due', 1)",
    )
    .await;
    for id in ["s1", "s2"] {
        run(
            h,
            &format!(
                "INSERT INTO student_tag (student_id, tag_id, source, added_at) \
                 VALUES ('{id}', 't1', 'roster', 1)"
            ),
        )
        .await;
    }
    app
}

pub fn chats(h: &Harness, id: &str) -> usize {
    h.fake_channel()
        .sent_to(&FakeChannel::account(format!("u-{id}")))
        .len()
}

/// A signed-in Officer and the form's `key`.
pub async fn officer<'a>(h: &Harness, app: &'a TestApp) -> (harpseal_testing::Browser<'a>, String) {
    h.create_staff(
        "officer@sealbay.example",
        "kelp forest at dawn",
        &["Officer"],
    )
    .await;
    let b = app.browser();
    b.login("officer@sealbay.example", "kelp forest at dawn")
        .await;
    let form = b.get("/admin/reminders").await.body;
    let at = form.find("name=\"key\" value=\"").unwrap() + "name=\"key\" value=\"".len();
    let key = form[at..at + form[at..].find('"').unwrap()].to_string();
    (b, key)
}

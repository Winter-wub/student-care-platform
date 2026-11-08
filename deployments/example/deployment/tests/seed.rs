//! `harpseal seed-example`: the demo data goes through the audited paths,
//! twice is the same as once, and it refuses a populated database.

use example_deployment::seed::{SeedError, seed};
use example_deployment::{Secrets, builder};
use harpseal::Statement;
use harpseal_testing::{Harness, TestApp};

fn secrets() -> Secrets {
    Secrets::from_lookup(|k| match k {
        "LINE_LIFF_ID" => Some("1234567890-AbCdEfGh".into()),
        "TELEGRAM_WEB_APP_URL" => Some("https://app.seal-bay.invalid/tg".into()),
        _ => None,
    })
}

async fn boot(h: &Harness) -> TestApp {
    let d = builder(h.services(), secrets())
        .channel(h.fake_channel())
        .build()
        .expect("the example Deployment builds");
    h.boot(d).await
}

async fn int(h: &Harness, sql: &str) -> i64 {
    let rows = h.db().query(Statement::new(sql.to_string())).await.unwrap();
    rows.rows[0][0].as_i64().unwrap()
}

/// (Students, active, announcements, scheduled, settings, Tags).
async fn counts(h: &Harness) -> [i64; 6] {
    [
        int(h, "SELECT count(*) FROM student").await,
        int(h, "SELECT count(*) FROM student WHERE status = 'active'").await,
        int(h, "SELECT count(*) FROM announcement").await,
        int(
            h,
            "SELECT count(*) FROM announcement WHERE status = 'scheduled'",
        )
        .await,
        int(h, "SELECT count(*) FROM setting WHERE locale <> ''").await,
        int(h, "SELECT count(*) FROM student_tag").await,
    ]
}

#[tokio::test]
async fn seeding_twice_gives_the_same_data_and_needs_force() {
    let h = Harness::new();
    let app = boot(&h).await;
    let admin = h
        .create_staff(
            "admin@sealbay.example",
            "kelp forest at dawn",
            &["Administrator"],
        )
        .await;
    let d = app.deployment();

    let first = seed(d, None, false).await.unwrap();
    assert_eq!(first.acting_as, "admin@sealbay.example");
    assert_eq!(
        (
            first.students,
            first.announcements_created,
            first.texts_written
        ),
        (50, 4, 2)
    );
    assert_eq!(first.settings_written, 1, "the fees bank account");
    let once = counts(&h).await;
    assert_eq!(once[..5], [50, 50, 4, 1, 2]);
    assert!(
        once[5] >= 100,
        "every Student has a year and a programme Tag"
    );

    let refused = seed(d, None, false).await.unwrap_err();
    assert!(matches!(refused, SeedError::NotEmpty(50)), "{refused}");

    let again = seed(d, None, true).await.unwrap();
    assert_eq!((again.announcements_created, again.texts_written), (0, 0));
    assert_eq!(again.settings_written, 0, "a saved account is left alone");
    assert_eq!(counts(&h).await, once, "a forced re-run adds nothing");

    // Every audit row of the seed names the Administrator and is marked.
    let marked = format!(
        "SELECT count(*) FROM audit_log WHERE actor_kind = 'staff' AND actor_id = '{admin}' \
         AND request_id = 'cli' AND json_extract(after, '$.seed') = 1"
    );
    let unmarked = format!(
        "SELECT count(*) FROM audit_log WHERE actor_id = '{admin}' \
         AND json_extract(after, '$.seed') IS NOT 1"
    );
    // Two uploads and confirms, four creates, one schedule, two texts.
    assert_eq!(int(&h, &marked).await, 4 + 4 + 1 + 2);
    assert_eq!(int(&h, &unmarked).await, 0);

    // The bank account: saved through `Settings::save` as the system, one
    // audit row marked as seed data, the value before `null`; Students see
    // it where they saw "not set".
    let account = d
        .settings()
        .text(example_deployment::fees::BANK_ACCOUNT)
        .await;
    assert_eq!(account.unwrap(), example_deployment::seed::BANK_ACCOUNT);
    let audit = "SELECT count(*) FROM audit_log WHERE action = 'setting.update' \
                 AND actor_kind = 'system' AND target_id = 'fees.bank_account:' \
                 AND json_extract(before, '$.value') IS NULL \
                 AND json_extract(after, '$.seed') = 1";
    assert_eq!(int(&h, audit).await, 1);
}

#[tokio::test]
async fn seeding_needs_an_administrator() {
    let h = Harness::new();
    let app = boot(&h).await;
    let err = seed(app.deployment(), None, false).await.unwrap_err();
    assert!(matches!(err, SeedError::NoAdministrator), "{err}");
    assert!(err.to_string().contains("harpseal staff create --admin"));

    h.create_staff("viewer@sealbay.example", "kelp forest at dawn", &["Viewer"])
        .await;
    let err = seed(app.deployment(), Some("viewer@sealbay.example"), false)
        .await
        .unwrap_err();
    assert!(matches!(err, SeedError::NoAdministrator), "{err}");
    assert_eq!(int(&h, "SELECT count(*) FROM student").await, 0);
}

#[tokio::test]
async fn a_customised_text_survives_a_forced_reseed() {
    let h = Harness::new();
    let app = boot(&h).await;
    let admin = h
        .create_staff(
            "admin@sealbay.example",
            "kelp forest at dawn",
            &["Administrator"],
        )
        .await;
    let d = app.deployment();
    let custom = "สวัสดีครับ ข้อความที่ผู้ดูแลแก้เอง กรุณายอมรับประกาศด้านล่าง";
    let actor = harpseal::Actor::Staff(admin);
    let ctx = harpseal::RequestCtx::cli();
    assert!(
        harpseal::texts::set_override(d, "kernel.flow.welcome", "th", custom, &actor, &ctx)
            .await
            .unwrap()
    );
    let first = seed(d, None, false).await.unwrap();
    assert_eq!(first.texts_written, 1, "only the English text was free");
    let forced = seed(d, None, true).await.unwrap();
    assert_eq!(forced.texts_written, 0);
    let rows = h
        .db()
        .query(Statement::new(
            "SELECT value FROM setting WHERE key = 'kernel.flow.welcome' AND locale = 'th'",
        ))
        .await
        .unwrap();
    assert_eq!(rows.get(0, "value").and_then(|v| v.as_text()), Some(custom));
}

#[tokio::test]
async fn seeding_as_a_named_administrator() {
    let h = Harness::new();
    let app = boot(&h).await;
    h.create_staff(
        "first@sealbay.example",
        "kelp forest at dawn",
        &["Administrator"],
    )
    .await;
    let second = h
        .create_staff(
            "second@sealbay.example",
            "kelp forest at dawn",
            &["Administrator"],
        )
        .await;
    let report = seed(app.deployment(), Some("Second@SealBay.example "), false)
        .await
        .unwrap();
    assert_eq!(report.acting_as, "second@sealbay.example");
    let by_second = format!(
        "SELECT count(*) FROM audit_log WHERE actor_id = '{second}' \
         AND json_extract(after, '$.seed') = 1"
    );
    assert_eq!(int(&h, &by_second).await, 2 + 4 + 1 + 2);
}

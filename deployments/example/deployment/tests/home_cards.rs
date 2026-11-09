//! The Fees Module's status card on the back-office home (#123): shown
//! only to Staff holding `fees.import`, empty before the first import,
//! in order when nobody owes, and a warning with the next reminder while
//! Students owe; its action follows the Staff member's Permissions.

use example_deployment::{Secrets, builder, reminders};
use harpseal::{Actor, RequestCtx, Statement};
use harpseal_testing::{Browser, Harness, TestApp};

const PASSWORD: &str = "kelp forest at dawn";

fn secrets() -> Secrets {
    Secrets::from_lookup(|k| match k {
        "LINE_LIFF_ID" => Some("1234567890-AbCdEfGh".into()),
        "TELEGRAM_WEB_APP_URL" => Some("https://app.seal-bay.invalid/tg".into()),
        _ => None,
    })
}

async fn boot(h: &Harness) -> TestApp {
    let d = builder(h.services(), secrets())
        .unwrap()
        .channel(h.fake_channel())
        .build()
        .expect("the example Deployment builds");
    let app = h.boot(d).await;
    let roles: [(&str, &[&str]); 3] = [
        ("FeeDesk", &["fees.import"]),
        ("FeeChaser", &["fees.import", reminders::PERMISSION.id]),
        ("Lookup", &["kernel.student.view"]),
    ];
    for (name, perms) in roles {
        let (ctx, services) = (RequestCtx::cli(), h.services());
        let made = harpseal::ensure_role(&services, &Actor::System, name, perms, &ctx);
        assert!(made.await.unwrap());
        let email = format!("{}@sealbay.example", name.to_lowercase());
        h.create_staff(&email, PASSWORD, &[name]).await;
    }
    app
}

async fn home(app: &TestApp, who: &str) -> String {
    let b: Browser<'_> = app.browser();
    b.login(&format!("{who}@sealbay.example"), PASSWORD).await;
    b.get("/admin").await.body
}

/// The fees card's `<li>`, if shown.
fn fees_card(body: &str) -> Option<String> {
    body.split("<li class=\"status-card\"")
        .skip(1)
        .map(|li| li.split("</li>").next().unwrap_or_default().to_string())
        .find(|li| li.contains("<h2>ค่าธรรมเนียม "))
}

async fn exec(h: &Harness, sql: &str) {
    h.db()
        .execute(Statement::new(sql.to_string()))
        .await
        .unwrap();
}

#[tokio::test]
async fn the_fees_card_follows_the_import_and_the_staff_members_permissions() {
    let h = Harness::new();
    let app = boot(&h).await;
    assert!(
        fees_card(&home(&app, "lookup").await).is_none(),
        "fees.import only"
    );

    let card = fees_card(&home(&app, "feedesk").await).unwrap();
    assert!(card.contains("data-state=\"empty\""), "{card}");
    assert!(card.contains("ยังไม่ได้นำเข้ายอดค้างชำระ") && card.contains("href=\"/admin/fees\""));

    exec(
        &h,
        "INSERT INTO fees_import (upload_id, rows, imported_at) VALUES ('u1', 2, 1)",
    )
    .await;
    exec(
        &h,
        "INSERT INTO fees_balance (student_code, amount_satang, upload_id, updated_at) \
         VALUES ('68010001', 0, 'u1', 1)",
    )
    .await;
    let card = fees_card(&home(&app, "feedesk").await).unwrap();
    assert!(card.contains("data-state=\"ok\"") && card.contains("ไม่มีนักศึกษาค้างชำระ"));

    exec(
        &h,
        "INSERT INTO fees_balance (student_code, amount_satang, upload_id, updated_at) \
         VALUES ('68010002', 1250000, 'u1', 1), ('68010003', 500, 'u1', 1)",
    )
    .await;
    let card = fees_card(&home(&app, "feedesk").await).unwrap();
    assert!(card.contains("data-state=\"warn\""), "{card}");
    assert!(card.contains("2 คนค้างชำระ · เตือนอัตโนมัติรอบถัดไป วันจันทร์ 9 พ.ย. 2569 09:00"));
    assert!(
        card.contains("href=\"/admin/fees\""),
        "no reminders.send: open fees"
    );
    let card = fees_card(&home(&app, "feechaser").await).unwrap();
    assert!(
        card.contains("href=\"/admin/reminders\""),
        "may send: remind now"
    );
}

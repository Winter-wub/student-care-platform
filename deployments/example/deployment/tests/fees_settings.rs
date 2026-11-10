//! The example's Fees bank account (#75): a Module setting Finance edits
//! on `/admin/settings` and the Student sees on "My fees" in the Mini App.
//! Administrators hold `kernel.setting.edit` but not the setting's own
//! Permission: they see it read-only. The account has no default (#103):
//! Students see "not set" until Finance saves one.

use example_deployment::{Secrets, builder, fees};
use harpseal::Statement;
use harpseal_testing::{Browser, Harness, TestApp, urlencode};

const PASSWORD: &str = "kelp forest at dawn";

fn secrets() -> Secrets {
    Secrets::from_lookup(|k| match k {
        "LINE_LIFF_ID" => Some("1234567890-AbCdEfGh".into()),
        "TELEGRAM_WEB_APP_URL" => Some("https://app.seal-bay.invalid/tg".into()),
        _ => None,
    })
}

async fn run(h: &Harness, sql: &str) {
    h.db()
        .execute(Statement::new(sql.to_string()))
        .await
        .unwrap();
}

/// One linked Student who owes 12,500.00 baht.
async fn boot(h: &Harness) -> TestApp {
    let d = builder(h.services(), secrets())
        .unwrap()
        .dev(true)
        .channel(h.fake_channel())
        .build()
        .expect("the example Deployment builds");
    let app = h.boot(d).await;
    for sql in [
        "INSERT INTO student (id, student_code, display_name, locale, attributes, created_at, \
         updated_at) VALUES ('s1', '68010001', 'Karn Seal', 'th', '{}', 1, 1)",
        "INSERT INTO channel_account (id, channel, external_id, first_seen_at, flow_state, \
         updated_at) VALUES ('a-s1', 'fake', 'u-s1', 1, 'linked', 1)",
        "INSERT INTO link (id, student_id, channel_account_id, channel, status, method, \
         linked_at, updated_at) VALUES ('l-s1', 's1', 'a-s1', 'fake', 'active', 'email_otp', 1, 1)",
        "INSERT INTO fees_balance (student_code, amount_satang, upload_id, updated_at) \
         VALUES ('68010001', 1250000, 'u1', 1)",
    ] {
        run(h, sql).await;
    }
    app
}

async fn signed_in<'a>(h: &Harness, app: &'a TestApp, role: &str) -> Browser<'a> {
    let email = format!("{}@sealbay.example", role.to_lowercase());
    h.create_staff(&email, PASSWORD, &[role]).await;
    let b = app.browser();
    assert_eq!(b.login(&email, PASSWORD).await.status, 303);
    b
}

fn version(body: &str) -> String {
    let k = "name=\"version\" value=\"";
    let at = body.find(k).expect("a form") + k.len();
    body[at..at + body[at..].find('"').unwrap()].to_string()
}

/// Saves `value` for `key` as `b`, with the token the page showed.
async fn save(b: &Browser<'_>, key: &str, value: &str) -> harpseal_testing::TestResponse {
    let form = b.get("/admin/settings").await.body;
    let at = form
        .find(&format!("name=\"key\" value=\"{key}\""))
        .expect("a form for the key");
    let body = format!(
        "key={key}&value={}&version={}",
        urlencode(value),
        urlencode(&version(&form[at..]))
    );
    b.post("/admin/settings", &body).await
}

#[tokio::test]
async fn finance_changes_the_bank_account_the_student_sees_next() {
    let h = Harness::new();
    let app = boot(&h).await;
    let student = app.browser();
    let r = student
        .open_mini_app(&h.fake_channel(), "u-s1", fees::student::PATH)
        .await;
    assert_eq!(r.status, 303, "{}", r.body);
    let home = student.get("/app").await.body;
    assert!(home.contains("href=\"/app/fees\""), "{home}");
    let page = student.get(fees::student::PATH).await.body;
    assert!(page.contains("12,500.00 บาท"), "{page}");
    assert!(!page.contains("ธนาคารทะเลใส"), "{page}");
    assert!(
        page.contains("id=\"not-set\""),
        "no account saved yet: {page}"
    );

    let finance = signed_in(&h, &app, "Finance").await;
    // The page tells Finance it is not set, not that a saved value failed.
    let admin_page = finance.get("/admin/settings").await.body;
    assert!(
        admin_page.contains("id=\"setting-fees-bank_account-unset\""),
        "{admin_page}"
    );
    assert!(!admin_page.contains("flash err"), "not a failed check");
    assert_eq!(
        save(&finance, fees::BANK_ACCOUNT, "123-4-56789-0")
            .await
            .status,
        303
    );
    let page = student.get(fees::student::PATH).await.body;
    assert!(page.contains(">123-4-56789-0</strong>"), "{page}");
    assert!(page.contains("ธนาคารทะเลใส สาขาอ่าวแมวน้ำ"));
    assert_eq!(
        save(&finance, fees::BANK_NAME, "ธนาคารอ่าวคราม สาขาหาดทราย")
            .await
            .status,
        303
    );
    assert_eq!(
        save(&finance, fees::BANK_ACCOUNT, "999-9-99999-9")
            .await
            .status,
        303
    );
    let page = student.get(fees::student::PATH).await.body;
    assert!(page.contains("ธนาคารอ่าวคราม สาขาหาดทราย"), "{page}");
    assert!(page.contains(">999-9-99999-9</strong>"), "{page}");
    assert!(!page.contains("ธนาคารทะเลใส"));
}

#[tokio::test]
async fn the_account_number_takes_digits_only_and_a_bad_row_shows_not_set() {
    let h = Harness::new();
    let app = boot(&h).await;
    let finance = signed_in(&h, &app, "Finance").await;
    // Reads 000-0-12345-0 on screen, copies as 0-54321-0-000.
    for bad in ["\u{202E}0-54321-0-000", "000-0-0000O-0", "บัญชี 000"] {
        let r = save(&finance, fees::BANK_ACCOUNT, bad).await;
        assert_eq!(r.status, 422, "{bad}");
    }
    let r = save(&finance, fees::BANK_NAME, "ธนาคาร\u{2067}ทะเลใส").await;
    assert_eq!(r.status, 422);
    // A row that slipped in (a restore) is "not set", never the default.
    run(
        &h,
        "INSERT INTO setting (key, locale, value, updated_at) \
         VALUES ('fees.bank_account', '', '0-54321-0-000\u{202E}', 1)",
    )
    .await;
    let student = app.browser();
    student
        .open_mini_app(&h.fake_channel(), "u-s1", fees::student::PATH)
        .await;
    let page = student.get(fees::student::PATH).await;
    assert_eq!(page.status, 200);
    assert!(page.body.contains("ยังไม่ได้ตั้งค่า"), "{}", page.body);
    assert!(
        !page.body.contains("0-54321-0-000"),
        "not the broken account"
    );
}

#[tokio::test]
async fn an_administrator_sees_the_bank_account_but_cannot_change_it() {
    let h = Harness::new();
    let app = boot(&h).await;
    let admin = signed_in(&h, &app, "Administrator").await;
    let page = admin.get("/admin/settings").await;
    assert_eq!(page.status, 200);
    assert!(
        page.body.contains("fees.bank_account.edit"),
        "names the Permission"
    );
    // No Save button on the fees cards (the reminders switch has one).
    for key in [fees::BANK_NAME, fees::BANK_ACCOUNT] {
        let id = format!("id=\"setting-{}\"", key.replace('.', "-"));
        let at = page.body.find(&id).expect("the fees card");
        let card = &page.body[at..at + page.body[at..].find("</section>").unwrap()];
        assert!(!card.contains("name=\"key\""), "no Save button: {key}");
    }
    let r = admin
        .post(
            "/admin/settings",
            "key=fees.bank_account&value=111&version=-",
        )
        .await;
    assert_eq!(r.status, 403);
    let account = app.deployment().settings().text(fees::BANK_ACCOUNT).await;
    assert!(matches!(account, Err(harpseal::Error::SettingNotSet(_))));
}

#[tokio::test]
async fn a_database_error_is_a_failure_never_not_set() {
    let h = Harness::new();
    let app = boot(&h).await;
    let student = app.browser();
    student
        .open_mini_app(&h.fake_channel(), "u-s1", fees::student::PATH)
        .await;
    assert_eq!(student.get(fees::student::PATH).await.status, 200);
    // The settings read fails (texts still load: they read only key,
    // locale and value): a database error, not a bad saved value.
    run(&h, "ALTER TABLE setting RENAME TO setting_before").await;
    run(
        &h,
        "CREATE TABLE setting (key TEXT, locale TEXT, value TEXT)",
    )
    .await;
    let page = student.get(fees::student::PATH).await;
    assert_eq!(page.status, 500);
    assert!(!page.body.contains("ยังไม่ได้ตั้งค่า"), "{}", page.body);
}

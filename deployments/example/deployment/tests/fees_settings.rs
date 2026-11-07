//! The example's Fees bank account (#75): a Module setting Finance edits
//! on `/admin/settings` and the Student sees on "My fees" in the Mini App.
//! Administrators hold `kernel.setting.edit` but not the setting's own
//! Permission: they see it read-only.

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
    assert!(page.contains("ธนาคารทะเลใส สาขาอ่าวแมวน้ำ<br>เลขที่บัญชี 000-0-00000-0"));

    let finance = signed_in(&h, &app, "Finance").await;
    let form = finance.get("/admin/settings").await.body;
    assert!(form.contains("name=\"key\" value=\"fees.bank_account\""));
    let new = "ธนาคารอ่าวคราม สาขาหาดทราย\r\nเลขที่บัญชี 999-9-99999-9";
    let body = format!(
        "key=fees.bank_account&value={}&version={}",
        urlencode(new),
        urlencode(&version(&form))
    );
    assert_eq!(finance.post("/admin/settings", &body).await.status, 303);

    let page = student.get(fees::student::PATH).await.body;
    assert!(
        page.contains("ธนาคารอ่าวคราม สาขาหาดทราย<br>เลขที่บัญชี 999-9-99999-9"),
        "{page}"
    );
    assert!(!page.contains("ธนาคารทะเลใส"));
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
    assert!(!page.body.contains("name=\"key\""), "no Save button");
    let r = admin
        .post("/admin/settings", "key=fees.bank_account&value=x&version=-")
        .await;
    assert_eq!(r.status, 403);
    let account = app
        .deployment()
        .settings()
        .text(fees::BANK_ACCOUNT)
        .await
        .unwrap();
    assert!(account.starts_with("ธนาคารทะเลใส"));
}

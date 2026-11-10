//! What the fee reminder's email adds (#163): the fees page in LINE for
//! a Student with a LINE Link the chat cannot reach (blocked), how to
//! link first for a Student with none, nothing with the switch off; a
//! broken switch value counts as off and never stops the reminder.

mod reminders_support;

use example_deployment::reminders;
use harpseal::{Statement, Timestamp};
use harpseal_testing::{Harness, TestApp};
use reminders_support::*;

const FEES: &str = "https://liff.line.me/1234567890-AbCdEfGh/app/fees";
const MALI: &str = "mali@seal-bay.invalid";
const PIM: &str = "pim@seal-bay.invalid";

/// The reminders setup, plus: Mali has a LINE Link she blocked
/// (unreachable), and Pim (tagged, an address, no Link).
async fn boot_blocked(h: &Harness) -> TestApp {
    let app = boot(h).await;
    for sql in [
        "INSERT INTO channel_account (id, channel, external_id, first_seen_at, flow_state, \
         updated_at) VALUES ('a-s2', 'line', 'U-mali', 1, 'linked', 1)",
        "INSERT INTO link (id, student_id, channel_account_id, channel, status, method, \
         linked_at, updated_at) VALUES ('l-s2', 's2', 'a-s2', 'line', 'unreachable', \
         'email_otp', 1, 1)",
        "INSERT INTO student (id, student_code, display_name, email, locale, created_at, \
         updated_at) VALUES ('s5', '68010005', 'Pim Auk', 'pim@seal-bay.invalid', 'th', 1, 1)",
        "INSERT INTO student_tag (student_id, tag_id, source, added_at) \
         VALUES ('s5', 't1', 'roster', 1)",
    ] {
        run(h, sql).await;
    }
    app
}

/// Runs this Monday's reminder; the one email to `to`.
async fn monday(h: &Harness, app: &TestApp, to: &str) -> String {
    h.clock().set_ms(MONDAY_9AM);
    let d = app.deployment();
    d.tick(Timestamp(MONDAY_9AM)).await.unwrap();
    let t = d.tick(Timestamp(MONDAY_9AM + MIN)).await.unwrap();
    assert!(t.errors.is_empty(), "{:?}", t.errors);
    assert_eq!(t.failed, 0, "{:?}", t.errors);
    let mails = h.email().to(to);
    assert_eq!(mails.len(), 1, "{to}");
    mails[0].text.clone()
}

async fn switch(h: &Harness, value: &str) {
    let sql = format!(
        "INSERT INTO setting (key, locale, value, updated_at) VALUES ('{}', '', '{value}', 1)",
        reminders::EMAIL_LINK
    );
    h.db().execute(Statement::new(sql)).await.unwrap();
}

#[tokio::test]
async fn a_blocked_line_link_gets_the_fees_page_and_no_link_gets_how_to_link() {
    let h = Harness::new();
    let app = boot_blocked(&h).await;
    let mali = monday(&h, &app, MALI).await;
    assert!(
        mali.ends_with(&format!("ดูยอดที่ต้องชำระใน LINE: {FEES}")),
        "{mali}"
    );
    assert!(!mali.contains("ผูกบัญชีด้วยรหัสนักศึกษา"), "{mali}");
    // In the same run, Pim (no Link): how to link first, no URL.
    let pim = h.email().to(PIM);
    assert_eq!(pim.len(), 1);
    assert!(pim[0].text.contains("ผูกบัญชีด้วยรหัสนักศึกษา"), "{}", pim[0].text);
    assert!(!pim[0].text.contains("https://"), "{}", pim[0].text);
    // One chunk, both groups in its report.
    let note = app
        .count(
            "SELECT count(*) FROM job WHERE kind = 'reminders.send' AND progress LIKE '%email 2,%'",
        )
        .await;
    assert_eq!(note, 1);
}

#[tokio::test]
async fn the_switch_off_leaves_the_link_out() {
    let h = Harness::new();
    let app = boot_blocked(&h).await;
    let ctx = harpseal::RequestCtx::cli();
    app.deployment()
        .save_setting_as_system(reminders::EMAIL_LINK, "0", &ctx)
        .await
        .unwrap();
    let mali = monday(&h, &app, MALI).await;
    assert!(!mali.contains("https://"), "{mali}");
    // Linking first is not a link: Pim is still told how.
    assert!(h.email().to(PIM)[0].text.contains("ผูกบัญชีด้วยรหัสนักศึกษา"));
}

#[tokio::test]
async fn a_broken_switch_value_counts_as_off_and_the_reminder_still_goes() {
    for broken in ["2", "true"] {
        let h = Harness::new();
        let app = boot_blocked(&h).await;
        switch(&h, broken).await;
        let read = app
            .deployment()
            .settings()
            .bool(reminders::EMAIL_LINK)
            .await;
        assert!(
            matches!(read, Err(harpseal::Error::SettingNotSet(_))),
            "{broken}: {read:?}"
        );
        let mali = monday(&h, &app, MALI).await;
        assert!(!mali.contains("https://"), "{broken}: {mali}");
    }
}

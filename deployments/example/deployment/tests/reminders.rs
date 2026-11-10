//! The example's fee reminder (#73): the weekly Job notifies the Students
//! tagged `fee_due` (chat when linked, email otherwise), and Staff send it
//! now to pasted student IDs from the back office, through a review step
//! (#135, tested in `reminders_review.rs`); neither repeats.

mod reminders_support;

use example_deployment::reminders;
use harpseal::Timestamp;
use harpseal_testing::Harness;
use reminders_support::*;

#[tokio::test]
async fn the_monday_job_reminds_the_tagged_students_once() {
    let h = Harness::new();
    let app = boot(&h).await;
    h.clock().set_ms(MONDAY_9AM);
    let d = app.deployment();
    d.tick(Timestamp(MONDAY_9AM)).await.unwrap();
    let t = d.tick(Timestamp(MONDAY_9AM + MIN)).await.unwrap();
    assert!(t.errors.is_empty(), "{:?}", t.errors);
    assert_eq!(chats(&h, "s1"), 1, "Karn is linked: chat");
    assert_eq!(chats(&h, "s3"), 0, "Niran is not tagged");
    let mali = h.email().to("mali@seal-bay.invalid");
    assert_eq!(mali.len(), 1, "Mali has no Link: email");
    assert_eq!(mali[0].subject, "เตือนชำระค่าธรรมเนียม");
    let rows = app
        .count("SELECT count(*) FROM outbound_message WHERE module = 'reminders'")
        .await;
    assert_eq!(rows, 1);

    // The same week's Job again (a second Cron Trigger, a manual re-run):
    // nobody gets it twice.
    d.enqueue(reminders::WEEKLY, serde_json::json!({}), None)
        .await
        .unwrap();
    h.clock().advance_minutes(2);
    d.tick(Timestamp(h.clock().now_ms())).await.unwrap();
    d.tick(Timestamp(h.clock().now_ms() + MIN)).await.unwrap();
    assert_eq!(chats(&h, "s1"), 1);
    assert_eq!(h.email().to("mali@seal-bay.invalid").len(), 1);
    let note = app
        .count("SELECT count(*) FROM job WHERE kind = 'reminders.send' AND state = 'done'")
        .await;
    assert_eq!(note, 1, "the chunk's unique key holds");
}

#[tokio::test]
async fn staff_send_it_now_to_pasted_ids_and_a_second_click_repeats_nothing() {
    let h = Harness::new();
    let app = boot(&h).await;
    h.create_staff(
        "officer@sealbay.example",
        "kelp forest at dawn",
        &["Officer"],
    )
    .await;
    let b = app.browser();
    assert_eq!(
        b.login("officer@sealbay.example", "kelp forest at dawn")
            .await
            .status,
        303
    );
    let form = b.get("/admin/reminders").await.body;
    let at = form.find("name=\"key\" value=\"").unwrap() + "name=\"key\" value=\"".len();
    let key = &form[at..at + form[at..].find('"').unwrap()];
    let ids = "ids=68010003%0A68010002%0A68019999";
    // "ตรวจก่อนส่งเตือน" (#157) opens the review step (#135): nothing is sent yet.
    let review = b
        .post("/admin/reminders", &format!("key={key}&{ids}"))
        .await;
    assert_eq!(review.status, 200, "{}", review.body);
    assert!(review.body.contains("ตรวจก่อนส่งเตือน"), "{}", review.body);
    assert_eq!(chats(&h, "s3"), 0, "the review sends nothing");
    assert!(h.email().to("mali@seal-bay.invalid").is_empty());
    let post = format!("key={key}&{ids}&reviewed=1&reach=2");
    let r = b.post("/admin/reminders", &post).await;
    assert_eq!(r.status, 200, "{}", r.body);
    for seen in [
        "68010003</span>: ส่งทางแชทแล้ว",
        "68010002</span>: ส่งทางอีเมลแล้ว",
        "68019999</span>: ข้าม",
    ] {
        assert!(r.body.contains(seen), "{seen}: {}", r.body);
    }
    assert_eq!(chats(&h, "s3"), 1);

    // A form without its key is refused, never sent under a shared one.
    let r = b
        .post("/admin/reminders", "key=&ids=68010003&reviewed=1&reach=1")
        .await;
    assert_eq!(r.status, 400);
    assert_eq!(chats(&h, "s3"), 1);

    // The same form posted again (a double click, a reload).
    b.post("/admin/reminders", &post).await;
    assert_eq!(chats(&h, "s3"), 1);
    assert_eq!(h.email().to("mali@seal-bay.invalid").len(), 1);

    // A Viewer may not send it.
    h.create_staff("viewer@sealbay.example", "kelp forest at dawn", &["Viewer"])
        .await;
    let v = app.browser();
    v.login("viewer@sealbay.example", "kelp forest at dawn")
        .await;
    assert_eq!(v.get("/admin/reminders").await.status, 403);
}

#[tokio::test]
async fn a_student_waiting_for_the_line_quota_is_reminded_a_day_later() {
    let h = Harness::new();
    let app = boot(&h).await;
    // Ploy (tagged) is on LINE only, with no email address.
    for sql in [
        "INSERT INTO student (id, student_code, display_name, locale, created_at, updated_at) \
         VALUES ('s4', '68010004', 'Ploy Heron', 'th', 1, 1)",
        "INSERT INTO channel_account (id, channel, external_id, first_seen_at, flow_state, \
         updated_at) VALUES ('a-s4', 'line', 'U00000000000000000000000000000004', 1, \
         'linked', 1)",
        "INSERT INTO link (id, student_id, channel_account_id, channel, status, method, \
         linked_at, updated_at) VALUES ('l-s4', 's4', 'a-s4', 'line', 'active', 'email_otp', 1, 1)",
        "INSERT INTO student_tag (student_id, tag_id, source, added_at) VALUES ('s4', 't1', \
         'roster', 1)",
    ] {
        run(&h, sql).await;
    }
    h.http()
        .respond_with(429, r#"{"message":"You have reached your monthly limit."}"#);
    h.clock().set_ms(MONDAY_9AM);
    let d = app.deployment();
    d.tick(Timestamp(MONDAY_9AM)).await.unwrap();
    let t = d.tick(Timestamp(MONDAY_9AM + MIN)).await.unwrap();
    assert!(t.errors.is_empty(), "{:?}", t.errors);
    let waiting = "SELECT count(*) FROM job WHERE kind = 'reminders.send' AND state = 'queued' \
                   AND payload LIKE '%68010004%'";
    assert_eq!(app.count(waiting).await, 1, "re-queued for tomorrow");
    let line = |h: &Harness| h.http_requests().len();
    assert_eq!(line(&h), 1);

    // A day later the quota has reset (LINE answers 200): Ploy gets it.
    let tomorrow = MONDAY_9AM + MIN + 24 * 60 * MIN;
    h.clock().set_ms(tomorrow);
    let t = d.tick(Timestamp(tomorrow)).await.unwrap();
    assert!(t.errors.is_empty(), "{:?}", t.errors);
    assert_eq!(line(&h), 2, "one Push to Ploy");
    assert_eq!(app.count(waiting).await, 0);
    assert_eq!(chats(&h, "s1"), 1, "Karn was not reminded twice");
}

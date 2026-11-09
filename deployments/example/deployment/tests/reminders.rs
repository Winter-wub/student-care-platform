//! The example's fee reminder (#73): the weekly Job notifies the Students
//! tagged `fee_due` (chat when linked, email otherwise), and Staff send it
//! now to pasted student IDs from the back office, through a review step
//! (#135); neither repeats.

use example_deployment::{Secrets, builder, reminders};
use harpseal::{Statement, Timestamp};
use harpseal_channel_fake::FakeChannel;
use harpseal_testing::{Harness, TestApp};

/// Monday 2026-11-09 09:00 in Bangkok (02:00 UTC).
const MONDAY_9AM: i64 = 1_794_189_600_000;
const MIN: i64 = 60_000;

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

/// Karn (linked, tagged), Mali (email only, tagged), Niran (linked, not
/// tagged).
async fn boot(h: &Harness) -> TestApp {
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

fn chats(h: &Harness, id: &str) -> usize {
    h.fake_channel()
        .sent_to(&FakeChannel::account(format!("u-{id}")))
        .len()
}

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
    // "ส่งเตือนตอนนี้" opens the review step (#135): nothing is sent yet.
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

/// A signed-in Officer and the form's `key`.
async fn officer<'a>(h: &Harness, app: &'a TestApp) -> (harpseal_testing::Browser<'a>, String) {
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

#[tokio::test]
async fn the_review_shows_who_gets_it_how_and_the_message_and_sends_only_when_confirmed() {
    let h = Harness::new();
    let app = boot(&h).await;
    run(
        &h,
        "INSERT INTO student (id, student_code, display_name, locale, status, created_at, \
         updated_at) VALUES ('s9', '68010009', 'Left Puffin', 'th', 'left', 1, 1)",
    )
    .await;
    let (b, key) = officer(&h, &app).await;
    let ids = "ids=68010001%0A68010002%0A68010009%0A68019999";
    let r = b
        .post("/admin/reminders", &format!("key={key}&{ids}"))
        .await;
    assert_eq!(r.status, 200);
    let page = &r.body;
    assert_eq!(page.matches("<h1").count(), 1);
    assert!(
        page.contains(r#"data-count="recipients"><strong>2 คน</strong>"#),
        "{page}"
    );
    assert!(page.contains("Karn Seal, Mali Tern"), "first names: {page}");
    assert!(page.contains("แชท fake 1 บัญชี"), "{page}");
    assert!(page.contains("อีเมล 1 คน"), "{page}");
    assert!(page.contains("68010009</span>: พ้นสภาพแล้ว"), "{page}");
    assert!(page.contains("68019999</span>: ไม่พบรหัสนี้"), "{page}");
    assert!(
        page.contains("กรุณาชำระค่าธรรมเนียมการศึกษาภายในวันศุกร์นี้"),
        "preview"
    );
    assert!(page.contains(r#"name="reach" value="2""#), "{page}");
    assert!(page.contains(">ยืนยันส่งเตือน 2 คน</button>"), "{page}");
    // "กลับไปแก้ไข" goes back to the form with the list kept.
    assert!(
        page.contains(r#"href="/admin/reminders?ids=68010001%0A68010002%0A68010009%0A68019999""#),
        "{page}"
    );
    let back = b.get("/admin/reminders?ids=68010001%0A68010002").await.body;
    assert!(back.contains(">68010001\n68010002</textarea>"), "{back}");
    // A stale count shows the review again, unsent.
    let stale = format!("key={key}&{ids}&reviewed=1&reach=3");
    let r = b.post("/admin/reminders", &stale).await;
    assert!(r.body.contains("ผู้รับเปลี่ยนไปจากที่ตรวจไว้"), "{}", r.body);
    assert_eq!(chats(&h, "s1"), 0);
    let ok = format!("key={key}&{ids}&reviewed=1&reach=2");
    let r = b.post("/admin/reminders", &ok).await;
    assert!(
        r.body.contains("68010001</span>: ส่งทางแชทแล้ว"),
        "{}",
        r.body
    );
    assert_eq!(chats(&h, "s1"), 1);
}

#[tokio::test]
async fn nobody_to_reach_disables_the_send_with_the_reason() {
    let h = Harness::new();
    let app = boot(&h).await;
    run(
        &h,
        "INSERT INTO student (id, student_code, display_name, locale, created_at, updated_at) \
         VALUES ('s8', '68010008', 'Quiet Auk', 'th', 1, 1)",
    )
    .await;
    let (b, key) = officer(&h, &app).await;
    let blocked = r#"<button type="submit" disabled aria-describedby="reminders-blocked">"#;
    for (ids, why) in [
        ("68010008%0A68019999", "ยังไม่มีนักศึกษาที่ผูกบัญชีหรือมีอีเมลในรายชื่อนี้"),
        ("", "กรุณาใส่รหัสนักศึกษาอย่างน้อยหนึ่งคน"),
    ] {
        let r = b
            .post("/admin/reminders", &format!("key={key}&ids={ids}"))
            .await;
        assert!(r.body.contains(blocked), "{ids}: {}", r.body);
        assert!(r.body.contains(why), "{ids}: {}", r.body);
        assert!(!r.body.contains(r#"name="reviewed""#), "no confirm form");
        // Posted anyway: still the review, nothing sent.
        let r = b
            .post(
                "/admin/reminders",
                &format!("key={key}&ids={ids}&reviewed=1&reach=0"),
            )
            .await;
        assert!(r.body.contains(blocked), "{ids}");
    }
    let many: Vec<String> = (0..41).map(|n| format!("6802{n:04}")).collect();
    let r = b
        .post(
            "/admin/reminders",
            &format!("key={key}&ids={}", many.join("%0A")),
        )
        .await;
    assert!(r.body.contains("ส่งเตือนได้ครั้งละไม่เกิน 40 คน"), "{}", r.body);
    assert!(
        r.body.contains(&format!("{blocked}ยืนยันส่งเตือน</button>")),
        "no \"0 คน\" on a blocked send: {}",
        r.body
    );
    assert_eq!(app.count("SELECT count(*) FROM outbound_message").await, 0);
    assert!(h.email().to("mali@seal-bay.invalid").is_empty());
}

#[tokio::test]
async fn the_page_shows_the_weekly_schedule() {
    let h = Harness::new();
    let app = boot(&h).await;
    h.clock().set_ms(MONDAY_9AM - 60 * MIN);
    let (b, _) = officer(&h, &app).await;
    let page = b.get("/admin/reminders").await.body;
    assert!(page.contains("ส่งอัตโนมัติทุกสัปดาห์"), "{page}");
    assert!(
        page.contains("ทุกวันจันทร์ เวลา 09:00 น. ถึงนักศึกษาที่ยังศึกษาอยู่และมีป้าย fee_due"),
        "{page}"
    );
    assert!(page.contains("ตอนนี้มี 2 คน"), "Karn and Mali: {page}");
    assert!(page.contains("ครั้งถัดไป"), "{page}");
}

#[tokio::test]
async fn names_show_only_to_staff_who_may_view_students() {
    let h = Harness::new();
    let app = boot(&h).await;
    // A Role that may send the reminder but not view Students.
    run(
        &h,
        "INSERT INTO role (id, name, created_at) VALUES ('r-send', 'Sender', 1)",
    )
    .await;
    run(
        &h,
        "INSERT INTO role_permission (role_id, permission_id) VALUES ('r-send', 'reminders.send')",
    )
    .await;
    h.create_staff("sender@sealbay.example", "kelp forest at dawn", &["Sender"])
        .await;
    let sender = app.browser();
    sender
        .login("sender@sealbay.example", "kelp forest at dawn")
        .await;
    let form = sender.get("/admin/reminders").await;
    assert_eq!(form.status, 200, "{}", form.body);
    let at = form.body.find("name=\"key\" value=\"").unwrap() + "name=\"key\" value=\"".len();
    let key = &form.body[at..at + form.body[at..].find('"').unwrap()];
    let ids = "ids=68010001%0A68010002";
    let r = sender
        .post("/admin/reminders", &format!("key={key}&{ids}"))
        .await;
    assert!(
        r.body
            .contains("<dt>รหัสนักศึกษา</dt><dd data-names>68010001, 68010002</dd>"),
        "{}",
        r.body
    );
    assert!(!r.body.contains("Karn Seal"), "no names: {}", r.body);
    // An Officer (kernel.student.view) sees the names.
    let (b, key) = officer(&h, &app).await;
    let r = b
        .post("/admin/reminders", &format!("key={key}&{ids}"))
        .await;
    assert!(
        r.body
            .contains("<dt>รายชื่อ</dt><dd data-names>Karn Seal, Mali Tern</dd>"),
        "{}",
        r.body
    );
}

#[tokio::test]
async fn a_list_too_long_for_a_link_is_edited_on_the_review_page() {
    let h = Harness::new();
    let app = boot(&h).await;
    let (b, key) = officer(&h, &app).await;
    let many: Vec<String> = (0..2000).map(|n| format!("6803{n:04}")).collect();
    let r = b
        .post(
            "/admin/reminders",
            &format!("key={key}&ids={}", many.join("%0A")),
        )
        .await;
    assert_eq!(r.status, 200);
    assert!(
        !r.body.contains("/admin/reminders?ids="),
        "no long GET link"
    );
    assert!(r.body.contains(r##"href="#edit-list""##));
    assert!(r.body.contains(r#"<section class="card" id="edit-list">"#));
    assert!(
        r.body.contains(&format!(">{}</textarea>", many.join("\n"))),
        "filled in"
    );
    // The page takes no over-long ?ids=; a short one fills the list.
    let long = format!("/admin/reminders?ids={}", many.join("%0A"));
    let page = b.get(&long).await.body;
    assert!(page.contains(r#"rows="6"></textarea>"#), "left empty");
    let short: Vec<String> = (0..3).map(|n| format!("6803{n:04}")).collect();
    let page = b
        .get(&format!("/admin/reminders?ids={}", short.join("%0A")))
        .await
        .body;
    assert!(page.contains(&format!(">{}</textarea>", short.join("\n"))));
}

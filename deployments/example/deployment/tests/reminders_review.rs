//! The review step of "ส่งเตือนตอนนี้" and the weekly schedule card (#135):
//! who gets it and how, names only for Staff who may view Students, a send
//! to nobody disabled with the reason, long lists edited on the page.

mod reminders_support;

use harpseal_testing::Harness;
use reminders_support::*;

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
        page.contains("ทุกวันจันทร์ เวลา 09:00 น. ถึงนักศึกษาที่ยังศึกษาอยู่และมีป้าย “ค้างชำระค่าธรรมเนียม”"),
        "{page}"
    );
    assert!(!page.contains("fee_due"), "no raw Tag name (#157): {page}");
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

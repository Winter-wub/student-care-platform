//! The Student picker on the reminders form (#146): a textarea of codes
//! without JS, a search gated by the Module's Permission (code prefix or
//! name, limited, escaped), chips that flag unknown and left codes, and
//! "เริ่มจากรายการค้างชำระ" preselecting the Students who owe a fee.

mod reminders_support;

use harpseal_testing::{Harness, a11y};
use reminders_support::*;

const PICK: &str = "/admin/reminders/pick";

fn search(q: &str) -> String {
    format!("{PICK}?q={q}&pf=ids&pmax=40")
}

#[tokio::test]
async fn the_form_keeps_the_plain_textarea_and_adds_the_picker_beside_it() {
    let h = Harness::new();
    let app = boot(&h).await;
    let (b, _) = officer(&h, &app).await;
    let page = b.get("/admin/reminders?ids=68010001%0A68019999").await.body;
    // Without JS: the same field, name and content as before.
    assert!(
        page.contains(r#"<textarea id="ids" name="ids" rows="6">68010001"#),
        "{page}"
    );
    // The picker waits hidden; its value is disabled until the script runs,
    // so the form never posts `ids` twice.
    assert!(page.contains(r#"<div class="picker-js" hidden>"#), "{page}");
    assert!(page.contains(r#"id="ids-value" name="ids" value="68010001"#));
    assert!(page.contains(" disabled>"), "{page}");
    assert!(page.contains(r#"hx-get="/admin/reminders/pick""#), "{page}");
    // Picked chips: the known one named, the unknown one flagged.
    assert!(page.contains("Karn Seal"), "{page}");
    assert!(page.contains("ไม่พบรหัสนี้"), "{page}");
    assert!(page.contains("ไม่พบ 1 รหัส"), "{page}");
    assert!(page.contains("เลือกแล้ว 2 จาก 40 คน"), "{page}");
    assert_eq!(a11y::problems(&page), Vec::<String>::new());
}

#[tokio::test]
async fn search_finds_by_code_prefix_or_name_limited_and_escaped() {
    let h = Harness::new();
    let app = boot(&h).await;
    for i in 0..10 {
        run(
            &h,
            &format!(
                "INSERT INTO student (id, student_code, display_name, locale, created_at, \
                 updated_at) VALUES ('p{i}', '6902000{i}', 'Puffin {i}', 'th', 1, 1)"
            ),
        )
        .await;
    }
    run(
        &h,
        "INSERT INTO student (id, student_code, display_name, locale, created_at, updated_at) \
         VALUES ('x', '68010077', '<script>alert(1)</script> Auk', 'th', 1, 1)",
    )
    .await;
    let (b, _) = officer(&h, &app).await;
    let r = b.get(&search("680100")).await;
    assert_eq!(r.status, 200);
    assert!(r.body.contains("68010001") && r.body.contains("68010077"));
    assert!(!r.body.contains("<script>"), "escaped: {}", r.body);
    assert!(r.body.contains("&lt;script&gt;"), "{}", r.body);
    // A code's middle is not its start.
    assert!(!b.get(&search("0100")).await.body.contains("68010001"));
    // By name, anywhere in it.
    let r = b.get(&search("Tern")).await.body;
    assert!(r.contains("68010002") && r.contains(r#"aria-label="เพิ่ม 68010002""#));
    // At most 8, and a note that there are more.
    let r = b.get(&search("6902")).await.body;
    assert_eq!(r.matches("<li>").count(), 8, "{r}");
    assert!(r.contains("แสดง 8 คนแรก"), "{r}");
    // LIKE wildcards are matched literally.
    let r = b.get(&search("%25")).await.body;
    assert!(r.contains("ไม่พบนักศึกษาที่ตรงกัน"), "{r}");
    // A field name that is not safe for ids and selectors is refused.
    let r = b.get(&format!("{PICK}?q=68&pf=a%22b&pmax=40")).await;
    assert_eq!(r.status, 400);
}

#[tokio::test]
async fn only_staff_with_the_modules_permission_search_and_names_need_student_view() {
    let h = Harness::new();
    let app = boot(&h).await;
    h.create_staff("viewer@sealbay.example", "kelp forest at dawn", &["Viewer"])
        .await;
    let viewer = app.browser();
    viewer
        .login("viewer@sealbay.example", "kelp forest at dawn")
        .await;
    assert_eq!(viewer.get(&search("6801")).await.status, 403);
    // May send the reminder, may not view Students: codes only.
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
    let r = sender.get(&search("6801")).await;
    assert_eq!(r.status, 200);
    assert!(
        r.body.contains("68010001") && !r.body.contains("Karn"),
        "{}",
        r.body
    );
    let r = sender.get(&search("Karn")).await.body;
    assert!(r.contains("ไม่พบนักศึกษาที่ตรงกัน"), "no name search: {r}");
    let page = sender.get("/admin/reminders").await.body;
    assert!(page.contains("ค้นหาจากรหัสนักศึกษา"), "{page}");
}

#[tokio::test]
async fn add_remove_paste_and_clear_answer_the_list_and_one_status_line() {
    let h = Harness::new();
    let app = boot(&h).await;
    let (b, _) = officer(&h, &app).await;
    let edit = |rest: &str| format!("pf=ids&pmax=40&ids=68010001&{rest}");
    let r = b.post(PICK, &edit("op=add&code=68010002")).await;
    assert_eq!(r.status, 200, "{}", r.body);
    assert!(
        r.body.contains(r#"value="68010001&#10;68010002""#)
            || r.body.contains("value=\"68010001\n68010002\""),
        "{}",
        r.body
    );
    assert!(!r.body.contains(" disabled>"), "a swapped value is live");
    assert!(
        r.body.contains("เพิ่ม 68010002 แล้ว เลือกแล้ว 2 คน"),
        "{}",
        r.body
    );
    assert_eq!(r.body.matches("hx-swap-oob").count(), 1, "one status line");
    let r = b.post(PICK, &edit("op=remove&code=68010001")).await;
    assert!(
        r.body.contains("ลบ 68010001 แล้ว เลือกแล้ว 0 คน"),
        "{}",
        r.body
    );
    assert!(r.body.contains("ยังไม่ได้เลือกนักศึกษา"), "{}", r.body);
    let r = b
        .post(PICK, &edit("op=paste&paste=68010003%2C68019999%0A68010001"))
        .await;
    assert!(r.body.contains("เพิ่ม 2 รหัส เลือกแล้ว 3 คน"), "{}", r.body);
    assert!(r.body.contains("ไม่พบ 1 รหัส"), "{}", r.body);
    let r = b.post(PICK, &edit("op=clear")).await;
    assert!(r.body.contains("ล้างรายชื่อแล้ว"), "{}", r.body);
    // Over the limit: flagged, never cut.
    let r = b
        .post(PICK, "pf=ids&pmax=1&ids=68010001&op=add&code=68010002")
        .await;
    assert!(
        r.body.contains("เลือกได้ไม่เกิน 1 คน กรุณาลบออก 1 คนก่อนส่ง"),
        "{}",
        r.body
    );
    // Without the CSRF token: refused.
    let raw = b.post_raw(PICK, &edit("op=clear")).await;
    assert_eq!(raw.status, 403);
    assert_eq!(b.post(PICK, &edit("op=drop")).await.status, 400);
}

#[tokio::test]
async fn starting_from_the_fee_list_preselects_active_students_who_owe() {
    let h = Harness::new();
    let app = boot(&h).await;
    run(
        &h,
        "INSERT INTO student (id, student_code, display_name, locale, status, created_at, \
         updated_at) VALUES ('s9', '68010009', 'Left Puffin', 'th', 'left', 1, 1)",
    )
    .await;
    for (code, satang) in [
        ("68010001", 150_000),
        ("68010002", 0),
        ("68010003", 2_500),
        ("68010009", 9_900),
        ("68019999", 100),
    ] {
        run(
            &h,
            &format!(
                "INSERT INTO fees_balance (student_code, amount_satang, upload_id, updated_at) \
                 VALUES ('{code}', {satang}, 'u1', 1)"
            ),
        )
        .await;
    }
    let (b, _) = officer(&h, &app).await;
    let page = b.get("/admin/reminders").await.body;
    assert!(
        page.contains(r#"href="/admin/reminders?from=fees">เริ่มจากรายการค้างชำระ</a>"#),
        "{page}"
    );
    let page = b.get("/admin/reminders?from=fees").await.body;
    assert!(
        page.contains(">68010001\n68010003</textarea>"),
        "owing, active, on the roster: {page}"
    );
    assert!(page.contains("เลือกนักศึกษาที่ค้างชำระ 2 คนไว้ให้แล้ว"), "{page}");
    assert_eq!(a11y::problems(&page), Vec::<String>::new());
}

#[tokio::test]
async fn more_owing_than_one_send_picks_the_first_forty_and_says_so() {
    let h = Harness::new();
    let app = boot(&h).await;
    for i in 0..45 {
        run(
            &h,
            &format!(
                "INSERT INTO student (id, student_code, display_name, locale, created_at, \
                 updated_at) VALUES ('o{i}', '690300{i:02}', 'Owl {i}', 'th', 1, 1)"
            ),
        )
        .await;
        run(
            &h,
            &format!(
                "INSERT INTO fees_balance (student_code, amount_satang, upload_id, updated_at) \
                 VALUES ('690300{i:02}', 100, 'u1', 1)"
            ),
        )
        .await;
    }
    let (b, _) = officer(&h, &app).await;
    let page = b.get("/admin/reminders?from=fees").await.body;
    assert!(
        page.contains("มีนักศึกษาค้างชำระ 45 คน เลือกไว้ให้ 40 คนแรก"),
        "{page}"
    );
    assert!(
        page.contains("69030039") && !page.contains("69030040"),
        "{page}"
    );
    assert!(page.contains("เลือกแล้ว 40 จาก 40 คน"), "{page}");
}

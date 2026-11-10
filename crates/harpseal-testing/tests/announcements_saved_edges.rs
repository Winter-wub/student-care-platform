//! Saved Audiences (#144), the edges: a hand-picked list with unreachable
//! accounts round-trips, the 100 cap, names already in use, a rename to
//! the same name, an unreadable row, a choice no page offers, a name of
//! zero-width characters, and `?audience=` for a deleted one.

mod announcements_support;

use announcements_support::*;
use harpseal_testing::{Browser, urlencode};

const LIST: &str = "/admin/announcements/audiences";

async fn opened<'a>(w: &'a World) -> Browser<'a> {
    let b = signed_in(w, "Officer").await;
    assert_eq!(b.get("/admin/announcements/new").await.status, 200);
    b
}

async fn save(b: &Browser<'_>, name: &str, audience: &str) -> harpseal_testing::TestResponse {
    let form = format!("{audience}&saved_name={}", urlencode(name));
    b.post(LIST, &form).await
}

async fn insert(w: &World, id: &str, name: &str, stored: &str) {
    run(
        &w.h,
        &format!(
            "INSERT INTO saved_audience (id, name, scope, audience, created_at, updated_at) \
             VALUES ('{id}', '{name}', 'groups', '{stored}', 1, 1)"
        ),
    )
    .await;
}

async fn audited(w: &World, action: &str) -> i64 {
    count(
        w,
        &format!("SELECT COUNT(*) FROM audit_log WHERE action = 'announcement.audience_{action}'"),
    )
    .await
}

#[tokio::test]
async fn a_hand_picked_list_with_unreachable_round_trips() {
    let w = boot().await;
    roster(&w).await;
    let b = opened(&w).await;
    let list = urlencode("code-s1\ncode-s3");
    let r = save(
        &b,
        "รายคน",
        &format!("scope=students&students={list}&unreachable=1&attr.year=2"),
    )
    .await;
    assert!(r.body.contains("flash ok"), "{}", r.body);
    let stored = texts(&w, "SELECT scope || ' ' || audience FROM saved_audience").await;
    assert_eq!(
        stored,
        [
            r#"students {"attributes":{},"include_unreachable":true,"students":["code-s1","code-s3"],"tags":[],"v":1}"#
        ],
        "the year typed under another choice is not kept"
    );
    let id = texts(&w, "SELECT id FROM saved_audience").await.remove(0);
    let used = b
        .post(&format!("{LIST}/use"), &format!("saved_audience={id}"))
        .await;
    let new = b
        .get(&format!("/admin/announcements/new?audience={id}"))
        .await;
    for body in [&used.body, &new.body] {
        assert!(
            body.contains(r#"value="students" required checked"#),
            "{body}"
        );
        assert!(body.contains(">code-s1\ncode-s3</textarea>"), "{body}");
        assert!(
            body.contains(r#"name="unreachable" value="1" checked"#),
            "{body}"
        );
        assert!(body.contains("ใช้กลุ่มผู้รับ &quot;รายคน&quot; แล้ว"), "{body}");
    }
    // The pick is said in the live region too, out of band.
    assert!(used.body.contains(r#"id="saved-status""#), "{}", used.body);
    assert!(
        used.body.contains(r#"hx-swap-oob="innerHTML""#),
        "{}",
        used.body
    );
}

#[tokio::test]
async fn at_most_one_hundred_are_saved() {
    let w = boot().await;
    roster(&w).await;
    for i in 0..100 {
        insert(&w, &format!("sa-{i}"), &format!("กลุ่ม {i}"), r#"{"v":1}"#).await;
    }
    let b = opened(&w).await;
    let r = save(&b, "กลุ่มที่ 101", "scope=everyone").await;
    assert!(r.body.contains("สูงสุด 100 กลุ่ม"), "{}", r.body);
    assert_eq!(count(&w, "SELECT COUNT(*) FROM saved_audience").await, 100);
    assert_eq!(audited(&w, "save").await, 0);
}

#[tokio::test]
async fn names_in_use_and_unchanged_names() {
    let w = boot().await;
    roster(&w).await;
    insert(&w, "sa-a", "ปีหนึ่ง", r#"{"v":1}"#).await;
    insert(&w, "sa-b", "ปีสอง", r#"{"v":1}"#).await;
    let b = opened(&w).await;
    let r = b
        .post(
            &format!("{LIST}/sa-b/rename"),
            &format!("name={}", urlencode("ปีหนึ่ง")),
        )
        .await;
    assert_eq!(r.status, 303);
    let page = b.get(LIST).await;
    assert!(page.body.contains("มีกลุ่มผู้รับชื่อนี้แล้ว"), "{}", page.body);
    let names = texts(&w, "SELECT name FROM saved_audience ORDER BY id").await;
    assert_eq!(names, ["ปีหนึ่ง", "ปีสอง"]);
    // The same name (white space aside): nothing written, nothing audited.
    let r = b
        .post(
            &format!("{LIST}/sa-b/rename"),
            &format!("name={}", urlencode(" ปีสอง ")),
        )
        .await;
    assert_eq!(r.status, 303);
    assert!(b.get(LIST).await.body.contains("ชื่อเหมือนเดิม"));
    assert_eq!(audited(&w, "rename").await, 0);
    // A name of zero-width characters only is blank, on save and rename.
    let zw = urlencode("\u{200B}\u{FEFF} ");
    let r = save(&b, "\u{200B}\u{FEFF} ", "scope=everyone").await;
    assert!(r.body.contains("ใส่ชื่อกลุ่มผู้รับ"), "{}", r.body);
    b.post(&format!("{LIST}/sa-b/rename"), &format!("name={zw}"))
        .await;
    assert!(b.get(LIST).await.body.contains("ใส่ชื่อกลุ่มผู้รับ"));
    // A choice no page offers is refused in Thai, not by the database.
    let r = save(&b, "แปลก", "scope=nobody&attr.year=2").await;
    assert_eq!(r.status, 200, "{}", r.body);
    assert!(r.body.contains("ยังไม่ได้เลือกผู้รับ"), "{}", r.body);
    assert_eq!(count(&w, "SELECT COUNT(*) FROM saved_audience").await, 2);
}

#[tokio::test]
async fn an_unreadable_row_is_never_everyone_and_only_deleted() {
    let w = boot().await;
    roster(&w).await;
    // Broken JSON, and a version this build does not know.
    insert(&w, "sa-broken", "เสีย", "{not json").await;
    insert(&w, "sa-later", "รุ่นใหม่", r#"{"v":9,"tags":["ชมรม"]}"#).await;
    let b = opened(&w).await;
    let page = b.get(LIST).await;
    assert_eq!(page.status, 200);
    assert_eq!(page.body.matches("อ่านกลุ่มนี้ไม่ได้ ลบแล้วบันทึกใหม่").count(), 2);
    assert!(
        !page.body.contains("ทุกคน"),
        "never read as everyone: {}",
        page.body
    );
    assert!(!page.body.contains("?audience=sa-broken"), "{}", page.body);
    assert!(!page.body.contains("sa-broken/rename"), "{}", page.body);
    let picker = b.get(&format!("{LIST}/picker")).await;
    assert!(!picker.body.contains("sa-broken"), "{}", picker.body);

    let used = b
        .post(
            &format!("{LIST}/use"),
            "saved_audience=sa-later&scope=groups&attr.year=3",
        )
        .await;
    assert!(used.body.contains("อ่านกลุ่มผู้รับที่เลือกไม่ได้"), "{}", used.body);
    assert!(
        used.body.contains(r#"value="3""#),
        "the form is unchanged: {}",
        used.body
    );
    let new = b.get("/admin/announcements/new?audience=sa-broken").await;
    assert!(new.body.contains("อ่านกลุ่มผู้รับที่เลือกไม่ได้"), "{}", new.body);
    assert!(!new.body.contains(r#"value="everyone" required checked"#));

    let r = b
        .post(&format!("{LIST}/sa-broken/delete"), "confirm=yes")
        .await;
    assert_eq!(r.status, 303);
    assert_eq!(audited(&w, "delete").await, 1);
    let left = texts(&w, "SELECT id FROM saved_audience").await;
    assert_eq!(left, ["sa-later"]);
}

#[tokio::test]
async fn a_deleted_one_in_the_link_says_so() {
    let w = boot().await;
    roster(&w).await;
    let b = opened(&w).await;
    let new = b.get("/admin/announcements/new?audience=sa-gone").await;
    assert_eq!(new.status, 200);
    assert!(new.body.contains("ไม่พบกลุ่มผู้รับที่เลือก"), "{}", new.body);
    assert!(new.body.contains(r#"id="saved-note""#), "{}", new.body);
}

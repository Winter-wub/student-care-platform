//! Saved Audiences (#144): save the editor's Audience under a name, pick
//! it again, rename and delete it, each write audited and gated by
//! `announcements.send`; the rules are kept, not the Students, so the
//! reach is counted again; a value `[roster]` no longer has is shown and
//! refused, never widened to everyone.

mod announcements_support;

use announcements_support::*;
use harpseal_testing::{Browser, urlencode};

const LIST: &str = "/admin/announcements/audiences";

async fn opened<'a>(w: &'a World, role: &str) -> Browser<'a> {
    let b = signed_in(w, role).await;
    assert_eq!(b.get("/admin/announcements/new").await.status, 200);
    b
}

async fn save(b: &Browser<'_>, name: &str, audience: &str) -> harpseal_testing::TestResponse {
    let form = format!("{audience}&saved_name={}", urlencode(name));
    b.post(LIST, &form).await
}

async fn saved_id(w: &World, name: &str) -> String {
    let rows = texts(
        w,
        &format!("SELECT id FROM saved_audience WHERE name = '{name}'"),
    )
    .await;
    rows.into_iter().next().expect("saved")
}

/// A saved Audience row as the Module stores it (`"v": 1` added).
async fn insert(w: &World, id: &str, name: &str, scope: &str, audience: &str) {
    let stored = audience.replacen('{', r#"{"v":1,"#, 1).replace(",}", "}");
    run(
        &w.h,
        &format!(
            "INSERT INTO saved_audience (id, name, scope, audience, created_at, updated_at) \
             VALUES ('{id}', '{name}', '{scope}', '{stored}', 1, 1)"
        ),
    )
    .await;
}

/// The number in the count box's "นักศึกษา N".
fn counted(body: &str) -> String {
    let at = body.find("</strong>").expect("a count");
    body[..at].rsplit(' ').next().unwrap().to_string()
}

#[tokio::test]
async fn save_pick_rename_and_delete_each_audited() {
    let w = boot().await;
    roster(&w).await;
    let b = opened(&w, "Officer").await;
    // Saved: the rules of the choice only (the list typed under "groups"
    // is not kept), never the Students they select.
    let r = save(
        &b,
        "ปีสอง MSC",
        "scope=groups&attr.programme=MSC&attr.year=2&students=code-s4",
    )
    .await;
    assert_eq!(r.status, 200, "{}", r.body);
    assert!(
        r.body.contains("บันทึกกลุ่มผู้รับ &quot;ปีสอง MSC&quot; แล้ว"),
        "{}",
        r.body
    );
    assert_eq!(r.header("hx-trigger"), Some("saved-audiences-changed"));
    let id = saved_id(&w, "ปีสอง MSC").await;
    let stored = texts(&w, "SELECT scope || ' ' || audience FROM saved_audience").await;
    assert_eq!(
        stored,
        [
            r#"groups {"attributes":{"programme":["MSC"],"year":["2"]},"include_unreachable":false,"tags":[],"v":1}"#
        ]
    );
    // The same name again is refused, and so is an Audience with no choice.
    let again = save(&b, "ปีสอง MSC", "scope=everyone").await;
    assert!(again.body.contains("มีกลุ่มผู้รับชื่อนี้แล้ว"), "{}", again.body);
    let none = save(&b, "ว่าง", "").await;
    assert!(none.body.contains("ยังไม่ได้เลือกผู้รับ"), "{}", none.body);
    let blank = save(&b, "  ", "scope=everyone").await;
    assert!(blank.body.contains("ใส่ชื่อกลุ่มผู้รับ"), "{}", blank.body);
    assert_eq!(count(&w, "SELECT COUNT(*) FROM saved_audience").await, 1);

    // Picked: the picker lists it, and "use" fills the fieldset.
    let picker = b.get(&format!("{LIST}/picker")).await;
    assert!(
        picker.body.contains(&format!("value=\"{id}\"")),
        "{}",
        picker.body
    );
    let used = b
        .post(
            &format!("{LIST}/use"),
            &format!("saved_audience={id}&scope=everyone"),
        )
        .await;
    assert_eq!(used.status, 200, "{}", used.body);
    assert!(used.body.contains("id=\"audience\""), "{}", used.body);
    assert!(used.body.contains("ใช้กลุ่มผู้รับ &quot;ปีสอง MSC&quot; แล้ว"));
    assert!(
        used.body.contains(r#"value="groups" required checked"#),
        "{}",
        used.body
    );
    assert!(
        used.body.contains(r#"value="MSC" checked"#),
        "{}",
        used.body
    );
    assert!(used.body.contains(r#"value="2""#), "{}", used.body);
    // `?audience=` on a new announcement starts from it too.
    let new = b
        .get(&format!("/admin/announcements/new?audience={id}"))
        .await;
    assert!(new.body.contains(r#"value="MSC" checked"#), "{}", new.body);

    // Listed, renamed, deleted.
    let page = b.get(LIST).await;
    assert_eq!(page.status, 200);
    assert!(page.body.contains("ปีสอง MSC"), "{}", page.body);
    let r = b
        .post(
            &format!("{LIST}/{id}/rename"),
            &format!("name={}", urlencode("ปี 2 MSC")),
        )
        .await;
    assert_eq!(r.status, 303, "{}", r.body);
    assert_eq!(
        texts(&w, "SELECT name FROM saved_audience").await,
        ["ปี 2 MSC"]
    );
    // Deleted only through the confirm step.
    let step = b.get(&format!("{LIST}/{id}/delete")).await;
    assert_eq!(step.status, 200, "{}", step.body);
    assert!(
        step.body.contains(r#"name="confirm" value="yes""#),
        "{}",
        step.body
    );
    let r = b.post(&format!("{LIST}/{id}/delete"), "").await;
    assert_eq!(r.status, 303, "{}", r.body);
    assert_eq!(count(&w, "SELECT COUNT(*) FROM saved_audience").await, 1);
    let r = b.post(&format!("{LIST}/{id}/delete"), "confirm=yes").await;
    assert_eq!(r.status, 303, "{}", r.body);
    assert_eq!(count(&w, "SELECT COUNT(*) FROM saved_audience").await, 0);
    let r = b.post(&format!("{LIST}/{id}/delete"), "confirm=yes").await;
    assert_eq!(r.status, 303, "a second delete is a flash, not an error");

    let actions = texts(
        &w,
        "SELECT action FROM audit_log WHERE action LIKE 'announcement.audience_%' ORDER BY rowid",
    )
    .await;
    assert_eq!(
        actions,
        [
            "announcement.audience_save",
            "announcement.audience_rename",
            "announcement.audience_delete"
        ]
    );
    let hand_picked = count(
        &w,
        "SELECT COUNT(*) FROM audit_log WHERE action LIKE 'announcement.audience_%' \
         AND COALESCE(after, '') || COALESCE(before, '') LIKE '%code-s%'",
    )
    .await;
    assert_eq!(hand_picked, 0, "audit rows never hold student ids");
}

#[tokio::test]
async fn every_page_needs_announcements_send() {
    let w = boot().await;
    roster(&w).await;
    insert(&w, "sa-1", "ทุกคน", "everyone", "{}").await;
    let v = signed_in(&w, "Viewer").await;
    assert_eq!(v.get(LIST).await.status, 403);
    assert_eq!(v.get(&format!("{LIST}/picker")).await.status, 403);
    assert_eq!(save(&v, "x", "scope=everyone").await.status, 403);
    assert_eq!(
        v.post(&format!("{LIST}/use"), "saved_audience=sa-1")
            .await
            .status,
        403
    );
    assert_eq!(
        v.post(&format!("{LIST}/sa-1/rename"), "name=y")
            .await
            .status,
        403
    );
    assert_eq!(v.get(&format!("{LIST}/sa-1/delete")).await.status, 403);
    assert_eq!(
        v.post(&format!("{LIST}/sa-1/delete"), "confirm=yes")
            .await
            .status,
        403
    );
    assert_eq!(texts(&w, "SELECT name FROM saved_audience").await, ["ทุกคน"]);
    // A Sender may.
    let s = signed_in(&w, "Sender").await;
    assert_eq!(s.get(LIST).await.status, 200);
}

#[tokio::test]
async fn a_value_the_roster_no_longer_has_warns_and_never_widens() {
    let w = boot().await;
    roster(&w).await;
    // "Fisheries-Old" left `[roster.attributes.programme] values`;
    // `faculty` left `[roster]` altogether.
    insert(
        &w,
        "sa-old",
        "หลักสูตรเก่า",
        "groups",
        r#"{"attributes":{"programme":["Fisheries-Old"],"year":["2"]}}"#,
    )
    .await;
    insert(
        &w,
        "sa-gone",
        "คณะเดิม",
        "groups",
        r#"{"attributes":{"faculty":["SCI"]}}"#,
    )
    .await;
    let b = opened(&w, "Officer").await;
    let page = b.get(LIST).await;
    assert_eq!(page.status, 200, "{}", page.body);
    assert!(page.body.contains("หลักสูตร: Fisheries-Old"), "{}", page.body);
    assert!(page.body.contains("faculty: SCI"), "{}", page.body);

    for (id, value) in [("sa-old", "Fisheries-Old"), ("sa-gone", "SCI")] {
        let used = b
            .post(&format!("{LIST}/use"), &format!("saved_audience={id}"))
            .await;
        assert_eq!(used.status, 200, "{}", used.body);
        assert!(used.body.contains("ไม่มีในข้อมูลนักศึกษาแล้ว"), "{}", used.body);
        assert!(
            used.body.contains(&format!(r#"value="{value}" checked"#)),
            "the stale value stays marked: {}",
            used.body
        );
    }

    // Posted back as the fieldset holds it: the count never shows
    // everyone, and the draft is refused.
    let old = "scope=groups&attr.programme=Fisheries-Old&attr.year=2";
    let c = b.post("/admin/announcements/audience", old).await;
    assert_eq!(counted(&c.body), "0", "{}", c.body);
    let c = b
        .post(
            "/admin/announcements/audience",
            "scope=groups&attr.faculty=SCI",
        )
        .await;
    assert!(!c.body.contains("</strong>"), "no count: {}", c.body);
    for audience in [old, "scope=groups&attr.faculty=SCI"] {
        let r = b
            .post(
                "/admin/announcements/new",
                &form("เก่า", "ข้อความ", &format!("&{audience}")),
            )
            .await;
        assert_eq!(r.status, 422, "{}", r.body);
    }
    assert_eq!(count(&w, "SELECT COUNT(*) FROM announcement").await, 0);
    // Nor can it be saved again as it is.
    let r = save(&b, "อีกครั้ง", old).await;
    assert!(r.body.contains("ไม่มีในข้อมูลนักศึกษาแล้ว"), "{}", r.body);
}

#[tokio::test]
async fn the_reach_is_counted_again_not_stored() {
    let w = boot().await;
    roster(&w).await;
    let b = opened(&w, "Officer").await;
    let rules = "scope=groups&attr.year=2";
    assert_eq!(save(&b, "ปีสอง", rules).await.status, 200);
    let c = b.post("/admin/announcements/audience", rules).await;
    assert_eq!(counted(&c.body), "2", "s1 and s3: {}", c.body);
    // A new Student in year 2 after the save is in the reach.
    student(
        &w.h,
        "s6",
        "th",
        "active",
        r#"{"programme":"MSC","year":2}"#,
    )
    .await;
    link(&w.h, "s6", "fake", "active").await;
    let id = saved_id(&w, "ปีสอง").await;
    let used = b
        .post(&format!("{LIST}/use"), &format!("saved_audience={id}"))
        .await;
    assert!(used.body.contains(r#"value="2""#), "{}", used.body);
    let c = b.post("/admin/announcements/audience", rules).await;
    assert_eq!(counted(&c.body), "3", "{}", c.body);
    // And on the review step.
    let r = b
        .post(
            "/admin/announcements/new",
            &form("ปีสอง", "ข้อความ", &format!("&{rules}&do=confirm")),
        )
        .await;
    assert_eq!(r.status, 303, "{}", r.body);
    let review = b.get(r.header("location").unwrap()).await;
    let at = review.body.find(r#"data-count="students""#).expect("reach");
    assert!(
        review.body[at..at + 80].contains('3'),
        "{}",
        &review.body[at..]
    );
}

#[tokio::test]
async fn the_pages_pass_the_accessibility_checks() {
    let w = boot().await;
    roster(&w).await;
    insert(
        &w,
        "sa-1",
        "ปีสอง",
        "groups",
        r#"{"attributes":{"year":["2"]}}"#,
    )
    .await;
    insert(
        &w,
        "sa-old",
        "หลักสูตรเก่า",
        "groups",
        r#"{"attributes":{"programme":["Fisheries-Old"]}}"#,
    )
    .await;
    let b = opened(&w, "Officer").await;
    for path in [
        LIST.to_string(),
        "/admin/announcements/new?audience=sa-1".into(),
        "/admin/announcements/new?audience=sa-old".into(),
        format!("{LIST}/sa-1/delete"),
    ] {
        let page = b.get(&path).await;
        assert_eq!(page.status, 200, "{path}");
        assert_eq!(
            harpseal_testing::a11y::problems(&page.body),
            Vec::<String>::new(),
            "{path}"
        );
    }
}

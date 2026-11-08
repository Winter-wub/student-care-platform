//! Announcements to a hand-picked list of Students (#76) at the Deployment
//! seam: pasted student ids and the picker, ANDed with the other filters,
//! the preview of what will not be sent, the stored list and the audit.

mod announcements_support;

use announcements_support::*;
use harpseal::audience::{self, Audience};
use harpseal_testing::urlencode;

/// The standard roster plus s5: active, never linked.
async fn world() -> World {
    let w = boot().await;
    roster(&w).await;
    student(
        &w.h,
        "s5",
        "th",
        "active",
        r#"{"programme":"MSC","year":2}"#,
    )
    .await;
    w
}

fn ids(list: &[&str]) -> String {
    format!("&students={}", urlencode(&list.join("\n")))
}

async fn delivered(w: &World) -> Vec<String> {
    texts(
        w,
        "SELECT DISTINCT student_id FROM announcement_delivery ORDER BY student_id",
    )
    .await
}

async fn sent(w: &World, b: &harpseal_testing::Browser<'_>, extra: &str) -> String {
    let id = create(
        b,
        &form("รายชื่อ", "เฉพาะบางคน", &format!("{extra}&do=confirm")),
    )
    .await;
    drain(w).await;
    id
}

#[tokio::test]
async fn a_list_alone_is_the_whole_audience() {
    let w = world().await;
    let b = signed_in(&w, "Officer").await;
    // s1 is not listed; s5 has no Link, s2 left, "nope" is nobody.
    let list = ids(&["code-s3", "code-s4", "code-s5", "code-s2", "nope-0000"]);
    sent(&w, &b, &list).await;
    assert_eq!(delivered(&w).await, ["s3", "s4"]);
    assert_eq!(w.line.sent_to(&fake_account("fake", "s3")).len(), 1);
    assert_eq!(w.tg.sent_to(&fake_account(TG, "s4")).len(), 1);
    assert!(w.line.sent_to(&fake_account("fake", "s1")).is_empty());
    assert!(w.tg.sent_to(&fake_account(TG, "s1")).is_empty());
}

#[tokio::test]
async fn a_list_is_anded_with_the_other_filters() {
    let w = world().await;
    let b = signed_in(&w, "Officer").await;
    // s1, s3: year 2; s4: year 3. Year 2 alone would reach s1 and s3; on
    // the list [s1, s4] only s1 is both listed and in year 2. Posted
    // without a choice, as an older page does: since #120 the form's
    // choices keep only their own filters, the Audience still ANDs them.
    let extra = format!("{}&attr.year=2", ids(&["code-s1", "code-s4"]));
    sent(&w, &b, &extra).await;
    assert_eq!(delivered(&w).await, ["s1"]);
}

#[tokio::test]
async fn ids_are_split_trimmed_and_deduplicated() {
    let w = world().await;
    let b = signed_in(&w, "Officer").await;
    let raw = " code-s3 ,code-s3\r\ncode-s1  code-s4,, \n";
    let r = b
        .post(
            "/admin/announcements/audience",
            &format!("students={}", urlencode(raw)),
        )
        .await;
    assert_eq!(r.status, 200, "{}", r.body);
    assert!(r.body.contains("รหัสในรายชื่อ 3"), "{}", r.body);
    let id = create(
        &b,
        &form("ก", "ข", &format!("&students={}", urlencode(raw))),
    )
    .await;
    assert_eq!(
        texts(
            &w,
            &format!("SELECT audience FROM announcement WHERE id = '{id}'")
        )
        .await,
        [
            r#"{"attributes":{},"tags":[],"students":["code-s3","code-s1","code-s4"],"include_unreachable":false}"#
        ]
    );
}

#[tokio::test]
async fn the_preview_lists_unknown_and_unlinked_ids() {
    let w = world().await;
    let b = signed_in(&w, "Officer").await;
    let post = |extra: String| {
        let b = &b;
        async move { b.post("/admin/announcements/audience", &extra).await }
    };
    let r = post(ids(&[
        "code-s1",
        "code-s3",
        "code-s5",
        "code-s2",
        "nope-0000",
        "x",
    ]))
    .await;
    assert_eq!(r.status, 200);
    // Two Students will get it: s1 and s3.
    assert!(r.body.contains("<strong>นักศึกษา 2</strong>"), "{}", r.body);
    let unknown = r
        .body
        .split(r#"id="hand-picked-unknown""#)
        .nth(1)
        .expect("unknown listed");
    let unknown = &unknown[..unknown.find("</p>").unwrap()];
    assert!(
        unknown.contains("nope-0000") && unknown.contains('x'),
        "{unknown}"
    );
    assert!(!unknown.contains("code-s5"));
    let unlinked = r
        .body
        .split(r#"id="hand-picked-unlinked""#)
        .nth(1)
        .expect("unlinked listed");
    let unlinked = &unlinked[..unlinked.find("</p>").unwrap()];
    assert!(
        unlinked.contains("code-s5") && unlinked.contains("code-s2"),
        "{unlinked}"
    );
    assert!(!unlinked.contains("code-s1"));
    assert!(!r.body.contains("hand-picked-filtered"));

    // A linked Student the other filters leave out is counted, not listed.
    let r = post(format!("{}&attr.year=3", ids(&["code-s1", "code-s4"]))).await;
    assert!(r.body.contains("<strong>นักศึกษา 1</strong>"), "{}", r.body);
    assert!(r.body.contains("hand-picked-filtered"), "{}", r.body);

    // No list: no report.
    let r = post("attr.year=3".into()).await;
    assert!(!r.body.contains("hand-picked-report"));
}

#[tokio::test]
async fn an_unreachable_link_is_live_only_when_counted() {
    let w = world().await;
    link(&w.h, "s5", "fake", "unreachable").await;
    let d = w.app.deployment();
    let mut a = Audience::everyone();
    a.students = vec!["code-s5".into(), "code-s3".into()];
    let r = audience::hand_picked(d, &a).await.unwrap();
    assert_eq!(
        (r.linked, r.unlinked.clone()),
        (1, vec!["code-s5".to_string()])
    );
    a.include_unreachable = true;
    let r = audience::hand_picked(d, &a).await.unwrap();
    assert_eq!((r.linked, r.unlinked.len()), (2, 0));
}

#[tokio::test]
async fn the_list_is_stored_shown_and_audited_by_size() {
    let w = world().await;
    let b = signed_in(&w, "Officer").await;
    let id = sent(&w, &b, &ids(&["code-s3", "code-s4", "nope-0000"])).await;
    assert_eq!(
        texts(
            &w,
            &format!("SELECT audience ->> '$.students[1]' FROM announcement WHERE id = '{id}'")
        )
        .await,
        ["code-s4"]
    );
    // Visible to Staff on the detail page and in the edit form of a draft.
    let detail = b.get(&format!("/admin/announcements/{id}")).await;
    assert!(
        detail.body.contains("code-s3, code-s4, nope-0000"),
        "{}",
        detail.body
    );
    // The audit rows carry the size, never the ids.
    for action in ["announcement.create", "announcement.send"] {
        let after = texts(
            &w,
            &format!("SELECT after FROM audit_log WHERE action = '{action}'"),
        )
        .await;
        assert_eq!(after.len(), 1, "{action}");
        assert!(!after[0].contains("code-s"), "{}", after[0]);
    }
    assert_eq!(
        texts(&w, "SELECT after ->> '$.audience.hand_picked' FROM audit_log WHERE action = 'announcement.create'").await,
        ["3"]
    );
    assert_eq!(
        texts(
            &w,
            "SELECT after ->> '$.hand_picked' FROM audit_log WHERE action = 'announcement.send'"
        )
        .await,
        ["3"]
    );
    // An announcement without a list keeps its audit rows as before.
    let plain = create(&b, &form("ก", "ข", "")).await;
    assert_eq!(
        texts(&w, &format!("SELECT count(*) FROM audit_log WHERE target_id = '{plain}' AND after LIKE '%hand_picked%'")).await,
        ["0"]
    );
}

#[tokio::test]
async fn a_draft_edit_keeps_and_changes_the_list() {
    let w = world().await;
    let b = signed_in(&w, "Officer").await;
    let id = create(&b, &form("ก", "ข", &ids(&["code-s1", "code-s3"]))).await;
    let edit = format!("/admin/announcements/{id}/edit");
    let page = b.get(&edit).await;
    assert!(page.body.contains("code-s1\ncode-s3"), "{}", page.body);
    let v = version(&w, &id).await;
    let r = b
        .post(
            &edit,
            &format!("{}&version={v}{}", form("ก", "ข", ""), ids(&["code-s4"])),
        )
        .await;
    assert_eq!(r.status, 303, "{}", r.body);
    assert_eq!(
        texts(
            &w,
            &format!("SELECT audience ->> '$.students' FROM announcement WHERE id = '{id}'")
        )
        .await,
        [r#"["code-s4"]"#]
    );
}

#[tokio::test]
async fn the_list_is_capped_at_two_thousand_ids() {
    let w = world().await;
    let b = signed_in(&w, "Officer").await;
    let many = |n: usize| -> String {
        let list: Vec<String> = (0..n).map(|i| format!("id-{i:05}")).collect();
        format!("&students={}", urlencode(&list.join("\n")))
    };
    let r = b
        .post("/admin/announcements/new", &form("ก", "ข", &many(2001)))
        .await;
    assert_eq!(r.status, 422, "{}", r.body);
    assert!(r.body.contains("2000"), "{}", r.body);
    assert_eq!(texts(&w, "SELECT count(*) FROM announcement").await, ["0"]);
    let r = b
        .post("/admin/announcements/audience", &many(2001)[1..])
        .await;
    assert!(r.body.contains("2000"), "{}", r.body);
    // Duplicates do not count twice; exactly 2000 is fine.
    let r = b
        .post(
            "/admin/announcements/new",
            &form("ก", "ข", &format!("{}{}", many(2000), many(10))),
        )
        .await;
    assert_eq!(r.status, 303, "{}", r.body);
    // From code: the same cap.
    let mut a = Audience::everyone();
    a.students = (0..2001).map(|i| format!("id-{i:05}")).collect();
    assert!(audience::resolve(w.app.deployment(), &a).await.is_err());
}

#[tokio::test]
async fn the_picker_searches_students_and_adds_one_id() {
    let w = world().await;
    let b = signed_in(&w, "Officer").await;
    let page = b.get("/admin/announcements/new").await;
    assert!(
        page.body.contains(r#"name="students""#) && page.body.contains("/admin/announcements/pick")
    );
    let r = b.get("/admin/announcements/pick?q=Name%20s3").await;
    assert_eq!(r.status, 200);
    assert!(
        r.body.contains("code-s3") && r.body.contains("Name s3"),
        "{}",
        r.body
    );
    assert!(!r.body.contains("code-s1"));
    assert_eq!(b.get("/admin/announcements/pick?q=zzz").await.status, 200);
    // Adding appends to what is typed, once, and asks for a new count.
    let add = |code: &str| format!("students={}&add={code}", urlencode("code-s1\ncode-s3"));
    let r = b.post("/admin/announcements/pick", &add("code-s4")).await;
    assert_eq!(r.status, 200, "{}", r.body);
    assert_eq!(r.header("hx-trigger"), Some("students-changed"));
    assert!(
        r.body.contains("code-s1\ncode-s3\ncode-s4</textarea>"),
        "{}",
        r.body
    );
    let r = b.post("/admin/announcements/pick", &add("code-s3")).await;
    assert!(r.body.contains("code-s1\ncode-s3</textarea>"), "{}", r.body);
}

#[tokio::test]
async fn the_picker_needs_permission_to_see_students() {
    let w = world().await;
    let b = signed_in(&w, "Sender").await;
    let page = b.get("/admin/announcements/new").await;
    assert_eq!(page.status, 200);
    assert!(
        page.body.contains(r#"name="students""#),
        "pasting still works"
    );
    assert!(!page.body.contains("pick-q"));
    assert_eq!(b.get("/admin/announcements/pick?q=Name").await.status, 403);
    assert_eq!(
        b.post("/admin/announcements/pick", "students=&add=code-s1")
            .await
            .status,
        403
    );
    // A Viewer cannot even reach it.
    let v = signed_in(&w, "Viewer").await;
    assert_eq!(v.get("/admin/announcements/pick?q=Name").await.status, 403);
}

#[tokio::test]
async fn ids_dropped_for_their_shape_still_count_against_the_cap() {
    let w = world().await;
    let b = signed_in(&w, "Officer").await;
    // 1,900 well-formed ids and 200 that fit no pattern: 2,100 in all.
    let mut list: Vec<String> = (0..1900).map(|i| format!("id-{i:05}")).collect();
    list.extend((0..200).map(|i| format!("x{i}")));
    let mut a = Audience::everyone();
    a.students = list.clone();
    assert!(audience::resolve(w.app.deployment(), &a).await.is_err());
    assert!(audience::count(w.app.deployment(), &a).await.is_err());
    // The preview says why, and Save refuses with the same message.
    let form_list = format!("students={}", urlencode(&list.join("\n")));
    let preview = b.post("/admin/announcements/audience", &form_list).await;
    assert!(preview.body.contains("2000"), "{}", preview.body);
    assert!(!preview.body.contains("hand-picked-report"));
    let save = b
        .post(
            "/admin/announcements/new",
            &form("ก", "ข", &format!("&{form_list}")),
        )
        .await;
    assert_eq!(save.status, 422);
    // Just under the cap, the same mix resolves (to nobody).
    a.students.truncate(2000);
    assert!(
        audience::resolve(w.app.deployment(), &a)
            .await
            .unwrap()
            .is_empty()
    );
}

#[test]
fn pasted_ids_split_on_semicolons_and_lose_zero_width_characters() {
    let raw = "code-s1;code-s3\u{200B}\ncode\u{FEFF}-s4\u{200C},\u{200D}code-s1 ;; ";
    assert_eq!(
        audience::parse_student_ids(raw),
        ["code-s1", "code-s3", "code-s4"]
    );
    assert!(audience::parse_student_ids("\u{200B}\u{FEFF} ; ,").is_empty());
}

#[tokio::test]
async fn a_spreadsheet_paste_reaches_the_right_students() {
    let w = world().await;
    let b = signed_in(&w, "Officer").await;
    sent(&w, &b, &ids(&["code-s3;\u{200B}code-s4\u{FEFF}"])).await;
    assert_eq!(delivered(&w).await, ["s3", "s4"]);
}

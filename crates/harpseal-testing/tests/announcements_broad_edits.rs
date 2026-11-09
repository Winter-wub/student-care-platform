//! #120 review round: a scheduled announcement whose Audience changes goes
//! back through the review step (no plain save, no crafted POST skips it);
//! the over-quota line says what really happens per LINE package; the
//! danger label names reachable Students; a choice drops the other
//! choice's filters.

mod announcements_support;

use announcements_support::*;
use harpseal::Statement;

/// 2026-11-05 07:00 in +07:00 is the harness START; this is 09:30.
const LATER: &str = "&when=later&send_at=2026-11-05T09%3A30";
const DANGER: &str = r#"<button class="danger" type="submit">"#;
const STATUS: &str = "SELECT status FROM announcement";
/// The parent deliver Jobs still queued.
const QUEUED: &str =
    "SELECT count(*) FROM job WHERE kind = 'announcements.deliver' AND state = 'queued'";

/// A send scheduled for 09:30 to programme MSC (s1, s4: 3 accounts).
async fn scheduled_group(w: &World, b: &harpseal_testing::Browser<'_>) -> String {
    let extra = format!("&scope=groups&attr.programme=MSC{LATER}&do=confirm");
    let id = create(b, &form("นัดหมาย", "พบอาจารย์", &extra)).await;
    assert_eq!(texts(w, STATUS).await, ["scheduled"]);
    id
}

async fn edit(
    w: &World,
    b: &harpseal_testing::Browser<'_>,
    id: &str,
    extra: &str,
) -> harpseal_testing::TestResponse {
    let path = format!("/admin/announcements/{id}/edit");
    assert_eq!(b.get(&path).await.status, 200);
    let v = version(w, id).await;
    b.post(
        &path,
        &form("นัดหมาย", "พบอาจารย์", &format!("{extra}{LATER}&version={v}")),
    )
    .await
}

/// Past 09:30: what went out.
async fn after_send_at(w: &World) -> usize {
    w.h.clock().advance_minutes(160);
    drain(w).await;
    w.line.sent().len() + w.tg.sent().len()
}

#[tokio::test]
async fn a_scheduled_group_send_edited_to_everyone_is_held_for_review() {
    let w = boot().await;
    roster(&w).await;
    let b = signed_in(&w, "Officer").await;
    let id = scheduled_group(&w, &b).await;
    // A plain "บันทึก" with everyone: held, not saved onto the schedule.
    let r = edit(&w, &b, &id, "&scope=everyone").await;
    assert_eq!(r.status, 303, "{}", r.body);
    let held = format!("/admin/announcements/{id}/review?held=1");
    assert_eq!(r.header("location"), Some(held.as_str()));
    assert!(
        b.cookie("hs_flash").is_none(),
        "an info note, not an error flash"
    );
    assert_eq!(texts(&w, STATUS).await, ["draft"]);
    assert_eq!(count(&w, QUEUED).await, 0, "the schedule's Job is gone");
    let page = b.get(&held).await.body;
    assert!(
        page.contains(r#"<p class="note" role="status">ผู้รับเปลี่ยน จึงพักกำหนดส่งไว้ก่อน"#),
        "{page}"
    );
    assert!(
        page.contains(&format!("{DANGER}ส่งถึงทุกคน 3 คน</button>")),
        "{page}"
    );
    assert!(page.contains("ส่งตามเวลา 5 พ.ย. 2569 09:30"), "{page}");
    // Without the review's reach it is refused; at 09:30 nothing goes out.
    let confirm = format!("/admin/announcements/{id}/confirm");
    let v = version(&w, &id).await;
    b.post(&confirm, &format!("version={v}&reviewed=1")).await;
    assert_eq!(
        b.cookie("hs_flash").unwrap_or_default(),
        "err:announcements.review.changed"
    );
    assert_eq!(texts(&w, STATUS).await, ["draft"]);
    // Confirmed on the review step: scheduled again, to everyone.
    assert_eq!(confirm_reviewed(&b, &id).await.status, 303);
    assert_eq!(texts(&w, STATUS).await, ["scheduled"]);
    assert_eq!(after_send_at(&w).await, 4, "everyone, once confirmed");
}

#[tokio::test]
async fn a_crafted_edit_cannot_skip_the_review_of_a_new_audience() {
    let w = boot().await;
    roster(&w).await;
    let b = signed_in(&w, "Officer").await;
    let id = scheduled_group(&w, &b).await;
    // Everything a review would post, on the edit form instead.
    let crafted = "&scope=everyone&reviewed=1&reach=3&confirm=yes&do=confirm";
    let r = edit(&w, &b, &id, crafted).await;
    assert!(r.header("location").unwrap().ends_with("/review?held=1"));
    assert_eq!(texts(&w, STATUS).await, ["draft"]);
    assert_eq!(after_send_at(&w).await, 0, "held: nothing at send_at");
    // Any Audience change is held, not only to everyone.
    let w = boot().await;
    roster(&w).await;
    let b = signed_in(&w, "Officer").await;
    let id = scheduled_group(&w, &b).await;
    let r = edit(&w, &b, &id, "&scope=groups&attr.programme=MSC-Intl").await;
    assert!(r.header("location").unwrap().ends_with("/review?held=1"));
    assert_eq!(texts(&w, STATUS).await, ["draft"]);
}

#[tokio::test]
async fn a_scheduled_text_edit_keeps_its_schedule() {
    let w = boot().await;
    roster(&w).await;
    let b = signed_in(&w, "Officer").await;
    let id = scheduled_group(&w, &b).await;
    let r = edit(&w, &b, &id, "&scope=groups&attr.programme=MSC").await;
    assert_eq!(
        r.header("location"),
        Some(format!("/admin/announcements/{id}").as_str())
    );
    assert_eq!(
        b.cookie("hs_flash").unwrap_or_default(),
        "ok:announcements.flash.updated"
    );
    assert_eq!(texts(&w, STATUS).await, ["scheduled"]);
    assert_eq!(after_send_at(&w).await, 3, "the same group at send_at");
}

/// The review page of a draft to everyone on a Deployment with a `line`
/// Channel (s1, s3 linked), a 300 quota with 299 used, and `package`
/// saved (or none).
async fn line_review(package: Option<&str>) -> String {
    let src = format!("{CONFIG}\n[quotas.line]\nmetered_per_month = 300\n");
    let w = boot_line(&src, 500, |f| f.with_kind("line")).await;
    roster(&w).await;
    link(&w.h, "s1", "line", "active").await;
    link(&w.h, "s3", "line", "active").await;
    let db = w.h.db();
    db.execute(Statement::new(
        "INSERT INTO usage_daily (channel, day, metered) VALUES ('line', '2026-11-05', 299)",
    ))
    .await
    .unwrap();
    if let Some(p) = package {
        db.execute(
            Statement::new(
                "INSERT INTO setting (key, locale, value, updated_at) \
                 VALUES ('kernel.usage.line_package', '', ?, 1)",
            )
            .bind(p),
        )
        .await
        .unwrap();
    }
    let b = signed_in(&w, "Officer").await;
    let id = create(&b, &form("ข่าว", "ข้อความ", "&scope=everyone")).await;
    b.get(&format!("/admin/announcements/{id}/review"))
        .await
        .body
}

#[tokio::test]
async fn the_over_quota_line_says_what_the_line_package_does() {
    // Free: 300, no extra. 2 LINE accounts, 1 left: 1 over.
    let page = line_review(Some("free")).await;
    assert!(page.contains("จากโควตาเดือนนี้ที่เหลือ 1"), "{page}");
    assert!(
        page.contains("แพ็กเกจฟรีซื้อข้อความเพิ่มไม่ได้ LINE จะไม่ส่งส่วนที่เกิน"),
        "{page}"
    );
    assert!(page.contains("ระบบไม่ส่งทางอีเมลแทน"), "{page}");
    assert!(!page.contains("อาจมีค่าใช้จ่ายเพิ่ม"), "{page}");
    // No package saved: it depends on the package.
    let page = line_review(None).await;
    assert!(page.contains("ถ้าแพ็กเกจซื้อข้อความเพิ่มไม่ได้"), "{page}");
    assert!(!page.contains("อาจมีค่าใช้จ่ายเพิ่ม"), "{page}");
}

#[tokio::test]
async fn the_danger_label_names_the_reachable_students_too() {
    let w = boot().await;
    roster(&w).await;
    // s4's only Link (tg) goes unreachable: 3 counted, 2 reachable.
    run(
        &w.h,
        "UPDATE link SET status = 'unreachable' WHERE student_id = 's4'",
    )
    .await;
    let b = signed_in(&w, "Officer").await;
    let id = create(&b, &form("ข่าว", "ข้อความ", "&scope=everyone&unreachable=1")).await;
    let page = b
        .get(&format!("/admin/announcements/{id}/review"))
        .await
        .body;
    assert!(
        page.contains(&format!("{DANGER}ส่งถึงทุกคน 3 คน (ติดต่อได้ 2)</button>")),
        "{page}"
    );
    assert!(review_fields(&b, &id).await.contains("reach=3"));
}

#[tokio::test]
async fn a_choice_drops_the_filters_left_under_another_choice() {
    let w = boot().await;
    roster(&w).await;
    let b = signed_in(&w, "Officer").await;
    // Hand-picked s1 with a programme filter left filled in: the list alone.
    let c = b
        .post(
            "/admin/announcements/audience",
            "scope=students&students=code-s1&attr.programme=Fisheries",
        )
        .await
        .body;
    assert!(
        !c.contains("นักศึกษา 0"),
        "the Fisheries filter is not applied: {c}"
    );
    // Groups with a stale hand-picked list: the group alone (2 in MSC).
    let c = b
        .post(
            "/admin/announcements/audience",
            "scope=groups&attr.programme=MSC&students=99999999",
        )
        .await
        .body;
    assert!(c.contains("นักศึกษา 2"), "{c}");
    assert!(!c.contains("hand-picked-report"), "{c}");
}

#[tokio::test]
async fn a_legacy_combined_audience_keeps_both_filters_when_saved_without_a_choice() {
    let w = boot().await;
    roster(&w).await;
    let b = signed_in(&w, "Officer").await;
    // Saved before #120: programme MSC AND the list [s1] (no choice posted).
    let combined = "&attr.programme=MSC&students=code-s1";
    let id = create(&b, &form("ข่าว", "ข้อความ", combined)).await;
    let path = format!("/admin/announcements/{id}/edit");
    let page = b.get(&path).await.body;
    assert!(
        page.contains("ใช้ทั้งกลุ่มและรายคนพร้อมกัน"),
        "the warning: {page}"
    );
    assert!(
        !page.contains(r#"name="scope" value="groups" required"#),
        "{page}"
    );
    for c in ["everyone", "groups", "students"] {
        let picked = format!(r#"name="scope" value="{c}" checked"#);
        assert!(!page.contains(&picked), "no choice preselected: {c}");
    }
    // Saved again without choosing: both filters stay.
    let v = version(&w, &id).await;
    let again = form("ข่าว", "แก้ข้อความ", &format!("{combined}&version={v}"));
    assert_eq!(b.post(&path, &again).await.status, 303);
    let audience = texts(&w, "SELECT audience FROM announcement").await;
    assert!(
        audience[0].contains(r#""programme":["MSC"]"#),
        "{audience:?}"
    );
    assert!(
        audience[0].contains(r#""students":["code-s1"]"#),
        "{audience:?}"
    );
    // Choosing "เลือกกลุ่ม" drops the list, as the warning says.
    let v = version(&w, &id).await;
    let groups = form(
        "ข่าว",
        "แก้ข้อความ",
        &format!("{combined}&scope=groups&version={v}"),
    );
    assert_eq!(b.post(&path, &groups).await.status, 303);
    let audience = texts(&w, "SELECT audience FROM announcement").await;
    assert!(!audience[0].contains("code-s1"), "{audience:?}");
}

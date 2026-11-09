//! Broad sends confirmed in proportion to their reach (#120): "ทุกคน" is
//! an explicit Audience choice, "ยืนยันส่ง" opens a review step (count,
//! quota, timing, bubble), and a send to everyone confirms with
//! "ส่งถึงทุกคน N คน", N counted again on the server.

mod announcements_support;

use announcements_support::*;
use harpseal::Statement;

const DANGER: &str = r#"<button class="danger" type="submit">"#;
const ROWS: &str = "SELECT count(*) FROM announcement";
const SENT: &str = "SELECT count(*) FROM announcement WHERE status IN ('sending', 'sent')";

/// The harness clock's day (see `usage_announcements.rs`).
const TODAY: &str = "2026-11-05";

/// [`roster`] (3 active Students; `fake` reaches 2, metered) with a quota
/// of 300 on `fake`, 245 used this month.
async fn quota_world() -> World {
    let w = boot_config(
        &format!("{CONFIG}\n[quotas.fake]\nmetered_per_month = 300\n"),
        500,
    )
    .await;
    roster(&w).await;
    w.h.db()
        .execute(
            Statement::new(
                "INSERT INTO usage_daily (channel, day, metered) VALUES ('fake', ?, 245)",
            )
            .bind(TODAY),
        )
        .await
        .unwrap();
    w
}

fn cookie_flash(b: &harpseal_testing::Browser<'_>) -> String {
    b.cookie("hs_flash").unwrap_or_default()
}

#[tokio::test]
async fn an_empty_audience_no_longer_means_everyone() {
    let w = boot().await;
    roster(&w).await;
    let b = signed_in(&w, "Officer").await;
    let page = b.get("/admin/announcements/new").await.body;
    assert!(
        page.contains(r#"name="scope" value="everyone" required>"#),
        "none preselected: {page}"
    );
    // No choice, a group choice without a group, a list choice without ids.
    for (extra, says) in [
        ("&do=confirm", "ยังไม่ได้เลือกผู้รับ"),
        ("&scope=groups&tags=&do=confirm", "เลือกกลุ่มไว้แต่ยังไม่ได้ระบุกลุ่ม"),
        (
            "&scope=students&students=&do=confirm",
            "เลือกรายคนไว้แต่ยังไม่มีรหัสนักศึกษา",
        ),
        ("&scope=other", "ยังไม่ได้เลือกผู้รับ"),
    ] {
        let r = b
            .post("/admin/announcements/new", &form("ข่าว", "ข้อความ", extra))
            .await;
        assert_eq!(r.status, 422, "{extra}");
        assert!(r.body.contains(says), "{extra}: {}", r.body);
        assert!(r.body.contains("ข้อความ"), "the draft comes back");
    }
    assert_eq!(count(&w, ROWS).await, 0, "nothing saved, nothing sent");
    // The live count asks for the choice instead of counting everyone.
    let c = b.post("/admin/announcements/audience", "").await;
    assert!(c.body.contains("ยังไม่ได้เลือกผู้รับ"), "{}", c.body);
    let c = b
        .post("/admin/announcements/audience", "scope=everyone&tags=ปี1")
        .await;
    assert!(
        c.body.contains("นักศึกษา 3"),
        "everyone drops stale filters: {}",
        c.body
    );
}

#[tokio::test]
async fn confirm_opens_the_review_step_before_anything_is_sent() {
    let w = quota_world().await;
    let b = signed_in(&w, "Officer").await;
    let r = b
        .post(
            "/admin/announcements/new",
            &form("ย้ายห้องสอบ", "ห้อง 3105", "&scope=everyone&do=confirm"),
        )
        .await;
    assert_eq!(r.status, 303, "{}", r.body);
    let to = r.header("location").unwrap().to_string();
    assert!(to.ends_with("/review"), "{to}");
    let id = to.split('/').nth(3).unwrap().to_string();
    assert_eq!(
        texts(&w, "SELECT status FROM announcement").await,
        ["draft"]
    );
    drain(&w).await;
    assert!(w.line.sent().is_empty(), "nothing sent before the review");
    let page = b.get(&to).await.body;
    // Count and quota match the form's count box for the same Audience.
    let c = b
        .post("/admin/announcements/audience", "scope=everyone")
        .await
        .body;
    assert!(c.contains("นักศึกษา 3"), "{c}");
    assert!(c.contains("ข้อความที่คิดค่าบริการประมาณ 2"), "{c}");
    assert!(
        page.contains(r#"<dd data-count="students"><strong>3 คน</strong>"#),
        "{page}"
    );
    assert!(
        page.contains("จะใช้ข้อความ fake ประมาณ 2 ข้อความ จากโควตาเดือนนี้ที่เหลือ 55"),
        "{page}"
    );
    assert!(page.contains("ส่งทันทีเมื่อกดยืนยัน"), "{page}");
    assert!(
        page.contains("ย้ายห้องสอบ") && page.contains("bubble"),
        "the bubble"
    );
    // Everyone: the danger button names the reach.
    assert!(
        page.contains(&format!("{DANGER}ส่งถึงทุกคน 3 คน</button>")),
        "{page}"
    );
    assert!(
        page.contains(&format!(
            r#"href="/admin/announcements/{id}/edit">กลับไปแก้ไข"#
        )),
        "{page}"
    );
}

#[tokio::test]
async fn back_to_editing_keeps_the_draft_and_a_group_send_is_not_danger() {
    let w = boot().await;
    roster(&w).await;
    let b = signed_in(&w, "Officer").await;
    let r = b
        .post(
            "/admin/announcements/new",
            &form(
                "สอบ",
                "ห้อง 3105",
                "&scope=groups&attr.programme=MSC&do=confirm",
            ),
        )
        .await;
    let id = r
        .header("location")
        .unwrap()
        .split('/')
        .nth(3)
        .unwrap()
        .to_string();
    let page = b
        .get(&format!("/admin/announcements/{id}/review"))
        .await
        .body;
    assert!(page.contains("ตามกลุ่มที่เลือก"), "{page}");
    assert!(
        page.contains(">ยืนยันส่ง</button>") && !page.contains(DANGER),
        "{page}"
    );
    // "กลับไปแก้ไข": the draft and its Audience choice are kept.
    let edit = b.get(&format!("/admin/announcements/{id}/edit")).await.body;
    assert!(edit.contains("ห้อง 3105"), "{edit}");
    assert!(
        edit.contains(r#"value="groups" required checked"#),
        "{edit}"
    );
    assert_eq!(
        texts(&w, "SELECT status FROM announcement").await,
        ["draft"]
    );
    // Saved with "ยืนยันส่ง" again: back to the review step, still a draft.
    let v = version(&w, &id).await;
    let again = form(
        "สอบ",
        "ห้อง 3106",
        &format!("&scope=groups&attr.programme=MSC&version={v}&do=confirm"),
    );
    let r = b
        .post(&format!("/admin/announcements/{id}/edit"), &again)
        .await;
    assert_eq!(
        r.header("location"),
        Some(format!("/admin/announcements/{id}/review").as_str())
    );
    assert_eq!(
        texts(&w, "SELECT status FROM announcement").await,
        ["draft"]
    );
}

#[tokio::test]
async fn a_send_to_everyone_needs_the_reach_it_showed_and_sends_once() {
    let w = boot().await;
    roster(&w).await;
    let b = signed_in(&w, "Officer").await;
    let id = create(&b, &form("ข่าว", "ข้อความ", "&scope=everyone")).await;
    let confirm = format!("/admin/announcements/{id}/confirm");
    // Straight from the detail page or an old form: to the review step.
    let r = b.post(&confirm, "version=0").await;
    assert_eq!(
        r.header("location"),
        Some(format!("/admin/announcements/{id}/review").as_str())
    );
    // Reviewed but without the reach, or with a stale one: refused.
    for reach in ["", "&reach=2", "&reach=4", "&reach=x"] {
        let r = b
            .post(&confirm, &format!("version=0&reviewed=1{reach}"))
            .await;
        assert_eq!(r.status, 303, "{reach}");
        assert!(
            r.header("location").unwrap().ends_with("/review"),
            "{reach}"
        );
        assert_eq!(
            cookie_flash(&b),
            "err:announcements.review.changed",
            "{reach}"
        );
    }
    drain(&w).await;
    assert_eq!(count(&w, SENT).await, 0);
    assert!(w.line.sent().is_empty(), "nothing sent");
    let audited = "SELECT count(*) FROM audit_log WHERE action = 'announcement.send'";
    assert_eq!(count(&w, audited).await, 0);
    // The reach shown confirms; a second submit is a stale version (CAS).
    let fields = review_fields(&b, &id).await;
    assert!(fields.contains("reach=3"), "{fields}");
    assert_eq!(b.post(&confirm, &fields).await.status, 303);
    let second = b.post(&confirm, &fields).await;
    assert_eq!(second.status, 303);
    assert_eq!(cookie_flash(&b), "err:announcements.flash.conflict");
    drain(&w).await;
    assert_eq!(count(&w, SENT).await, 1);
    assert_eq!(count(&w, audited).await, 1, "sent once");
    assert_eq!(w.line.sent().len(), 2, "one Card per fake account");
}

#[tokio::test]
async fn a_resend_to_everyone_asks_with_the_reach() {
    let w = boot().await;
    roster(&w).await;
    let b = signed_in(&w, "Officer").await;
    let id = create(&b, &form("ข่าว", "ข้อความ", "&scope=everyone&do=confirm")).await;
    drain(&w).await;
    w.line.take_sent();
    let v = version(&w, &id).await;
    let path = format!("/admin/announcements/{id}/resend");
    let ask = b.post(&path, &format!("version={v}")).await.body;
    assert!(
        ask.contains(&format!("{DANGER}ส่งถึงทุกคน 3 คน</button>")),
        "{ask}"
    );
    let r = b.post(&path, &format!("version={v}&confirm=yes")).await;
    assert_eq!(r.status, 200, "asked again without the reach");
    drain(&w).await;
    assert!(w.line.sent().is_empty());
    assert_eq!(resend_confirmed(&b, &id, v).await.status, 303);
    drain(&w).await;
    assert_eq!(w.line.sent().len(), 2);
}

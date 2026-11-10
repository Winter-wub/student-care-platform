//! Bulk actions on the Students list (#145), continued: unlink (a review
//! naming the count, the number typed for more than one Student),
//! "ประกาศถึงที่เลือก" to the active Students selected, and the limits.

mod bulk_support;
mod students_support;
mod support;

use bulk_support::{BULK, boot, confirm_of, link_row, many};
use harpseal_testing::a11y;
use students_support::{flash, signed_in};
use support::Chat;

#[tokio::test]
async fn unlinking_many_needs_the_number_typed() {
    let (h, app) = boot().await;
    Chat::new(&h, &app, "u1").link("68010001").await;
    Chat::new(&h, &app, "u4").link("68010004").await;
    let (b, _) = signed_in(&h, &app, "Everything").await;
    let pick = "which=picked&student=s1&student=s3&student=s4&action=unlink";
    let review = b.post(BULK, pick).await;
    assert_eq!(review.status, 200);
    assert!(review.body.contains("ของนักศึกษา 2 คน"), "{}", review.body);
    assert!(review.body.contains("อีก 1 คนยังไม่ได้ผูกบัญชี"));
    assert!(review.body.contains("class=\"danger\""));
    assert!(review.body.contains("id=\"bulk-typed\""));
    assert!(a11y::problems(&review.body).is_empty());
    let ok = confirm_of(&review);

    for typed in ["", "ยืนยัน", "3"] {
        let r = b.post(BULK, &format!("{pick}&{ok}&typed={typed}")).await;
        assert_eq!(r.status, 422, "{typed}");
        assert!(r.body.contains("bulk-typed-error"));
        // An error summary and the title say so too.
        assert!(r.body.contains("role=\"alert\"") && r.body.contains("ตัวเลขไม่ตรง: ตรวจก่อนยืนยัน"));
        assert!(a11y::problems(&r.body).is_empty());
    }
    assert_eq!(
        app.count("SELECT count(*) FROM link WHERE status <> 'revoked'")
            .await,
        2
    );
    h.fake_channel().take_sent();
    let r = b.post(BULK, &format!("{pick}&{ok}&typed=%E0%B9%92")).await;
    assert_eq!(r.status, 303, "{}", r.body);
    assert_eq!(flash(&b), "ok:kernel.students.bulk_unlinked");
    assert_eq!(
        app.count("SELECT count(*) FROM link WHERE status <> 'revoked'")
            .await,
        0
    );
    assert_eq!(
        app.count("SELECT count(*) FROM audit_log WHERE action = 'student.unlink'")
            .await,
        2
    );
    assert_eq!(
        app.count("SELECT count(*) FROM audit_log WHERE action = 'link.revoke'")
            .await,
        2
    );
    assert_eq!(h.fake_channel().take_sent().len(), 2, "each Student told");

    // Nothing left to unlink: the confirm is blocked with the reason.
    let r = b.post(BULK, pick).await;
    assert!(r.body.contains("blocked-reason") && r.body.contains("disabled"));
}

#[tokio::test]
async fn a_changed_unlink_still_asks_for_the_number() {
    let (h, app) = boot().await;
    link_row(&h, "s1").await;
    link_row(&h, "s3").await;
    let (b, _) = signed_in(&h, &app, "Everything").await;
    let all = "which=filter&q=&action=unlink";
    let review = b.post(BULK, all).await;
    let stale = confirm_of(&review);
    link_row(&h, "s4").await;
    let r = b.post(BULK, &format!("{all}&{stale}&typed=2")).await;
    assert_eq!(r.status, 200, "{}", r.body);
    assert!(r.body.contains("รายชื่อเปลี่ยนไประหว่างนี้"));
    // The field is there (no error yet), so the confirm can carry it.
    assert!(r.body.contains("id=\"bulk-typed\"") && !r.body.contains("bulk-typed-error"));
    assert!(a11y::problems(&r.body).is_empty());
    let fresh = confirm_of(&r);
    let r = b.post(BULK, &format!("{all}&{fresh}&typed=3")).await;
    assert_eq!(r.status, 303, "{}", r.body);
    assert_eq!(
        app.count("SELECT count(*) FROM link WHERE status <> 'revoked'")
            .await,
        0
    );
}

#[tokio::test]
async fn announcing_prefills_the_editor_with_the_active_students() {
    let (h, app) = boot().await;
    let (b, _) = signed_in(&h, &app, "Sender").await;
    let page = b.get("/admin/students").await.body;
    assert!(page.contains("value=\"announce\"") && !page.contains("value=\"tag_add\""));
    // s2 has left: not in the Audience.
    let r = b
        .post(
            BULK,
            "which=picked&student=s3&student=s2&student=s1&action=announce",
        )
        .await;
    assert_eq!(r.status, 303);
    let to = r.header("location").unwrap().to_string();
    assert_eq!(
        to,
        "/admin/announcements/new?scope=students&students=68010001+68010003"
    );
    let editor = b.get(&to).await.body;
    assert!(editor.contains("68010001\n68010003"), "{editor}");
    let r = b
        .post(BULK, "which=picked&student=s2&action=announce")
        .await;
    assert_eq!(r.status, 303);
    assert_eq!(flash(&b), "err:kernel.students.bulk_none_active");

    // A whole filter travels as the filter and is read on the server.
    let editor = b
        .get("/admin/announcements/new?scope=students&from=students&q=Nok")
        .await
        .body;
    assert!(editor.contains(">68010001</textarea>"), "{editor}");
    let r = b.post(BULK, "which=filter&q=&action=announce").await;
    assert_eq!(r.status, 303);
    let to = r.header("location").unwrap();
    assert!(to.contains("students=68010001+68010003+68010004"), "{to}");

    // Without kernel.student.view the filter names nobody: a bare form.
    let (only, _) = signed_in(&h, &app, "SendOnly").await;
    let editor = only
        .get("/admin/announcements/new?scope=students&from=students&q=Nok")
        .await;
    assert_eq!(editor.status, 200);
    assert!(!editor.body.contains("68010001"), "{}", editor.body);
}

#[tokio::test]
async fn more_than_the_limits_are_refused_with_the_count() {
    let (h, app) = boot().await;
    many(&h, 2000).await;
    let (b, _) = signed_in(&h, &app, "Everything").await;
    let r = b.post(BULK, "which=filter&q=&action=announce").await;
    assert_eq!(r.status, 422);
    assert!(
        r.body.contains("เลือกไว้ 2,003 คน"),
        "active only: {}",
        r.body
    );
    assert!(r.body.contains("ไม่เกิน 2,000 คน"));
    assert!(a11y::problems(&r.body).is_empty());
    let r = b.post(BULK, "which=filter&q=&action=tag_add&tag=x").await;
    assert_eq!(r.status, 422);
    assert!(
        r.body
            .contains("เลือกไว้ 2,004 คน ทำพร้อมกันได้ครั้งละไม่เกิน 2,000 คน")
    );
    // The editor ignores a filter over the limit: a bare form.
    let editor = b
        .get("/admin/announcements/new?scope=students&from=students&q=")
        .await
        .body;
    assert!(!editor.contains("69020000"));
    // The HTMX count names the whole filter, and a page its rows.
    let r = b
        .post("/admin/students/bulk/count", "which=filter&q=")
        .await;
    assert!(r.body.contains("เลือกแล้ว 2,004 คน"), "{}", r.body);
    let r = b
        .post("/admin/students/bulk/count", "which=page&page=41&q=")
        .await;
    assert!(r.body.contains("เลือกแล้ว 4 คน"), "{}", r.body);

    // The unlink cap counts the Students who would change, not those
    // selected: 1,111 match "Krathing 1", none linked yet; 101 linked is
    // over.
    let narrow = "which=filter&q=Krathing+1&action=unlink";
    let r = b.post(BULK, narrow).await;
    assert_eq!(r.status, 200, "nobody linked: a blocked review");
    for i in 0..101 {
        link_row(&h, &format!("x1{i:03}")).await;
    }
    let r = b.post(BULK, narrow).await;
    assert_eq!(r.status, 422);
    assert!(r.body.contains("101 คนผูกบัญชีอยู่") && r.body.contains("ไม่เกิน 100 คน"));
}

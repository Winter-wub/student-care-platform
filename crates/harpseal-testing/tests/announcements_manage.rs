//! Announcements (#25) back office: edit, resend, the pages, Permissions,
//! CSRF, and the Audience resolver.

mod announcements_support;

use announcements_support::*;
use harpseal::Message;
use harpseal::audience::{self, Audience};

#[tokio::test]
async fn editing_a_sent_announcement_edits_on_telegram_like_only() {
    let w = boot().await;
    roster(&w).await;
    let b = signed_in(&w, "Officer").await;
    let id = create(&b, &form("เดิม", "ข้อความเดิม", "&do=confirm")).await;
    drain(&w).await;
    let edit = format!("/admin/announcements/{id}/edit");
    assert_eq!(b.get(&edit).await.status, 200);
    let v = version(&w, &id).await;
    let r = b
        .post(
            &edit,
            &format!("{}&version={v}", form("ใหม่", "ข้อความใหม่", "")),
        )
        .await;
    assert_eq!(r.status, 303, "{}", r.body);
    drain(&w).await;
    let edited = w.tg.edited();
    assert_eq!(edited.len(), 2, "s1 and s4 on the Telegram-like Channel");
    assert!(
        edited
            .iter()
            .all(|(_, m)| matches!(m, Message::Card(c) if c.title == "ใหม่"))
    );
    assert!(w.line.edited().is_empty(), "LINE-like cannot edit: nothing");
    assert_eq!(
        texts(&w, "SELECT edit_report ->> 'edited' FROM announcement").await,
        ["2"]
    );
    // A stale version is a conflict and changes nothing.
    let stale = b
        .post(&edit, &format!("{}&version={v}", form("อีก", "อีกครั้ง", "")))
        .await;
    assert_eq!(stale.status, 303);
    assert_eq!(
        texts(
            &w,
            "SELECT title FROM announcement_text WHERE locale = 'th'"
        )
        .await,
        ["ใหม่"]
    );
}

#[tokio::test]
async fn resend_is_a_new_round_with_a_new_job_key() {
    let w = boot().await;
    roster(&w).await;
    let b = signed_in(&w, "Officer").await;
    let id = create(&b, &form("ย้ำ", "อย่าลืม", "&do=confirm")).await;
    drain(&w).await;
    w.line.take_calls();
    let v = version(&w, &id).await;
    let r = resend_confirmed(&b, &id, v).await;
    assert_eq!(r.status, 303);
    drain(&w).await;
    assert_eq!(
        texts(
            &w,
            "SELECT round || ':' || count(*) FROM announcement_delivery GROUP BY round ORDER BY round"
        )
        .await,
        ["0:4", "1:4"]
    );
    let keys = texts(
        &w,
        "SELECT unique_key FROM job WHERE kind = 'announcements.deliver' ORDER BY created_at",
    )
    .await;
    assert_eq!(keys.len(), 2);
    assert_ne!(keys[0], keys[1]);
    assert_eq!(w.line.calls().len(), 1, "pushed again: one more multicast");
    assert_eq!(w.line.sent_to(&fake_account("fake", "s3")).len(), 2);
    assert_eq!(texts(&w, "SELECT status FROM announcement").await, ["sent"]);
}

#[tokio::test]
async fn a_viewer_sees_but_cannot_send_edit_or_retract() {
    let w = boot().await;
    roster(&w).await;
    let officer = signed_in(&w, "Officer").await;
    let id = create(&officer, &form("ข่าว", "รายละเอียด", "&do=confirm")).await;
    drain(&w).await;
    let viewer = signed_in(&w, "Viewer").await;
    let list = viewer.get("/admin/announcements").await;
    assert_eq!(list.status, 200);
    assert!(list.body.contains("ข่าว"));
    let detail = viewer.get(&format!("/admin/announcements/{id}")).await;
    assert_eq!(detail.status, 200);
    assert!(detail.body.contains("0 / 3"), "acked / in Audience");
    assert!(!detail.body.contains("/retract"), "no retract button");
    assert_eq!(viewer.get("/admin/announcements/new").await.status, 403);
    for path in ["new", &format!("{id}/retract"), &format!("{id}/resend")] {
        let r = viewer
            .post(
                &format!("/admin/announcements/{path}"),
                "version=1&confirm=yes",
            )
            .await;
        assert_eq!(r.status, 403, "{path}");
    }
    assert_eq!(texts(&w, "SELECT status FROM announcement").await, ["sent"]);
}

#[tokio::test]
async fn posts_without_the_csrf_token_are_refused() {
    let w = boot().await;
    roster(&w).await;
    let b = signed_in(&w, "Officer").await;
    let r = b
        .post_raw(
            "/admin/announcements/new",
            &form("ข่าว", "รายละเอียด", "&do=confirm"),
        )
        .await;
    assert_eq!(r.status, 403);
    assert_eq!(count(&w, "SELECT count(*) FROM announcement").await, 0);
}

#[tokio::test]
async fn the_form_refuses_a_missing_thai_text_and_counts_the_audience_live() {
    let w = boot().await;
    roster(&w).await;
    let b = signed_in(&w, "Officer").await;
    let r = b
        .post("/admin/announcements/new", "title_en=News&body_en=Text")
        .await;
    assert_eq!(r.status, 422);
    assert!(r.body.contains("ต้องมีหัวข้อและรายละเอียดภาษาไทย"));
    assert_eq!(count(&w, "SELECT count(*) FROM announcement").await, 0);
    // The HTMX count: 3 Students; LINE-like metered 2, Telegram-like free.
    let c = b
        .post(
            "/admin/announcements/audience",
            "attr.programme=MSC&attr.programme=MSC-Intl",
        )
        .await;
    assert_eq!(c.status, 200);
    assert!(c.body.contains("นักศึกษา 3"), "{}", c.body);
    assert!(c.body.contains("จะใช้ push ประมาณ 2 ครั้ง"), "{}", c.body);
    // A draft is listed and confirmed from its page.
    let id = create(&b, &form("ร่าง", "ยังไม่ส่ง", "&do=draft")).await;
    assert_eq!(
        texts(&w, "SELECT status FROM announcement").await,
        ["draft"]
    );
    drain(&w).await;
    assert!(w.line.sent().is_empty());
    let r = confirm_reviewed(&b, &id).await;
    assert_eq!(r.status, 303);
    drain(&w).await;
    assert_eq!(texts(&w, "SELECT status FROM announcement").await, ["sent"]);
    // The detail page's delivery filter answers HTMX with the fragment.
    let req = http::Request::get(format!("/admin/announcements/{id}?status=sent"))
        .header("hx-request", "true")
        .body(axum::body::Body::empty())
        .unwrap();
    let frag = b.send(req).await;
    assert_eq!(frag.status, 200);
    assert!(frag.body.starts_with("<div id=\"deliveries\""));
    assert_eq!(frag.header("vary"), Some("HX-Request"));
}

#[tokio::test]
async fn the_audience_ands_attributes_tags_and_link_status() {
    let w = boot().await;
    let h = &w.h;
    student(h, "a1", "th", "active", r#"{"programme":"MSC","year":2}"#).await;
    student(
        h,
        "a2",
        "th",
        "active",
        r#"{"programme":"MSC-Intl","year":2}"#,
    )
    .await;
    student(
        h,
        "a3",
        "th",
        "active",
        r#"{"programme":"Fisheries","year":2}"#,
    )
    .await;
    student(h, "a4", "th", "active", r#"{"programme":"MSC","year":3}"#).await;
    student(h, "a5", "th", "left", r#"{"programme":"MSC","year":2}"#).await;
    student(h, "a6", "th", "active", r#"{"programme":"MSC","year":2}"#).await;
    for s in ["a1", "a2", "a3", "a4", "a5", "a6"] {
        tag(h, s, "clinic").await;
        link(
            h,
            s,
            "fake",
            if s == "a6" { "unreachable" } else { "active" },
        )
        .await;
    }
    run(h, "DELETE FROM student_tag WHERE student_id = 'a2'").await;
    link(h, "a1", TG, "revoked").await;
    let d = w.app.deployment();
    let mut a = Audience::everyone();
    a.attributes
        .insert("programme".into(), vec!["MSC".into(), "MSC-Intl".into()]);
    a.attributes.insert("year".into(), vec!["2".into()]);
    a.tags = vec!["clinic".into()];
    let ids = |r: Vec<audience::Recipient>| -> Vec<String> {
        r.into_iter().map(|r| r.student_id).collect()
    };
    // a2 lacks the Tag, a3 the programme, a4 the year, a5 left, a6 unreachable.
    assert_eq!(ids(audience::resolve(d, &a).await.unwrap()), ["a1"]);
    a.include_unreachable = true;
    let r = audience::resolve(d, &a).await.unwrap();
    assert_eq!(ids(r.clone()), ["a1", "a6"]);
    assert!(r[1].unreachable);
    let counts = audience::Counts::of(&r);
    assert_eq!(counts.students, 2);
    assert_eq!(counts.per_channel["fake"], (1, 1));
    // An unknown attribute key is refused, never spliced into SQL.
    a.attributes
        .insert("x') OR 1=1 --".into(), vec!["1".into()]);
    assert!(audience::resolve(d, &a).await.is_err());
}

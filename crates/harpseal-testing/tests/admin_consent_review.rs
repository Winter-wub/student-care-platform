//! The review step of a privacy notice version change (#135): it counts
//! from the Links and Consents exactly the Students the chat flow will ask
//! again, saves nothing, and the version field lists and checks its
//! allowed characters.

mod admin_support;
mod support;

use admin_support::{boot, flash, run, signed_in};

const DANGER: &str = r#"<button class="danger" type="submit">"#;
const UNSAVED: &str = "SELECT count(*) FROM setting WHERE key = 'kernel.consent.version'";

/// A Channel Account `a-{id}` in `state` with a Link of Student `id`.
async fn linked(h: &harpseal_testing::Harness, id: &str, status: &str, state: &str) {
    run(
        h,
        &format!(
            "INSERT INTO channel_account (id, channel, external_id, first_seen_at, flow_state, \
             updated_at) VALUES ('a-{id}', 'fake', 'u-{id}', 1, '{state}', 1)"
        ),
    )
    .await;
    run(
        h,
        &format!(
            "INSERT INTO link (id, student_id, channel_account_id, channel, status, method, \
             linked_at, updated_at) VALUES ('l-{id}', '{id}', 'a-{id}', 'fake', '{status}', \
             'email_otp', 1, 1)"
        ),
    )
    .await;
}

async fn accepted(h: &harpseal_testing::Harness, id: &str, version: &str) {
    run(
        h,
        &format!(
            "INSERT INTO consent (id, student_id, channel_account_id, notice_version, locale, \
             accepted_at) VALUES ('c-{id}-{version}', '{id}', 'a-{id}', '{version}', 'th', 1)"
        ),
    )
    .await;
}

#[tokio::test]
async fn the_review_counts_the_students_the_chat_flow_will_ask_again() {
    let (h, app) = boot().await;
    for (id, code) in [("s5", "68010005"), ("s6", "68010006"), ("s7", "68010007")] {
        run(
            &h,
            &format!(
                "INSERT INTO student (id, student_code, display_name, locale, status, \
                 created_at, updated_at) VALUES ('{id}', '{code}', 'Student {id}', 'th', \
                 'active', 1, 1)"
            ),
        )
        .await;
    }
    // Asked again: accepted 1 (active, and unreachable), or parked on the Card.
    linked(&h, "s1", "active", "linked").await;
    accepted(&h, "s1", "1").await;
    linked(&h, "s3", "unreachable", "linked").await;
    accepted(&h, "s3", "1").await;
    linked(&h, "s6", "active", "awaiting_reconsent").await;
    // Not asked: already accepted "2"; a seeded Link with no Consent; a
    // revoked Link.
    linked(&h, "s4", "active", "linked").await;
    accepted(&h, "s4", "1").await;
    accepted(&h, "s4", "2").await;
    linked(&h, "s5", "active", "linked").await;
    linked(&h, "s7", "revoked", "linked").await;
    accepted(&h, "s7", "1").await;

    let (admin, _) = signed_in(&h, &app, "Administrator").await;
    admin.get("/admin/texts").await;
    let review = admin
        .post("/admin/texts/consent", "version=2&expected=-")
        .await;
    assert_eq!(review.status, 200, "{}", review.body);
    let b = &review.body;
    assert_eq!(b.matches("<h1").count(), 1, "{b}");
    assert!(b.contains("ตรวจก่อนเปลี่ยนฉบับประกาศความเป็นส่วนตัว"), "{b}");
    assert!(b.contains(r#"<strong class="code">1</strong>"#), "old: {b}");
    assert!(b.contains(r#"<strong class="code">2</strong>"#), "new: {b}");
    assert!(b.contains("นักศึกษา 3 คน"), "{b}");
    assert!(
        b.contains(&format!("{DANGER}เปลี่ยนฉบับ: ขอความยินยอมใหม่ 3 คน</button>")),
        "{b}"
    );
    assert!(b.contains(r#"name="reviewed" value="1""#), "{b}");
    assert!(b.contains(r#"name="reach" value="3""#), "{b}");
    assert!(b.contains("กลับไปแก้ไข"), "{b}");
    assert_eq!(app.count(UNSAVED).await, 0, "the review saves nothing");

    // Confirmed without the review's number: the review again, unsaved.
    for form in [
        "version=2&expected=-&reviewed=1",
        "version=2&expected=-&reviewed=1&reach=2",
        "version=2&expected=-&reach=3",
    ] {
        let r = admin.post("/admin/texts/consent", form).await;
        assert_eq!(r.status, 200, "{form}");
        assert!(r.body.contains("นักศึกษา 3 คน"), "{form}");
        assert_eq!(app.count(UNSAVED).await, 0, "{form}");
    }
    let r = admin
        .post(
            "/admin/texts/consent",
            "version=2&expected=-&reviewed=1&reach=3",
        )
        .await;
    assert_eq!(r.status, 303, "{}", r.body);
    assert_eq!(flash(&admin), "ok:kernel.admin.texts.consent_saved");
    assert_eq!(app.count(UNSAVED).await, 1);
}

#[tokio::test]
async fn with_nobody_to_ask_the_confirm_is_the_plain_one() {
    let (h, app) = boot().await;
    let (admin, _) = signed_in(&h, &app, "Administrator").await;
    admin.get("/admin/texts").await;
    let review = admin
        .post("/admin/texts/consent", "version=2026-11&expected=-")
        .await;
    assert_eq!(review.status, 200);
    assert!(review.body.contains("ไม่มีนักศึกษาที่ต้องยอมรับใหม่ในตอนนี้"));
    assert!(!review.body.contains(DANGER), "{}", review.body);
    assert!(
        review
            .body
            .contains(r#"<button type="submit">ยืนยันเปลี่ยนฉบับ</button>"#)
    );
    let r = admin
        .post(
            "/admin/texts/consent",
            "version=2026-11&expected=-&reviewed=1&reach=0",
        )
        .await;
    assert_eq!(r.status, 303);
    assert_eq!(flash(&admin), "ok:kernel.admin.texts.consent_saved");
}

#[tokio::test]
async fn the_version_field_lists_and_checks_its_allowed_characters() {
    let (h, app) = boot().await;
    let (admin, _) = signed_in(&h, &app, "Administrator").await;
    let page = admin.get("/admin/texts").await.body;
    assert!(
        page.contains("ใช้ได้เฉพาะตัวอักษรภาษาอังกฤษ ตัวเลข จุด (.) ขีดล่าง (_) และขีดกลาง (-) ไม่เกิน 32 ตัว"),
        "{page}"
    );
    assert!(
        page.contains(r#"aria-describedby="consent-version-rules consent-version-error""#),
        "{page}"
    );
    assert!(page.contains(r#"id="consent-version-error" aria-live="polite""#));
    assert!(page.contains("data-bad=\"ใช้ตัวนี้ไม่ได้: {chars}\""), "{page}");
    assert!(
        page.contains("getElementById('consent-version')"),
        "the check"
    );
    // The button opens the review; it no longer says it saves.
    assert!(page.contains(">ตรวจก่อนเปลี่ยนฉบับ</button>"), "{page}");
    // The server checks the characters too.
    let r = admin
        .post(
            "/admin/texts/consent",
            "version=%E0%B8%89%E0%B8%9A&expected=-",
        )
        .await;
    assert_eq!(r.status, 303);
    assert_eq!(flash(&admin), "err:kernel.admin.texts.consent_invalid");
}

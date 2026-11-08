//! Serving announcement images (#109): the public route answers only
//! while the announcement is sent and live, with the stored type,
//! `nosniff`, an inline safe name and a short cache; everything else is
//! the same 404. Staff see any status through the back office.

mod announcements_support;
mod images_support;

use announcements_support::*;
use images_support::*;

async fn get(w: &World, path: &str) -> harpseal_testing::TestResponse {
    w.app.get(path).await
}

#[tokio::test]
async fn a_draft_is_404_publicly_and_200_to_staff() {
    let w = boot_public().await;
    let b = signed_in(&w, "Officer").await;
    let id = create_parts(&b, "x", vec![f("image1", "a.png", png(20, 13))]).await;
    let tok = token(&w, &id, "1").await;
    assert_eq!(
        get(&w, &format!("/announcements/files/{tok}")).await.status,
        404
    );
    let staff = b
        .get(&format!("/admin/announcements/{id}/files/{tok}"))
        .await;
    assert_eq!(staff.status, 200);
    assert_eq!(staff.bytes, png(20, 13));
    assert_eq!(staff.header("cache-control"), Some("no-store"));
    // Not for a signed-out visitor, nor under another announcement's id.
    let anon = get(&w, &format!("/admin/announcements/{id}/files/{tok}")).await;
    assert_ne!(anon.status, 200);
    let other = create_parts(&b, "y", vec![]).await;
    let r = b
        .get(&format!("/admin/announcements/{other}/files/{tok}"))
        .await;
    assert_eq!(r.status, 404);
}

#[tokio::test]
async fn a_sent_one_is_served_with_safe_headers() {
    let w = boot_public().await;
    roster(&w).await;
    let b = signed_in(&w, "Officer").await;
    let id = create_parts(
        &b,
        "x",
        vec![
            f("image1", "a.png", png(20, 13)),
            f("image2", "b.jpg", jpeg(20, 13)),
        ],
    )
    .await;
    send_now(&w, &b, &id).await;
    for (n, ty, ext, bytes) in [
        ("1", "image/png", "png", png(20, 13)),
        ("2", "image/jpeg", "jpg", jpeg(20, 13)),
    ] {
        let r = get(
            &w,
            &format!("/announcements/files/{}", token(&w, &id, n).await),
        )
        .await;
        assert_eq!(r.status, 200);
        assert_eq!(r.bytes, bytes);
        assert_eq!(r.header("content-type"), Some(ty));
        assert_eq!(r.header("x-content-type-options"), Some("nosniff"));
        let disposition = format!("inline; filename=\"image-{n}.{ext}\"");
        assert_eq!(r.header("content-disposition"), Some(disposition.as_str()));
        assert_eq!(r.header("cache-control"), Some("public, max-age=300"));
        assert_eq!(
            r.header("content-security-policy"),
            Some("default-src 'none'; sandbox")
        );
        assert!(r.header("set-cookie").is_none());
    }
}

#[tokio::test]
async fn unknown_and_malformed_tokens_are_the_same_404() {
    let w = boot_public().await;
    for path in [
        "/announcements/files/0123456789abcdef0123456789abcdef",
        "/announcements/files/0123456789ABCDEF0123456789ABCDEF",
        "/announcements/files/..%2F..%2Fetc%2Fpasswd",
        "/announcements/files/x",
        "/announcements/files/'%20OR%201=1%20--",
    ] {
        let r = get(&w, path).await;
        assert_eq!(r.status, 404, "{path}");
        assert_eq!(r.header("cache-control"), Some("no-store"), "{path}");
    }
}

#[tokio::test]
async fn retracted_is_public_404_and_staff_200() {
    let w = boot_public().await;
    roster(&w).await;
    let b = signed_in(&w, "Officer").await;
    let id = create_parts(&b, "x", vec![f("image1", "a.png", png(20, 13))]).await;
    send_now(&w, &b, &id).await;
    let tok = token(&w, &id, "1").await;
    let public = format!("/announcements/files/{tok}");
    assert_eq!(get(&w, &public).await.status, 200);
    b.get(&format!("/admin/announcements/{id}")).await;
    let v = version(&w, &id).await;
    let r = b
        .post(
            &format!("/admin/announcements/{id}/retract"),
            &format!("version={v}&confirm=yes"),
        )
        .await;
    assert_eq!(r.status, 303, "{}", r.body);
    assert_eq!(get(&w, &public).await.status, 404);
    // Kept, as the text is: Staff still see it.
    let staff = b
        .get(&format!("/admin/announcements/{id}/files/{tok}"))
        .await;
    assert_eq!(staff.status, 200);
    assert_eq!(w.h.files().keys().len(), 1);
}

#[tokio::test]
async fn expired_is_404() {
    let w = boot_public().await;
    roster(&w).await;
    let b = signed_in(&w, "Officer").await;
    let id = create_parts(
        &b,
        "x",
        vec![
            f("image1", "a.png", png(20, 13)),
            t("expires", "2026-12-31"),
        ],
    )
    .await;
    send_now(&w, &b, &id).await;
    let public = format!("/announcements/files/{}", token(&w, &id, "1").await);
    assert_eq!(get(&w, &public).await.status, 200);
    // Two years on: past the expiry.
    w.h.clock().advance_minutes(2 * 366 * 24 * 60);
    assert_eq!(get(&w, &public).await.status, 404);
}

#[tokio::test]
async fn removed_from_a_draft_is_404_and_its_object_deleted() {
    let w = boot_public().await;
    let b = signed_in(&w, "Officer").await;
    let id = create_parts(&b, "x", vec![f("image1", "a.png", png(20, 13))]).await;
    let tok = token(&w, &id, "1").await;
    let r = edit_parts(&w, &b, &id, "x", vec![t("image1_remove", "1")]).await;
    assert_eq!(r.status, 303, "{}", r.body);
    let staff = b
        .get(&format!("/admin/announcements/{id}/files/{tok}"))
        .await;
    assert_eq!(staff.status, 404);
    assert!(w.h.files().keys().is_empty());
    assert_eq!(count(&w, "SELECT count(*) FROM announcement_file").await, 0);
}

#[tokio::test]
async fn replaced_on_a_sent_one_stays_served_until_retracted() {
    let w = boot_public().await;
    roster(&w).await;
    let b = signed_in(&w, "Officer").await;
    let id = create_parts(&b, "x", vec![f("image1", "a.png", png(20, 13))]).await;
    send_now(&w, &b, &id).await;
    let old = token(&w, &id, "1").await;
    // The form says a removed image stays reachable until the retract.
    let edit = b.get(&format!("/admin/announcements/{id}/edit")).await.body;
    assert!(edit.contains("ยังเปิดได้จากลิงก์เดิม"), "{edit}");
    let r = edit_parts(&w, &b, &id, "x", vec![f("image1", "b.png", png(40, 26))]).await;
    assert_eq!(r.status, 303, "{}", r.body);
    drain(&w).await;
    let new = token(&w, &id, "1").await;
    assert_ne!(old, new);
    // Cards already delivered still point at the old one.
    let (old_path, new_path) = (
        format!("/announcements/files/{old}"),
        format!("/announcements/files/{new}"),
    );
    assert_eq!(get(&w, &old_path).await.bytes, png(20, 13));
    assert_eq!(get(&w, &new_path).await.bytes, png(40, 26));
    let superseded = count(
        &w,
        "SELECT count(*) FROM announcement_file WHERE superseded_at IS NOT NULL",
    )
    .await;
    assert_eq!(superseded, 1);
    // The Mini App shows the current one only.
    assert_eq!(stored(&w, &id).await.len(), 1);

    b.get(&format!("/admin/announcements/{id}")).await;
    let v = version(&w, &id).await;
    let r = b
        .post(
            &format!("/admin/announcements/{id}/retract"),
            &format!("version={v}&confirm=yes"),
        )
        .await;
    assert_eq!(r.status, 303, "{}", r.body);
    assert_eq!(get(&w, &old_path).await.status, 404);
    assert_eq!(get(&w, &new_path).await.status, 404);
    // Kept (no purge yet): both objects are still in the FileStore.
    assert_eq!(w.h.files().keys().len(), 2);
}

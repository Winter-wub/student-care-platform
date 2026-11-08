//! Serving announcement PDFs (#113) under #109's rules: public only while
//! sent and live, `application/pdf`, the uploaded name in
//! `Content-Disposition` (`filename*`, Thai kept), `nosniff` and the
//! sandbox CSP; a PDF replaced after sending stays served until the
//! retract. Backup and restore: `backup_pdfs.rs`.

mod announcements_support;
mod images_support;

use announcements_support::*;
use images_support::*;

fn pdf(tag: &str) -> Vec<u8> {
    format!("%PDF-1.4\n% {tag}\n1 0 obj << /Type /Catalog >> endobj\n%%EOF\n").into_bytes()
}

async fn pdf_token(w: &World, id: &str, n: u8) -> String {
    texts(
        w,
        &format!(
            "SELECT token FROM announcement_file WHERE announcement_id = '{id}' \
             AND kind = 'pdf' AND n = {n} AND superseded_at IS NULL"
        ),
    )
    .await
    .pop()
    .unwrap_or_else(|| panic!("no PDF {n}"))
}

async fn retract(w: &World, b: &harpseal_testing::Browser<'_>, id: &str) {
    b.get(&format!("/admin/announcements/{id}")).await;
    let v = version(w, id).await;
    let r = b
        .post(
            &format!("/admin/announcements/{id}/retract"),
            &format!("version={v}&confirm=yes"),
        )
        .await;
    assert_eq!(r.status, 303, "{}", r.body);
}

const THAI: &str = "inline; filename=\"document-1.pdf\"; \
     filename*=UTF-8''%E0%B8%95%E0%B8%B2%E0%B8%A3%E0%B8%B2%E0%B8%87%E0%B8%AA%E0%B8%AD%E0%B8%9A.pdf";

#[tokio::test]
async fn draft_404_sent_200_with_headers_retracted_404() {
    let w = boot_public().await;
    roster(&w).await;
    let b = signed_in(&w, "Officer").await;
    let id = create_parts(&b, "x", vec![f("pdf1", "ตารางสอบ.pdf", pdf("a"))]).await;
    let tok = pdf_token(&w, &id, 1).await;
    assert_eq!(tok.len(), 32);
    let public = format!("/announcements/files/{tok}");
    let staff = format!("/admin/announcements/{id}/files/{tok}");

    // Draft: public 404, Staff 200.
    let r = w.app.get(&public).await;
    assert_eq!(r.status, 404);
    assert_eq!(r.header("cache-control"), Some("no-store"));
    let r = b.get(&staff).await;
    assert_eq!(r.status, 200);
    assert_eq!(r.bytes, pdf("a"));
    assert_eq!(r.header("content-disposition"), Some(THAI));
    // The detail page lists it for download.
    let detail = b.get(&format!("/admin/announcements/{id}")).await.body;
    assert!(detail.contains(&format!("href=\"{staff}\"")), "{detail}");
    assert!(detail.contains("download"));

    // Sent: public 200, every header.
    send_now(&w, &b, &id).await;
    let r = w.app.get(&public).await;
    assert_eq!(r.status, 200);
    assert_eq!(r.bytes, pdf("a"));
    assert_eq!(r.header("content-type"), Some("application/pdf"));
    assert_eq!(r.header("content-disposition"), Some(THAI));
    assert_eq!(r.header("x-content-type-options"), Some("nosniff"));
    assert_eq!(
        r.header("content-security-policy"),
        Some("default-src 'none'; sandbox")
    );
    assert_eq!(r.header("cache-control"), Some("public, max-age=300"));
    assert!(r.header("set-cookie").is_none());

    // Retracted: public 404, Staff still 200, the object kept.
    retract(&w, &b, &id).await;
    assert_eq!(w.app.get(&public).await.status, 404);
    assert_eq!(b.get(&staff).await.status, 200);
    assert_eq!(w.h.files().keys().len(), 1);
}

#[tokio::test]
async fn an_ascii_name_is_its_own_fallback() {
    let w = boot_public().await;
    roster(&w).await;
    let b = signed_in(&w, "Officer").await;
    let id = create_parts(&b, "x", vec![f("pdf2", "exam rules.pdf", pdf("b"))]).await;
    send_now(&w, &b, &id).await;
    let tok = pdf_token(&w, &id, 2).await;
    let r = w.app.get(&format!("/announcements/files/{tok}")).await;
    assert_eq!(
        r.header("content-disposition"),
        Some("inline; filename=\"exam rules.pdf\"; filename*=UTF-8''exam%20rules.pdf")
    );
}

#[tokio::test]
async fn expired_is_404() {
    let w = boot_public().await;
    roster(&w).await;
    let b = signed_in(&w, "Officer").await;
    let id = create_parts(
        &b,
        "x",
        vec![f("pdf1", "a.pdf", pdf("a")), t("expires", "2026-12-31")],
    )
    .await;
    send_now(&w, &b, &id).await;
    let public = format!("/announcements/files/{}", pdf_token(&w, &id, 1).await);
    assert_eq!(w.app.get(&public).await.status, 200);
    w.h.clock().advance_minutes(2 * 366 * 24 * 60);
    assert_eq!(w.app.get(&public).await.status, 404);
}

#[tokio::test]
async fn replaced_after_sending_stays_served_until_retracted() {
    let w = boot_public().await;
    roster(&w).await;
    let b = signed_in(&w, "Officer").await;
    let id = create_parts(&b, "x", vec![f("pdf1", "a.pdf", pdf("old"))]).await;
    send_now(&w, &b, &id).await;
    let old = pdf_token(&w, &id, 1).await;
    let edit = b.get(&format!("/admin/announcements/{id}/edit")).await.body;
    assert!(edit.contains("ยังเปิดได้จากปุ่มเดิม"), "{edit}");
    let r = edit_parts(&w, &b, &id, "x", vec![f("pdf1", "b.pdf", pdf("new"))]).await;
    assert_eq!(r.status, 303, "{}", r.body);
    drain(&w).await;
    let new = pdf_token(&w, &id, 1).await;
    assert_ne!(old, new);
    let (old_path, new_path) = (
        format!("/announcements/files/{old}"),
        format!("/announcements/files/{new}"),
    );
    assert_eq!(w.app.get(&old_path).await.bytes, pdf("old"));
    assert_eq!(w.app.get(&new_path).await.bytes, pdf("new"));
    retract(&w, &b, &id).await;
    assert_eq!(w.app.get(&old_path).await.status, 404);
    assert_eq!(w.app.get(&new_path).await.status, 404);
}

#[tokio::test]
async fn removed_from_a_draft_is_deleted() {
    let w = boot_public().await;
    let b = signed_in(&w, "Officer").await;
    let id = create_parts(&b, "x", vec![f("pdf3", "a.pdf", pdf("a"))]).await;
    let r = edit_parts(&w, &b, &id, "x", vec![t("pdf3_remove", "1")]).await;
    assert_eq!(r.status, 303, "{}", r.body);
    assert!(w.h.files().keys().is_empty());
    assert_eq!(count(&w, "SELECT count(*) FROM announcement_file").await, 0);
}

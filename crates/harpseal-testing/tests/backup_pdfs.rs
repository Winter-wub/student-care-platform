//! Backup and restore carry announcement PDFs (#113) as they carry images
//! (#109): through the `file_key` column of `announcement_file`.

mod backup_support;

use axum::body::Body;
use backup_support::{OFFICER, PASSWORD, deployment, populated, run_jobs, text};
use harpseal::FileStore;
use harpseal::backup::{self, LoadOptions, MemoryBackup};
use harpseal_testing::{Browser, Harness};
use http::{Request, header};

/// A small PDF (fictional bytes).
fn pdf() -> Vec<u8> {
    b"%PDF-1.4\n1 0 obj << /Type /Catalog >> endobj\n%%EOF\n".to_vec()
}

const THAI: &str = "inline; filename=\"document-1.pdf\"; \
     filename*=UTF-8''%E0%B8%95%E0%B8%B2%E0%B8%A3%E0%B8%B2%E0%B8%87%E0%B8%AA%E0%B8%AD%E0%B8%9A.pdf";

/// Creates and sends an announcement with one PDF through the form.
async fn with_pdf(b: &Browser<'_>) {
    b.get("/admin/announcements/new").await;
    let boundary = "----sealbay-backup-113";
    let mut body = Vec::new();
    let token = b.csrf().unwrap();
    for (head, value) in [
        ("name=\"_csrf\"".to_string(), token.into_bytes()),
        ("name=\"title_th\"".into(), "ตารางสอบ".as_bytes().to_vec()),
        ("name=\"body_th\"".into(), "ดูไฟล์แนบ".as_bytes().to_vec()),
        ("name=\"do\"".into(), b"confirm".to_vec()),
        (
            "name=\"pdf1\"; filename=\"ตารางสอบ.pdf\"\r\nContent-Type: application/pdf".into(),
            pdf(),
        ),
    ] {
        body.extend_from_slice(
            format!("--{boundary}\r\nContent-Disposition: form-data; {head}\r\n\r\n").as_bytes(),
        );
        body.extend_from_slice(&value);
        body.extend_from_slice(b"\r\n");
    }
    body.extend_from_slice(format!("--{boundary}--\r\n").as_bytes());
    let req = Request::post("/admin/announcements/new")
        .header(
            header::CONTENT_TYPE,
            format!("multipart/form-data; boundary={boundary}"),
        )
        .body(Body::from(body))
        .unwrap();
    let r = b.send(req).await;
    assert_eq!(r.status, 303, "{}", r.body);
}

#[tokio::test]
async fn a_backup_restores_pdfs_and_their_objects() {
    let (h, app) = populated().await;
    let b = app.browser();
    assert_eq!(b.login(OFFICER, PASSWORD).await.status, 303);
    with_pdf(&b).await;
    run_jobs(&h, &app).await;
    let key = text(
        &h,
        "SELECT file_key FROM announcement_file WHERE kind = 'pdf'",
    )
    .await;
    let token = text(&h, "SELECT token FROM announcement_file WHERE kind = 'pdf'").await;
    assert!(
        key.starts_with("announcement-file/") && key.ends_with(".pdf"),
        "{key}"
    );

    let mut bk = MemoryBackup::new();
    let dumped = backup::dump(app.deployment(), &mut bk).await.unwrap();
    assert!(dumped.missing_files.is_empty());
    assert!(dumped.manifest.files.iter().any(|f| f.key == key));

    let fresh = Harness::new();
    let d = deployment(&fresh);
    backup::load(&d, &bk, &LoadOptions::new()).await.unwrap();
    assert_eq!(
        fresh.files().get(&key).await.unwrap().as_deref(),
        Some(pdf().as_slice())
    );
    assert_eq!(
        text(&fresh, "SELECT file_name FROM announcement_file").await,
        "ตารางสอบ.pdf"
    );
    // Served again, typed from the row (a restored object is stored as
    // application/octet-stream), its Thai name kept.
    fresh.clock().advance_minutes(24 * 60);
    let app = fresh.boot(d).await;
    let r = app.get(&format!("/announcements/files/{token}")).await;
    assert_eq!(r.status, 200);
    assert_eq!(r.header("content-type"), Some("application/pdf"));
    assert_eq!(r.header("content-disposition"), Some(THAI));
    assert_eq!(r.bytes, pdf());
}

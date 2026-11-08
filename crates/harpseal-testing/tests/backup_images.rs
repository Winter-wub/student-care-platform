//! Backup and restore carry announcement images (#109) through the
//! `file_key` column of `announcement_file`; a backup taken before
//! Module migration `0003_files` still restores.

mod backup_support;

use axum::body::Body;
use backup_support::{OFFICER, PASSWORD, deployment, populated, run_jobs, text};
use harpseal::backup::{self, LoadOptions, MemoryBackup};
use harpseal::{FileStore, Statement};
use harpseal_testing::{Browser, Harness};
use http::{Request, header};

/// A PNG header (fictional bytes).
fn png() -> Vec<u8> {
    let mut b = b"\x89PNG\r\n\x1a\n\0\0\0\x0dIHDR".to_vec();
    b.extend_from_slice(&20u32.to_be_bytes());
    b.extend_from_slice(&13u32.to_be_bytes());
    b.extend_from_slice(&[8, 2, 0, 0, 0, 0, 0, 0, 0]);
    b
}

/// Creates and sends an announcement with one image through the form.
async fn with_image(b: &Browser<'_>) {
    b.get("/admin/announcements/new").await;
    let boundary = "----sealbay-backup-109";
    let mut body = Vec::new();
    let token = b.csrf().unwrap();
    for (head, value) in [
        ("name=\"_csrf\"".to_string(), token.into_bytes()),
        ("name=\"title_th\"".into(), "แผนที่สอบ".as_bytes().to_vec()),
        (
            "name=\"body_th\"".into(),
            "![แผนที่](image:1)".as_bytes().to_vec(),
        ),
        ("name=\"do\"".into(), b"confirm".to_vec()),
        (
            "name=\"image1\"; filename=\"map.png\"\r\nContent-Type: image/png".into(),
            png(),
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
async fn a_backup_restores_images_and_their_objects() {
    let (h, app) = populated().await;
    let b = app.browser();
    assert_eq!(b.login(OFFICER, PASSWORD).await.status, 303);
    with_image(&b).await;
    run_jobs(&h, &app).await;
    let key = text(&h, "SELECT file_key FROM announcement_file").await;
    let token = text(&h, "SELECT token FROM announcement_file").await;
    assert!(key.starts_with("announcement-file/"), "{key}");

    let mut bk = MemoryBackup::new();
    let dumped = backup::dump(app.deployment(), &mut bk).await.unwrap();
    assert!(dumped.missing_files.is_empty());
    assert!(dumped.manifest.files.iter().any(|f| f.key == key));

    let fresh = Harness::new();
    let d = deployment(&fresh);
    backup::load(&d, &bk, &LoadOptions::new()).await.unwrap();
    assert_eq!(
        fresh.files().get(&key).await.unwrap().as_deref(),
        Some(png().as_slice())
    );
    assert_eq!(
        text(&fresh, "SELECT token FROM announcement_file").await,
        token
    );
    // Served again, with the type the row keeps (a restored object is
    // stored as application/octet-stream).
    fresh.clock().advance_minutes(24 * 60);
    let app = fresh.boot(d).await;
    let r = app.get(&format!("/announcements/files/{token}")).await;
    assert_eq!(r.status, 200);
    assert_eq!(r.header("content-type"), Some("image/png"));
    assert_eq!(r.bytes, png());
}

#[tokio::test]
async fn a_backup_from_before_0003_files_restores() {
    // A Deployment still at Announcements migration 0002.
    let old = Harness::new();
    let d_old = deployment(&old);
    d_old.migrate().await.unwrap();
    for sql in [
        "DROP TABLE announcement_file",
        "DELETE FROM migration WHERE module = 'announcements' AND name = '0003_files'",
    ] {
        old.db().execute(Statement::new(sql)).await.unwrap();
    }
    let mut bk = MemoryBackup::new();
    backup::dump(&d_old, &mut bk).await.unwrap();
    assert!(!bk.entries.contains_key("tables/announcement_file.ndjson"));

    // Into a fresh database: the backup's migrations, then ours on top.
    let fresh = Harness::new();
    let d = deployment(&fresh);
    backup::load(&d, &bk, &LoadOptions::new()).await.unwrap();
    d.migrate().await.unwrap();
    let applied = text(
        &fresh,
        "SELECT name FROM migration WHERE module = 'announcements' ORDER BY name DESC",
    )
    .await;
    assert_eq!(applied, "0003_files");
    let n = fresh
        .db()
        .query(Statement::new("SELECT count(*) FROM announcement_file"))
        .await
        .unwrap();
    assert_eq!(n.rows[0][0].as_i64(), Some(0));

    // Into a database already at 0003 (forced): the table is left as is.
    let ahead = Harness::new();
    let d = deployment(&ahead);
    d.migrate().await.unwrap();
    backup::load(&d, &bk, &LoadOptions::new().force(true))
        .await
        .unwrap();
}

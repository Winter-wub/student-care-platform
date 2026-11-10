//! Announcement files from code (#164): `create_draft` attaches images and
//! PDFs with the create form's checks (size, type, count, pixels, the PDF
//! scan), refuses a content type that disagrees with the bytes, and audits
//! slots only.

use harpseal::modules::announcements::{self, AnnouncementFile, Announcements, NewAnnouncement};
use harpseal::{Actor, DeploymentConfig, Error, RequestCtx};
use harpseal_testing::{Harness, TestApp};

const CONFIG: &str = r#"
name = "Seal Bay University (files from code)"
default_locale = "th"
utc_offset = "+07:00"

[roles]
Officer = ["announcements.view", "announcements.send"]
Viewer = ["announcements.view"]
"#;

async fn boot() -> (Harness, TestApp, Actor, Actor) {
    let h = Harness::new();
    let d = h
        .builder()
        .config(DeploymentConfig::from_toml(CONFIG).unwrap())
        .module(Announcements)
        .build()
        .unwrap();
    let app = h.boot(d).await;
    let pw = "kelp forest at dawn";
    let officer = h
        .create_staff("officer@sealbay.example", pw, &["Officer"])
        .await;
    let viewer = h
        .create_staff("viewer@sealbay.example", pw, &["Viewer"])
        .await;
    (h, app, Actor::Staff(officer), Actor::Staff(viewer))
}

/// A PNG header of `w` x `h` (fictional bytes).
fn png(w: u32, h: u32) -> Vec<u8> {
    let mut b = b"\x89PNG\r\n\x1a\n".to_vec();
    b.extend_from_slice(&13u32.to_be_bytes());
    b.extend_from_slice(b"IHDR");
    b.extend_from_slice(&w.to_be_bytes());
    b.extend_from_slice(&h.to_be_bytes());
    b.extend_from_slice(&[8, 2, 0, 0, 0, 0, 0, 0, 0]);
    b
}

/// A small PDF (fictional bytes).
fn pdf(extra: &str) -> Vec<u8> {
    format!("%PDF-1.4\n1 0 obj << /Type /Catalog >> endobj\n{extra}\ntrailer << >>\n%%EOF\n")
        .into_bytes()
}

fn with(files: Vec<AnnouncementFile>) -> NewAnnouncement {
    let mut new = NewAnnouncement::new("ปฐมนิเทศ", "![แผนที่หอประชุม](image:1) รายละเอียดอยู่ในไฟล์แนบ");
    new.files = files;
    new
}

fn keys(r: harpseal::Result<String>) -> String {
    match r {
        Err(Error::Invalid(m)) => m,
        other => panic!("expected a refusal, got {other:?}"),
    }
}

#[tokio::test]
async fn an_image_and_a_pdf_are_stored_and_audited_by_slot() {
    let (_h, app, officer, viewer) = boot().await;
    let d = app.deployment();
    let ctx = RequestCtx::cli();
    let files = vec![
        AnnouncementFile::image(1, "map.png", "image/png", png(800, 600)),
        AnnouncementFile::pdf(1, "กำหนดการ.pdf", "application/pdf", pdf("")),
    ];
    let refused = announcements::create_draft(d, &with(files.clone()), &viewer, &ctx).await;
    assert!(
        matches!(refused, Err(Error::Forbidden { .. })),
        "{refused:?}"
    );
    assert_eq!(app.count("SELECT count(*) FROM announcement_file").await, 0);

    announcements::create_draft(d, &with(files), &officer, &ctx)
        .await
        .unwrap();
    let rows = app
        .query(
            "SELECT kind, n, content_type, width, height, file_name FROM announcement_file \
             ORDER BY kind",
        )
        .await;
    let text = |i: usize, c: &str| rows.get(i, c).unwrap().as_text().unwrap().to_string();
    assert_eq!(rows.len(), 2);
    assert_eq!(
        (
            text(0, "kind"),
            text(0, "content_type"),
            text(0, "file_name")
        ),
        ("image".into(), "image/png".into(), "image-1.png".into())
    );
    assert_eq!(rows.get(0, "width").unwrap().as_i64(), Some(800));
    assert_eq!(
        (text(1, "kind"), text(1, "file_name")),
        ("pdf".into(), "กำหนดการ.pdf".into())
    );
    let audit = app
        .query("SELECT after FROM audit_log WHERE action = 'announcement.create'")
        .await;
    let after = audit
        .get(0, "after")
        .unwrap()
        .as_text()
        .unwrap()
        .to_string();
    let json: serde_json::Value = serde_json::from_str(&after).unwrap();
    assert_eq!(json["images"]["put"], serde_json::json!([1]), "{after}");
    assert_eq!(
        json["images"]["pdfs"]["put"],
        serde_json::json!([1]),
        "{after}"
    );
    assert!(
        !after.contains("กำหนดการ"),
        "names are never audited: {after}"
    );
}

#[tokio::test]
async fn code_may_fill_every_pdf_slot_at_once() {
    let (_h, app, officer, _) = boot().await;
    let d = app.deployment();
    let files = (1..=3)
        .map(|n| AnnouncementFile::pdf(n, format!("ไฟล์{n}.pdf"), "application/pdf", pdf("")))
        .collect();
    announcements::create_draft(d, &with(files), &officer, &RequestCtx::cli())
        .await
        .unwrap();
    assert_eq!(
        app.count("SELECT count(*) FROM announcement_file WHERE kind = 'pdf'")
            .await,
        3
    );
}

#[tokio::test]
async fn the_forms_checks_refuse_and_nothing_is_written() {
    let (_h, app, officer, _) = boot().await;
    let d = app.deployment();
    let ctx = RequestCtx::cli();
    let png_ok = || png(100, 100);
    let cases: Vec<(Vec<AnnouncementFile>, &str)> = vec![
        (
            vec![AnnouncementFile::image(1, "a.png", "image/jpeg", png_ok())],
            "announcements.images.bad_type",
        ),
        (
            vec![AnnouncementFile::image(1, "a.jpg", "image/png", png_ok())],
            "announcements.images.bad_type",
        ),
        (
            vec![AnnouncementFile::image(
                1,
                "a.svg",
                "image/svg+xml",
                b"<svg/>".to_vec(),
            )],
            "announcements.images.bad_type",
        ),
        (
            vec![AnnouncementFile::image(5, "a.png", "image/png", png_ok())],
            "announcements.images.too_many",
        ),
        (
            vec![
                AnnouncementFile::image(2, "a.png", "image/png", png_ok()),
                AnnouncementFile::image(2, "b.png", "image/png", png_ok()),
            ],
            "announcements.images.too_many",
        ),
        (
            vec![AnnouncementFile::image(1, "a.png", "image/png", {
                let mut b = png_ok();
                b.resize(1024 * 1024 + 1, 0);
                b
            })],
            "announcements.images.too_big",
        ),
        (
            vec![AnnouncementFile::image(
                1,
                "a.png",
                "image/png",
                png(5000, 10),
            )],
            "announcements.images.too_many_pixels",
        ),
        (
            vec![AnnouncementFile::pdf(
                1,
                "a.pdf",
                "application/pdf",
                b"<html>".to_vec(),
            )],
            "announcements.pdfs.bad_type",
        ),
        (
            vec![AnnouncementFile::pdf(1, "a.pdf", "text/html", pdf(""))],
            "announcements.pdfs.bad_type",
        ),
        (
            vec![AnnouncementFile::pdf(
                1,
                "a.pdf",
                "application/pdf",
                pdf("2 0 obj << /S /JavaScript /JS (app.alert(1)) >> endobj"),
            )],
            "announcements.pdfs.active",
        ),
        (
            vec![AnnouncementFile::pdf(
                4,
                "a.pdf",
                "application/pdf",
                pdf(""),
            )],
            "announcements.pdfs.too_many",
        ),
    ];
    for (files, key) in cases {
        let m = keys(announcements::create_draft(d, &with(files), &officer, &ctx).await);
        assert!(m.contains(key), "{key}: {m}");
    }
    // The text checks and the file checks are reported together.
    let mut both = with(vec![AnnouncementFile::image(
        1,
        "a.png",
        "image/gif",
        png_ok(),
    )]);
    both.title_th.clear();
    let m = keys(announcements::create_draft(d, &both, &officer, &ctx).await);
    assert!(
        m.contains("announcements.form.th_required") && m.contains("images.bad_type"),
        "{m}"
    );
    assert_eq!(app.count("SELECT count(*) FROM announcement").await, 0);
    assert_eq!(app.count("SELECT count(*) FROM announcement_file").await, 0);
}

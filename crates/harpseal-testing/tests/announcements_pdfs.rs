//! Announcement PDFs (#113): the upload (sniff, size, count, one per
//! save, the Thai hint and errors), the Mini App list with names and
//! sizes, and the chat Card's URL button for the first PDF within the
//! 2-button budget.

mod announcements_support;
mod app_support;
mod images_support;

use announcements_support::*;
use app_support::*;
use harpseal::{Button, Card};
use images_support::*;

const HINT: &str = "แนบได้ไม่เกิน 3 ไฟล์ ไฟล์ละไม่เกิน 10 MB (PDF เท่านั้น) \
                    ห้ามแนบไฟล์ที่มีข้อมูลส่วนตัว";
const BAD_TYPE: &str = "รับเฉพาะไฟล์ PDF ที่ชื่อลงท้ายด้วย .pdf";
const ONCE: &str = "แนบได้ครั้งละ 1 ไฟล์ บันทึกแล้วแนบไฟล์ถัดไปได้";
const FULL: &str = "แนบครบ 3 ไฟล์แล้ว";

/// A small PDF (fictional bytes): header, one object, `%%EOF`.
fn pdf(extra: &str) -> Vec<u8> {
    format!("%PDF-1.4\n1 0 obj << /Type /Catalog >> endobj\n{extra}\ntrailer << >>\n%%EOF\n")
        .into_bytes()
}

/// The token of PDF `n` of `id` (current).
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

fn url_buttons(c: &Card) -> Vec<(String, String)> {
    c.buttons
        .iter()
        .filter_map(|b| match b {
            Button::Url { label, url, .. } => Some((label.clone(), url.clone())),
            _ => None,
        })
        .collect()
}

#[tokio::test]
async fn the_form_shows_the_thai_hint_and_one_file_input() {
    let w = boot_public().await;
    let b = signed_in(&w, "Officer").await;
    let page = b.get("/admin/announcements/new").await.body;
    assert!(page.contains(HINT), "{page}");
    assert!(page.contains(ONCE));
    // One file per save: the input of the first empty slot only.
    assert!(page.contains("name=\"pdf1\""));
    for n in 2..=4 {
        assert!(!page.contains(&format!("name=\"pdf{n}\"")), "pdf{n}");
    }
    assert!(page.contains("accept=\"application/pdf,.pdf\""));
    // HTMX fragments never post the files.
    assert!(page.contains("hx-params=\"not image1,image2,image3,image4,pdf1,pdf2,pdf3\""));
    // The toolbar: the H button and the five colours.
    assert!(page.contains("data-md=\"heading\""));
    for c in ["red", "orange", "green", "blue", "grey"] {
        assert!(page.contains(&format!("data-md=\"colour-{c}\"")), "{c}");
    }
}

#[tokio::test]
async fn refused_pdfs_name_the_limit_in_thai_and_nothing_is_stored() {
    let w = boot_public().await;
    let b = signed_in(&w, "Officer").await;
    let html = b"<!doctype html><html><script>alert(1)</script></html>".to_vec();
    let mut big = pdf("");
    big.resize(10 * 1024 * 1024 + 1, b' ');
    for (file, why) in [
        // A fake PDF: the magic bytes but no end.
        (
            f("pdf1", "fake.pdf", b"%PDF-1.4\nnot really".to_vec()),
            BAD_TYPE,
        ),
        // An HTML page renamed .pdf.
        (f("pdf1", "notes.pdf", html), BAD_TYPE),
        // A real PDF, misnamed.
        (f("pdf2", "notes.txt", pdf("")), BAD_TYPE),
        (f("pdf2", "notes.pdf.html", pdf("")), BAD_TYPE),
        // Script inside.
        (
            f(
                "pdf1",
                "form.pdf",
                pdf("<< /OpenAction << /S /JavaScript /JS (app.alert(1)) >> >>"),
            ),
            "มีสคริปต์ ไฟล์ฝัง หรือคำสั่งเปิดโปรแกรม",
        ),
        (
            f(
                "pdf1",
                "pack.pdf",
                pdf("<< /Names << /EmbeddedFiles 2 0 R >> >>"),
            ),
            "มีสคริปต์ ไฟล์ฝัง หรือคำสั่งเปิดโปรแกรม",
        ),
        // An unreferenced `(` before the script: it would have hidden it.
        (
            f(
                "pdf1",
                "open.pdf",
                pdf("0 0 obj (\n2 0 obj << /S /JavaScript /JS (app.alert(1)) >> endobj"),
            ),
            "ตรวจไฟล์ PDF นี้ไม่ได้",
        ),
        (f("pdf1", "big.pdf", big), "ไฟล์ใหญ่เกิน 10 MB"),
        (f("pdf4", "fourth.pdf", pdf("")), "แนบ PDF ได้ไม่เกิน 3 ไฟล์"),
    ] {
        let parts = vec![t("title_th", "ตารางสอบ"), t("body_th", "ดูไฟล์แนบ"), file];
        let r = post_parts(&b, "/admin/announcements/new", &parts).await;
        assert_eq!(r.status, 422, "{why}");
        assert!(r.body.contains(why), "{why}: {}", r.body);
        assert!(r.body.contains("เลือกไฟล์อีกครั้ง"), "files are lost: say so");
        assert!(r.body.contains("ดูไฟล์แนบ"), "the text comes back");
    }
    assert_eq!(count(&w, "SELECT count(*) FROM announcement").await, 0);
    assert!(w.h.files().keys().is_empty());
}

#[tokio::test]
async fn two_pdfs_in_one_save_are_refused_in_thai() {
    let w = boot_public().await;
    let b = signed_in(&w, "Officer").await;
    let parts = vec![
        t("title_th", "ตารางสอบ"),
        t("body_th", "ดูไฟล์แนบ"),
        f("pdf1", "a.pdf", pdf("")),
        f("pdf2", "b.pdf", pdf("")),
    ];
    let r = post_parts(&b, "/admin/announcements/new", &parts).await;
    assert_eq!(r.status, 422);
    assert!(r.body.contains(ONCE), "{}", r.body);
    assert_eq!(count(&w, "SELECT count(*) FROM announcement").await, 0);
    assert!(w.h.files().keys().is_empty());
}

#[tokio::test]
async fn a_body_over_the_one_pdf_cap_is_refused_before_it_is_read() {
    let w = boot_public().await;
    let b = signed_in(&w, "Officer").await;
    // The cap is 15,204,352 bytes: 4 images, one PDF, 512 KiB.
    let mut a = pdf("");
    a.resize(8 * 1024 * 1024, b' ');
    let parts = vec![
        t("title_th", "x"),
        f("pdf1", "a.pdf", a.clone()),
        f("pdf2", "b.pdf", a),
    ];
    let r = post_parts(&b, "/admin/announcements/new", &parts).await;
    assert_eq!(r.status, 413);
    assert_eq!(count(&w, "SELECT count(*) FROM announcement").await, 0);
    // Under it: 4 images of 1 MiB and a PDF of 10 MiB are taken.
    let mut big = pdf("");
    big.resize(10 * 1024 * 1024, b' ');
    let mut parts = vec![
        t("title_th", "x"),
        t("body_th", "x"),
        t("scope", "everyone"),
        f("pdf1", "a.pdf", big),
    ];
    for n in 1..=4 {
        let mut img = png(10, 10);
        img.resize(1024 * 1024, 0);
        parts.push(f(&format!("image{n}"), "a.png", img));
    }
    let r = post_parts(&b, "/admin/announcements/new", &parts).await;
    assert_eq!(r.status, 303, "{}", r.body);
}

#[tokio::test]
async fn three_pdfs_are_kept_with_their_names_one_save_each() {
    let w = boot_public().await;
    let b = signed_in(&w, "Officer").await;
    let id = create_parts(
        &b,
        "ดูไฟล์แนบ",
        vec![
            f("pdf1", "ตารางสอบ ภาค 1.pdf", pdf("")),
            f("image1", "a.png", png(20, 13)),
        ],
    )
    .await;
    // The edit form offers the next empty slot only.
    let edit = b.get(&format!("/admin/announcements/{id}/edit")).await.body;
    assert!(edit.contains("name=\"pdf2\"") && !edit.contains("name=\"pdf3\""));
    for (field, name, tag) in [
        ("pdf2", "C:\\fakepath\\map.PDF", "% two"),
        ("pdf3", "../rules.pdf", "% three"),
    ] {
        let r = edit_parts(&w, &b, &id, "ดูไฟล์แนบ", vec![f(field, name, pdf(tag))]).await;
        assert_eq!(r.status, 303, "{}", r.body);
    }
    // All three: no file input, and the form says how to add another.
    let edit = b.get(&format!("/admin/announcements/{id}/edit")).await.body;
    assert!(edit.contains(FULL), "{edit}");
    assert!(!edit.contains("type=\"file\" name=\"pdf"));
    let rows = texts(
        &w,
        &format!(
            "SELECT kind || '|' || n || '|' || file_name || '|' || content_type \
             FROM announcement_file WHERE announcement_id = '{id}' ORDER BY kind, n"
        ),
    )
    .await;
    assert_eq!(
        rows,
        vec![
            "image|1|image-1.png|image/png",
            "pdf|1|ตารางสอบ ภาค 1.pdf|application/pdf",
            "pdf|2|map.PDF|application/pdf",
            "pdf|3|rules.pdf|application/pdf",
        ]
    );
    assert_eq!(w.h.files().keys().len(), 4);
    // Image slot 1 and PDF slot 1 are distinct: removing the PDF keeps
    // the image.
    let r = edit_parts(&w, &b, &id, "ดูไฟล์แนบ", vec![t("pdf1_remove", "1")]).await;
    assert_eq!(r.status, 303, "{}", r.body);
    let left = count(
        &w,
        &format!("SELECT count(*) FROM announcement_file WHERE announcement_id = '{id}'"),
    )
    .await;
    assert_eq!(left, 3);
    assert_eq!(w.h.files().keys().len(), 3);
    // The edit page lists the PDFs left, the staff link a download.
    let edit = b.get(&format!("/admin/announcements/{id}/edit")).await.body;
    assert!(edit.contains("map.PDF") && !edit.contains("ตารางสอบ ภาค 1.pdf"));
}

#[tokio::test]
async fn the_mini_app_lists_pdfs_and_the_card_gets_one_button_within_the_budget() {
    let w = boot_public().await;
    roster(&w).await;
    let b = signed_in(&w, "Officer").await;
    let a = "https://reg.seal-bay.invalid/exam";
    let c = "https://map.seal-bay.invalid/3105";
    let body = format!("## สอบปลายภาค\nดู [ตาราง]({a}) และ [แผนที่]({c})");
    let mut big = pdf("");
    big.resize(1_258_291, b' ');
    let id = create_parts(&b, &body, vec![f("pdf1", "ตารางสอบ.pdf", big)]).await;
    let r = edit_parts(&w, &b, &id, &body, vec![f("pdf2", "rules.pdf", pdf(""))]).await;
    assert_eq!(r.status, 303, "{}", r.body);
    send_now(&w, &b, &id).await;
    let tok = pdf_token(&w, &id, 1).await;
    let file_url = format!("{PUBLIC}/announcements/files/{tok}");
    for card in [
        card_of(w.line.sent_to(&fake_account("fake", "s1"))),
        card_of(w.tg.sent_to(&fake_account(TG, "s1"))),
    ] {
        // 2 URL buttons at most: the first link, then the first PDF.
        assert_eq!(
            url_buttons(&card),
            vec![
                ("ตาราง".to_string(), a.to_string()),
                ("ตารางสอบ.pdf".to_string(), file_url.clone()),
            ]
        );
        assert_eq!(card.body, "สอบปลายภาค\nดู ตาราง และ แผนที่");
    }

    let app = open(&w, "fake", "s1").await;
    let page = app.get(&format!("/app/announcements/{id}")).await.body;
    let item = format!(
        "<li><a href=\"/announcements/files/{tok}\" target=\"_blank\" rel=\"noopener noreferrer\" \
         type=\"application/pdf\" data-ext>📄 ตารางสอบ.pdf</a> <span class=\"size\">(1.2 MB)</span></li>"
    );
    assert!(page.contains(&item), "{page}");
    assert!(page.contains("📄 rules.pdf</a> <span class=\"size\">(1 KB)</span>"));
    assert!(page.contains("<h2 class=\"app-h\">สอบปลายภาค</h2>"));
    // The list on /app expands to the same.
    assert!(app.get("/app").await.body.contains(&item));
}

#[tokio::test]
async fn a_pdf_alone_gets_the_button_and_the_outside_script() {
    let w = boot_public().await;
    roster(&w).await;
    let b = signed_in(&w, "Officer").await;
    let id = create_parts(&b, "ห้อง 3105", vec![f("pdf1", "a.pdf", pdf(""))]).await;
    send_now(&w, &b, &id).await;
    let card = card_of(w.line.sent_to(&fake_account("fake", "s1")));
    let tok = pdf_token(&w, &id, 1).await;
    assert_eq!(
        url_buttons(&card),
        vec![(
            "a.pdf".to_string(),
            format!("{PUBLIC}/announcements/files/{tok}")
        )]
    );
    // Marked for the phone's browser (LINE adds openExternalBrowser=1).
    assert!(card.buttons.iter().any(Button::is_external));
    // No link in the body, but a PDF: the page loads the script that
    // opens it outside the chat app.
    let app = open(&w, "fake", "s1").await;
    let page = app.get(&format!("/app/announcements/{id}")).await.body;
    assert!(page.contains("a[data-ext]"), "{page}");
}

#[tokio::test]
async fn without_public_url_the_card_has_no_pdf_button() {
    let w = boot().await;
    roster(&w).await;
    let b = signed_in(&w, "Officer").await;
    let page = b.get("/admin/announcements/new").await.body;
    assert!(page.contains("การ์ดในแชทจะยังไม่มีปุ่มเปิดไฟล์"), "{page}");
    let a = "https://reg.seal-bay.invalid/exam";
    let c = "https://map.seal-bay.invalid/3105";
    let body = format!("[ตาราง]({a}) [แผนที่]({c})");
    let id = create_parts(&b, &body, vec![f("pdf1", "a.pdf", pdf(""))]).await;
    send_now(&w, &b, &id).await;
    let card = card_of(w.line.sent_to(&fake_account("fake", "s1")));
    // Both links keep their buttons.
    let urls: Vec<String> = url_buttons(&card).into_iter().map(|(_, u)| u).collect();
    assert_eq!(urls, vec![a.to_string(), c.to_string()]);
    // The Mini App still lists it.
    let app = open(&w, "fake", "s1").await;
    let page = app.get(&format!("/app/announcements/{id}")).await.body;
    assert!(page.contains("📄 a.pdf"));
}

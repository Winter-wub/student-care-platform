//! The shorter editor (#140): title, body, Audience and the send choice
//! come first; the optional parts (English, images, PDFs, expiry and the
//! acknowledgement) sit behind disclosures that open on load when they
//! hold a value or an error. Field names are unchanged.

mod announcements_support;
mod images_support;

use announcements_support::*;
use images_support::*;

/// The `<details …>` opening tag of the optional part `id`, without `>`.
fn part<'p>(page: &'p str, id: &str) -> &'p str {
    let at = page
        .find(&format!("id=\"{id}\""))
        .unwrap_or_else(|| panic!("no {id}: {page}"));
    let start = page[..at].rfind("<details").expect("a details");
    &page[start..at + page[at..].find('>').unwrap()]
}

/// Whether the part carries the whole `open` attribute (not a value or
/// another name that merely contains it).
fn is_open(page: &str, id: &str) -> bool {
    part(page, id).split_whitespace().any(|a| a == "open")
}

/// Which of the images and PDFs parts are open: `(images, pdfs)`.
fn files_open(page: &str) -> (bool, bool) {
    (is_open(page, "more-images"), is_open(page, "more-pdfs"))
}

/// A small PDF (fictional bytes).
fn pdf() -> Vec<u8> {
    b"%PDF-1.4\n1 0 obj << /Type /Catalog >> endobj\ntrailer << >>\n%%EOF\n".to_vec()
}

/// The new form posted with `files`, the Thai body `body` (empty: refused).
async fn post_new(b: &harpseal_testing::Browser<'_>, body: &str, files: Vec<Part>) -> String {
    let mut parts = vec![
        t("title_th", "ย้ายห้องสอบ"),
        t("body_th", body),
        t("scope", "everyone"),
    ];
    parts.extend(files);
    let r = post_parts(b, "/admin/announcements/new", &parts).await;
    assert_eq!(r.status, 422, "{}", r.body);
    r.body
}

const PARTS: [&str; 4] = ["more-english", "more-images", "more-pdfs", "more-options"];

#[tokio::test]
async fn a_new_form_keeps_the_essentials_first_and_folds_the_rest() {
    let w = boot().await;
    let b = signed_in(&w, "Officer").await;
    let page = b.get("/admin/announcements/new").await.body;
    for id in PARTS {
        assert!(!is_open(&page, id), "{id} open on a blank form");
    }
    // Title, body, Audience and when come before the first optional part.
    let first = page.find("class=\"optional\"").unwrap();
    for needle in [
        "name=\"title_th\"",
        "name=\"body_th\"",
        "id=\"audience\"",
        "name=\"when\"",
    ] {
        let at = page.find(needle).unwrap_or_else(|| panic!("{needle}"));
        assert!(at < first, "{needle} after the optional parts");
    }
    // Folded, the fields are still in the form, under their names.
    for name in [
        "title_en",
        "body_en",
        "image1",
        "pdf1",
        "expires",
        "require_ack",
    ] {
        assert!(page.contains(&format!("name=\"{name}\"")), "{name}");
    }
    assert_eq!(
        harpseal_testing::a11y::problems(&page),
        Vec::<String>::new()
    );
}

#[tokio::test]
async fn a_part_with_an_error_or_a_value_is_open_on_load() {
    let w = boot().await;
    let b = signed_in(&w, "Officer").await;
    // An English title without its body is refused; the expiry was set.
    let f = with_scope(&form(
        "สอบกลางภาค",
        "ตารางสอบอยู่ที่ห้อง 3105",
        "&title_en=Midterm&expires=2026-12-31",
    ));
    let r = b.post("/admin/announcements/new", &f).await;
    assert_eq!(r.status, 422, "{}", r.body);
    let page = &r.body;
    assert!(
        is_open(page, "more-english"),
        "{}",
        part(page, "more-english")
    );
    assert!(page.contains("id=\"err-en\""), "{page}");
    assert!(is_open(page, "more-options"));
    assert!(!is_open(page, "more-images"));
    assert!(!is_open(page, "more-pdfs"));
    assert_eq!(harpseal_testing::a11y::problems(page), Vec::<String>::new());
}

#[tokio::test]
async fn a_saved_acknowledgement_opens_its_part_when_editing() {
    let w = boot().await;
    roster(&w).await;
    let b = signed_in(&w, "Officer").await;
    let id = create(&b, &form("ส่งเอกสาร", "ส่งภายในวันศุกร์", "&require_ack=1")).await;
    let page = b.get(&format!("/admin/announcements/{id}/edit")).await.body;
    assert!(
        is_open(&page, "more-options"),
        "{}",
        part(&page, "more-options")
    );
    assert!(!is_open(&page, "more-english"));
}

#[tokio::test]
async fn an_image_error_opens_the_images_part_only() {
    let w = boot_public().await;
    let b = signed_in(&w, "Officer").await;
    // A PNG named .jpg is refused under its slot; a fifth image under "images".
    for file in [
        f("image1", "map.jpg", png(10, 10)),
        f("image5", "fifth.png", png(10, 10)),
    ] {
        let page = post_new(&b, "ห้อง 3105", vec![file]).await;
        assert_eq!(
            files_open(&page),
            (true, false),
            "{}",
            part(&page, "more-images")
        );
        assert_eq!(
            harpseal_testing::a11y::problems(&page),
            Vec::<String>::new()
        );
    }
}

#[tokio::test]
async fn a_pdf_error_opens_the_pdfs_part_only() {
    let w = boot_public().await;
    let b = signed_in(&w, "Officer").await;
    let page = post_new(
        &b,
        "ห้อง 3105",
        vec![f("pdf1", "notes.pdf", b"not a pdf".to_vec())],
    )
    .await;
    assert_eq!(
        files_open(&page),
        (false, true),
        "{}",
        part(&page, "more-pdfs")
    );
}

#[tokio::test]
async fn a_lost_file_opens_the_part_of_its_own_kind() {
    let w = boot_public().await;
    let b = signed_in(&w, "Officer").await;
    // Good files, but no Thai body: the form comes back without them.
    let page = post_new(&b, "", vec![f("pdf1", "ตารางสอบ.pdf", pdf())]).await;
    assert_eq!(files_open(&page), (false, true), "PDF only");
    assert!(page.contains("id=\"err-reselect-pdfs\""), "{page}");
    assert!(!page.contains("id=\"err-reselect\""), "{page}");
    assert_eq!(
        harpseal_testing::a11y::problems(&page),
        Vec::<String>::new()
    );

    let page = post_new(&b, "", vec![f("image1", "a.png", png(20, 13))]).await;
    assert_eq!(files_open(&page), (true, false), "image only");
    assert!(page.contains("id=\"err-reselect\""), "{page}");
    assert!(!page.contains("id=\"err-reselect-pdfs\""), "{page}");

    let page = post_new(
        &b,
        "",
        vec![f("image1", "a.png", png(20, 13)), f("pdf1", "a.pdf", pdf())],
    )
    .await;
    assert_eq!(files_open(&page), (true, true), "both");
}

#[tokio::test]
async fn a_saved_image_or_pdf_opens_its_part_when_editing() {
    let w = boot_public().await;
    let b = signed_in(&w, "Officer").await;
    let image = create_parts(&b, "ห้อง 3105", vec![f("image1", "a.png", png(20, 13))]).await;
    let page = b
        .get(&format!("/admin/announcements/{image}/edit"))
        .await
        .body;
    assert_eq!(files_open(&page), (true, false));
    let doc = create_parts(&b, "ดูไฟล์แนบ", vec![f("pdf1", "ตารางสอบ.pdf", pdf())]).await;
    let page = b
        .get(&format!("/admin/announcements/{doc}/edit"))
        .await
        .body;
    assert_eq!(files_open(&page), (false, true));
}

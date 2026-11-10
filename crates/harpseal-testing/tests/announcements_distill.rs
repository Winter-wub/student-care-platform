//! The shorter editor (#140): title, body, Audience and the send choice
//! come first; the optional parts (English, images, PDFs, expiry and the
//! acknowledgement) sit behind disclosures that open on load when they
//! hold a value or an error. Field names are unchanged.

mod announcements_support;

use announcements_support::*;

/// The `<details>` tag of the optional part `id`.
fn part<'p>(page: &'p str, id: &str) -> &'p str {
    let at = page
        .find(&format!("id=\"{id}\""))
        .unwrap_or_else(|| panic!("no {id}: {page}"));
    let start = page[..at].rfind("<details").expect("a details");
    &page[start..at + page[at..].find('>').unwrap()]
}

fn is_open(page: &str, id: &str) -> bool {
    part(page, id).contains(" open")
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

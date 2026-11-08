//! Announcement images (#109): the upload (sniff, size, count, Thai
//! errors, the hint), placement in the Mini App, the chat Card's image on
//! both Fake Channels, and edits that add, replace or remove images.

mod announcements_support;
mod app_support;
mod images_support;

use announcements_support::*;
use app_support::*;
use images_support::*;

const HINT: &str = "ไม่เกิน 4 รูป รูปละไม่เกิน 1 MB (JPEG/PNG) ย่อรูปก่อนอัปโหลด \
                    และห้ามใส่ข้อมูลส่วนตัวในรูป";

#[tokio::test]
async fn the_form_shows_the_hint_and_takes_files() {
    let w = boot_public().await;
    let b = signed_in(&w, "Officer").await;
    let page = b.get("/admin/announcements/new").await.body;
    assert!(page.contains(HINT), "{page}");
    assert!(page.contains("enctype=\"multipart/form-data\""));
    assert!(page.contains("accept=\"image/jpeg,image/png\""));
    assert!(
        page.contains("data-md=\"image\""),
        "the toolbar's image button"
    );
    assert!(!page.contains("ยังไม่ได้ตั้งค่า public_url"));
    // Without public_url the hint says chat Cards have no image yet.
    let w2 = boot().await;
    let b2 = signed_in(&w2, "Officer").await;
    let page = b2.get("/admin/announcements/new").await.body;
    assert!(page.contains("ยังไม่ได้ตั้งค่า public_url"), "{page}");
}

#[tokio::test]
async fn refused_files_name_the_limit_they_broke_in_thai() {
    let w = boot_public().await;
    let b = signed_in(&w, "Officer").await;
    let base = || vec![t("title_th", "ย้ายห้องสอบ"), t("body_th", "ห้อง 3105")];
    let svg = b"<svg xmlns=\"http://www.w3.org/2000/svg\"><script>alert(1)</script></svg>";
    let bad_type = "รับเฉพาะรูป JPEG หรือ PNG ที่นามสกุลไฟล์ตรงกับชนิดรูป";
    let mut big = png(10, 10);
    big.resize(1024 * 1024 + 1, 0);
    for (file, why) in [
        (f("image1", "map.jpg", png(10, 10)), bad_type),
        (f("image1", "map.png", svg.to_vec()), bad_type),
        (f("image1", "logo.svg", svg.to_vec()), bad_type),
        (
            f("image2", "anim.gif", b"GIF89a\x01\x00\x01\x00".to_vec()),
            bad_type,
        ),
        (
            f("image2", "anim.png", b"GIF89a\x01\x00\x01\x00".to_vec()),
            bad_type,
        ),
        (
            f("image3", "notes.png", b"hello, not an image".to_vec()),
            bad_type,
        ),
        (f("image1", "big.png", big), "รูปใหญ่เกิน 1 MB"),
        (f("image5", "fifth.png", png(10, 10)), "แนบรูปได้ไม่เกิน 4 รูป"),
        (f("image1", "wide.png", png(4097, 10)), PIXELS),
        (f("image1", "tall.jpg", jpeg(10, 4097)), PIXELS),
        (f("image1", "huge.png", png(4096, 4097)), PIXELS),
        (f("image1", "square.png", png(4096, 4096)), PIXELS),
        // A JPEG with no start-of-frame: not an image we can size.
        (
            f("image1", "noframe.jpg", vec![0xFF, 0xD8, 0xFF, 0xD9]),
            bad_type,
        ),
    ] {
        let mut parts = base();
        parts.push(file);
        let r = post_parts(&b, "/admin/announcements/new", &parts).await;
        assert_eq!(r.status, 422, "{why}");
        assert!(r.body.contains(why), "{why}: {}", r.body);
        assert!(r.body.contains("เลือกไฟล์อีกครั้ง"), "files are lost: say so");
        assert!(r.body.contains("ห้อง 3105"), "the text comes back");
    }
    // One slot posted twice: refused as too many, not a server error; and
    // each message once, however many files broke it.
    let mut parts = base();
    parts.push(f("image1", "a.png", png(10, 10)));
    parts.push(f("image1", "b.png", png(10, 10)));
    parts.push(f("image9", "c.png", png(10, 10)));
    let r = post_parts(&b, "/admin/announcements/new", &parts).await;
    assert_eq!(r.status, 422);
    assert_eq!(r.body.matches("แนบรูปได้ไม่เกิน 4 รูป").count(), 1, "{}", r.body);
    assert_eq!(count(&w, "SELECT count(*) FROM announcement").await, 0);
    assert!(w.h.files().keys().is_empty(), "nothing stored");
}

const PIXELS: &str = "รูปใหญ่เกิน 4096 พิกเซลต่อด้าน หรือเกิน 16 ล้านพิกเซล";
const HERO_LARGE: &str = "LINE อาจไม่แสดงรูปบนการ์ด";
const REMOVE_SENT: &str = "จนกว่าจะถอนประกาศหรือประกาศหมดอายุ";

#[tokio::test]
async fn a_hero_over_1024_pixels_is_saved_with_a_warning() {
    let w = boot_public().await;
    let b = signed_in(&w, "Officer").await;
    // Slot 2 is placed first, so it is the hero; slot 1 is small.
    let id = create_parts(
        &b,
        "![](image:2)",
        vec![
            f("image1", "small.png", png(1000, 650)),
            f("image2", "big.jpg", jpeg(2000, 1300)),
        ],
    )
    .await;
    assert_eq!(stored(&w, &id).await.len(), 2, "saved, not refused");
    let edit = b.get(&format!("/admin/announcements/{id}/edit")).await.body;
    assert!(edit.contains(HERO_LARGE), "{edit}");
    assert!(!edit.contains(REMOVE_SENT), "a draft deletes at once");
    let detail = b.get(&format!("/admin/announcements/{id}")).await.body;
    assert!(detail.contains(HERO_LARGE));
    // The English body's Card shows its own first image: checked too.
    let r = edit_parts(
        &w,
        &b,
        &id,
        "ห้อง 3105",
        vec![t("title_en", "Room change"), t("body_en", "![](image:2)")],
    )
    .await;
    assert_eq!(r.status, 303, "{}", r.body);
    let edit = b.get(&format!("/admin/announcements/{id}/edit")).await.body;
    assert!(edit.contains(HERO_LARGE), "the English Card's hero");
    let r = edit_parts(&w, &b, &id, "ห้อง 3105", vec![]).await;
    assert_eq!(r.status, 303, "{}", r.body);
    // The hero within 1024 px: no warning (slot 2 not placed any more).
    let r = edit_parts(&w, &b, &id, "ห้อง 3105", vec![t("image2_remove", "1")]).await;
    assert_eq!(r.status, 303, "{}", r.body);
    let edit = b.get(&format!("/admin/announcements/{id}/edit")).await.body;
    assert!(!edit.contains(HERO_LARGE));
}

#[tokio::test]
async fn a_body_over_the_limit_is_refused_before_it_is_read() {
    let w = boot_public().await;
    let b = signed_in(&w, "Officer").await;
    let huge = vec![0u8; 5 * 1024 * 1024];
    let r = post_parts(
        &b,
        "/admin/announcements/new",
        &[t("title_th", "x"), f("image1", "a.png", huge)],
    )
    .await;
    assert_eq!(r.status, 413);
    assert_eq!(count(&w, "SELECT count(*) FROM announcement").await, 0);
    // Without the Permission: refused before the body, whatever its size.
    let viewer = signed_in(&w, "Viewer").await;
    let r = post_parts(&viewer, "/admin/announcements/new", &[t("title_th", "x")]).await;
    assert_eq!(r.status, 403);
}

#[tokio::test]
async fn images_show_in_place_in_the_mini_app_and_the_first_placed_tops_the_card() {
    let w = boot_public().await;
    roster(&w).await;
    let b = signed_in(&w, "Officer").await;
    let body = "สอบย้ายไปห้องใหม่\n![แผนที่ \"ห้อง\" <b>](image:2)\nนำบัตรมาด้วย";
    let id = create_parts(
        &b,
        body,
        vec![
            f("image1", "poster.PNG", png(1040, 676)),
            f("image2", "map.jpeg", jpeg(800, 520)),
        ],
    )
    .await;
    let (t1, t2) = (token(&w, &id, "1").await, token(&w, &id, "2").await);
    assert_eq!(t1.len(), 32);
    assert_ne!(t1, t2);
    assert_eq!(w.h.files().keys().len(), 2);
    send_now(&w, &b, &id).await;

    // Chat: the first image the body places (slot 2), on both Channels.
    for card in [
        card_of(w.line.sent_to(&fake_account("fake", "s1"))),
        card_of(w.tg.sent_to(&fake_account(TG, "s1"))),
    ] {
        let img = card.image.expect("a Card image");
        assert_eq!(img.url, format!("{PUBLIC}/announcements/files/{t2}"));
        assert_eq!(
            card.body, "สอบย้ายไปห้องใหม่\nนำบัตรมาด้วย",
            "the image line left out"
        );
    }

    // Mini App: slot 2 in place, slot 1 after the body, lazy, escaped alt.
    let app = open(&w, "fake", "s1").await;
    let page = app.get(&format!("/app/announcements/{id}")).await.body;
    let placed = format!(
        "สอบย้ายไปห้องใหม่\n<img class=\"app-img\" src=\"/announcements/files/{t2}\" \
         alt=\"แผนที่ &quot;ห้อง&quot; &lt;b&gt;\" loading=\"lazy\" decoding=\"async\" \
         width=\"800\" height=\"520\">\nนำบัตรมาด้วย"
    );
    assert!(page.contains(&placed), "{page}");
    let after = format!(
        "<div class=\"app-images\"><img class=\"app-img\" src=\"/announcements/files/{t1}\" \
         alt=\"รูปที่ 1\" loading=\"lazy\""
    );
    assert!(page.contains(&after), "{page}");
    // The list on /app expands to the same.
    assert!(app.get("/app").await.body.contains(&placed));
    // Staff see them through the back office.
    let detail = b.get(&format!("/admin/announcements/{id}")).await.body;
    assert!(detail.contains(&format!("/admin/announcements/{id}/files/{t1}")));
}

#[tokio::test]
async fn without_public_url_the_card_has_no_image_and_the_mini_app_still_shows_it() {
    let w = boot().await;
    roster(&w).await;
    let b = signed_in(&w, "Officer").await;
    let id = create_parts(&b, "ห้อง 3105", vec![f("image1", "a.png", png(20, 13))]).await;
    send_now(&w, &b, &id).await;
    let card = card_of(w.line.sent_to(&fake_account("fake", "s1")));
    assert!(card.image.is_none());
    let app = open(&w, "fake", "s1").await;
    let page = app.get(&format!("/app/announcements/{id}")).await.body;
    let tok = token(&w, &id, "1").await;
    assert!(page.contains(&format!("src=\"/announcements/files/{tok}\"")));
}

#[tokio::test]
async fn an_edit_replaces_and_removes_images_deleting_objects_only_before_sending() {
    let w = boot_public().await;
    roster(&w).await;
    let b = signed_in(&w, "Officer").await;
    let id = create_parts(
        &b,
        "ห้อง 3105",
        vec![
            f("image1", "a.png", png(20, 13)),
            f("image2", "b.png", png(20, 13)),
        ],
    )
    .await;
    let before = stored(&w, &id).await;
    // A draft: replace 1, remove 2.
    let r = edit_parts(
        &w,
        &b,
        &id,
        "ห้อง 3105",
        vec![f("image1", "c.jpg", jpeg(40, 26)), t("image2_remove", "1")],
    )
    .await;
    assert_eq!(r.status, 303, "{}", r.body);
    let after = stored(&w, &id).await;
    assert_eq!(after.len(), 1);
    assert_ne!(after[0].1, before[0].1, "a replaced image gets a new token");
    let keys = w.h.files().keys();
    assert_eq!(
        keys,
        vec![after[0].2.clone()],
        "removed by edit -> object deleted"
    );

    // Sent: an image-only edit edits the Cards, notifies no one.
    send_now(&w, &b, &id).await;
    w.line.take_sent();
    let r = edit_parts(
        &w,
        &b,
        &id,
        "ห้อง 3105",
        vec![f("image1", "d.png", png(20, 13))],
    )
    .await;
    assert_eq!(r.status, 303, "{}", r.body);
    drain(&w).await;
    assert!(w.line.sent().is_empty(), "no notice for an image change");
    let edited = w.tg.edited();
    let newest = token(&w, &id, "1").await;
    assert!(
        edited.iter().any(|(_, m)| matches!(m,
            harpseal::Message::Card(c) if c.image.as_ref().is_some_and(|i| i.url.ends_with(&newest)))),
        "{edited:?}"
    );
    // Cards already delivered still show the replaced image: superseded,
    // still served, its object kept.
    assert_eq!(w.h.files().keys().len(), 2);
    let old = app_get(&w, &format!("/announcements/files/{}", after[0].1)).await;
    assert_eq!(old, 200, "the replaced image of a sent one stays served");
}

/// The status of an anonymous `GET path`.
async fn app_get(w: &World, path: &str) -> u16 {
    w.app.get(path).await.status.as_u16()
}

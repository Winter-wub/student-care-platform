//! The Texts page labelled by purpose (#125): texts grouped under the page
//! they show on, searchable by text, page or key, filtered by page, with
//! the key itself only in the technical details.

mod admin_support;
mod support;

use admin_support::{boot, htmx_get, signed_in};
use harpseal_testing::urlencode;

/// The page before its technical details.
fn main_part(body: &str) -> &str {
    body.split_once("<details class=\"tech\">")
        .map_or(body, |(m, _)| m)
}

/// The `data-area` of every group shown.
fn areas(body: &str) -> Vec<&str> {
    body.split("data-area=\"")
        .skip(1)
        .filter_map(|s| s.split('"').next())
        .collect()
}

#[tokio::test]
async fn texts_are_grouped_by_page_and_never_named_by_key() {
    let (h, app) = boot().await;
    let (admin, _) = signed_in(&h, &app, "Administrator").await;
    let page = admin.get("/admin/texts").await.body;
    let main = main_part(&page);
    assert!(main.contains("<h2>แชทกับนักศึกษา "), "{main}");
    assert!(main.contains("<h2>มินิแอป "), "{main}");
    assert!(main.contains("<option value=\"kernel.chat\">แชทกับนักศึกษา</option>"));
    // The Locale columns are named, and the welcome links by its text.
    assert!(
        main.contains("<th>ไทย</th>") && main.contains("<th>อังกฤษ</th>"),
        "{main}"
    );
    assert!(
        main.contains("href=\"/admin/texts/kernel.flow.welcome\">สวัสดีครับ"),
        "{main}"
    );
    let shown: String = main
        .split('<')
        .map(|s| s.split_once('>').map_or(s, |(_, t)| t))
        .collect();
    assert!(!shown.contains("kernel."), "a key is shown: {shown}");
    // The edit page is titled by the text and where it shows.
    let edit = admin.get("/admin/texts/kernel.flow.welcome").await.body;
    let (top, tech) = edit.split_once("<details class=\"tech\">").unwrap();
    assert!(top.contains("<h1>สวัสดีครับ ยินดีต้อนรับ"), "{top}");
    assert!(top.contains(">แชทกับนักศึกษา</a>"), "{top}");
    assert!(!top.contains(">kernel.flow.welcome<"), "{top}");
    assert!(tech.contains("<code>kernel.flow.welcome</code>"), "{tech}");
}

#[tokio::test]
async fn texts_are_searched_by_text_page_or_key_and_filtered_by_page() {
    let (h, app) = boot().await;
    let (admin, _) = signed_in(&h, &app, "Administrator").await;
    let all = admin.get("/admin/texts").await.body;
    let every = areas(&all);
    assert!(every.len() > 3, "{every:?}");

    // By the text, in Thai.
    let q = urlencode("ยินดีต้อนรับสู่ช่องทางบริการ");
    let r = admin.get(&format!("/admin/texts?q={q}")).await.body;
    assert!(
        r.contains("href=\"/admin/texts/kernel.flow.welcome\""),
        "{r}"
    );
    assert_eq!(areas(&r), ["kernel.chat"], "{r}");
    // By the key, for whoever has it from the technical details.
    let r = admin.get("/admin/texts?q=kernel.flow.welcome").await.body;
    assert!(
        r.contains("href=\"/admin/texts/kernel.flow.welcome\""),
        "{r}"
    );
    // By the page's name: every text of that page.
    let r = admin
        .get(&format!("/admin/texts?q={}", urlencode("มินิแอป")))
        .await
        .body;
    assert!(areas(&r).contains(&"kernel.app"), "{r}");
    // Filtered by page, as the select sends it; HTMX gets the list only.
    let r = htmx_get(&admin, "/admin/texts?q=&area=kernel.app")
        .await
        .body;
    assert!(r.starts_with("<div id=\"texts-results\">"), "{r}");
    assert_eq!(areas(&r), ["kernel.app"], "{r}");
    let full = admin.get("/admin/texts?area=kernel.app").await.body;
    assert!(
        full.contains("<option value=\"kernel.app\" selected>"),
        "{full}"
    );
    // Nothing matches: says so.
    let r = admin
        .get(&format!("/admin/texts?q={}", urlencode("ไม่มีข้อความนี้แน่นอน")))
        .await
        .body;
    assert!(areas(&r).is_empty() && r.contains("ไม่พบรายการ"), "{r}");
}

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

/// Texts the back office reads only from the catalog are not offered for
/// editing (#125): a saved edit would never show. The emails stay.
#[tokio::test]
async fn labels_and_page_texts_are_not_offered_but_emails_are() {
    let (h, app) = boot().await;
    let (admin, _) = signed_in(&h, &app, "Administrator").await;
    for key in [
        "kernel.role.Administrator",
        "kernel.locale.th",
        "kernel.backup.total",
        "kernel.usage.metered",
        "kernel.home.title",
    ] {
        let r = admin.get(&format!("/admin/texts/{key}")).await;
        assert_eq!(r.status, 404, "{key} is offered");
        let r = admin
            .post(
                &format!("/admin/texts/{key}"),
                "locale=th&value=x&version=-&op=save",
            )
            .await;
        assert_ne!(r.status, 303, "{key} was saved");
    }
    for key in ["kernel.backup.email_subject", "kernel.usage.email_body"] {
        assert_eq!(
            admin.get(&format!("/admin/texts/{key}")).await.status,
            200,
            "{key}"
        );
    }
    let list = admin.get("/admin/texts").await.body;
    for raw in [
        "/admin/texts/kernel.role.",
        "/admin/texts/kernel.locale.",
        "/admin/texts/kernel.home.",
    ] {
        assert!(!list.contains(raw), "{raw} listed");
    }
}

/// The edit page is titled by the text in use, an edit included.
#[tokio::test]
async fn the_edit_page_title_follows_an_edit() {
    let (h, app) = boot().await;
    let (admin, _) = signed_in(&h, &app, "Administrator").await;
    let path = "/admin/texts/kernel.flow.welcome";
    let body = format!(
        "locale=th&value={}&version=-&op=save",
        urlencode("ยินดีต้อนรับ ฉบับแก้ไข")
    );
    assert_eq!(admin.post(path, &body).await.status, 303);
    let page = admin.get(path).await.body;
    assert!(page.contains("<h1>ยินดีต้อนรับ ฉบับแก้ไข</h1>"), "{page}");
}

/// The `data-area` of every group shown open (#142).
fn open_areas(body: &str) -> Vec<&str> {
    body.split("<details class=\"text-group\" data-area=\"")
        .skip(1)
        .filter(|s| {
            s.split_once('>')
                .is_some_and(|(tag, _)| tag.ends_with(" open"))
        })
        .filter_map(|s| s.split('"').next())
        .collect()
}

/// The result count's text, from the live region or its HTMX update.
fn count(body: &str) -> &str {
    body.split_once("id=\"texts-count\"")
        .and_then(|(_, rest)| rest.split_once('>'))
        .and_then(|(_, rest)| rest.split_once('<'))
        .map_or("", |(text, _)| text)
}

/// Groups load collapsed; a search opens the groups that match and says
/// how many texts it found, once per update (#142).
#[tokio::test]
async fn groups_load_collapsed_and_a_search_opens_the_matches() {
    let (h, app) = boot().await;
    let (admin, _) = signed_in(&h, &app, "Administrator").await;
    let all = admin.get("/admin/texts").await.body;
    assert!(areas(&all).len() > 3, "{all}");
    assert!(open_areas(&all).is_empty(), "{all}");
    assert!(count(&all).starts_with("ข้อความทั้งหมด "), "{all}");
    assert!(
        all.contains("aria-describedby=\"texts-count\""),
        "the search names its count"
    );
    let problems = harpseal_testing::a11y::problems(&all);
    assert!(problems.is_empty(), "{problems:?}");

    // By a Thai value, an English value, or a key, in any case.
    for q in [
        "ยินดีต้อนรับสู่ช่องทางบริการ",
        "WELCOME TO STUDENT SERVICES",
        "Kernel.Flow.Welcome",
    ] {
        let r = admin
            .get(&format!("/admin/texts?q={}", urlencode(q)))
            .await
            .body;
        assert!(r.contains("/admin/texts/kernel.flow.welcome\""), "{q}: {r}");
        assert!(open_areas(&r).contains(&"kernel.chat"), "{q}: {r}");
        assert_eq!(open_areas(&r), areas(&r), "{q}: every match is open");
        assert!(count(&r).starts_with("พบ "), "{q}: {r}");
    }

    // HTMX: the list, then the count out of band into the live region
    // the page already has, so it is announced once.
    let r = htmx_get(&admin, "/admin/texts?q=kernel.flow.welcome&area=")
        .await
        .body;
    assert!(r.starts_with("<div id=\"texts-results\">"), "{r}");
    assert!(
        r.contains("<span id=\"texts-count\" hx-swap-oob=\"innerHTML\">"),
        "{r}"
    );
    assert!(
        !r.contains("aria-live") && !r.contains("role=\"status\""),
        "{r}"
    );
    let problems = harpseal_testing::a11y::problems(&r);
    assert!(problems.is_empty(), "{problems:?}");

    // Filtered by page: that page's group is open.
    let r = admin.get("/admin/texts?area=kernel.app").await.body;
    assert_eq!(open_areas(&r), ["kernel.app"], "{r}");
}

/// A saved edit the current default no longer accepts (a placeholder it
/// dropped) opens its group and says what to do (#142).
#[tokio::test]
async fn a_group_with_an_edit_to_fix_is_open() {
    let (h, app) = boot().await;
    let (admin, _) = signed_in(&h, &app, "Administrator").await;
    admin_support::run(
        &h,
        "INSERT INTO setting (key, locale, value, updated_at) \
         VALUES ('kernel.inbound.file_too_large', 'th', 'ไฟล์ใหญ่เกินไป', 0)",
    )
    .await;
    let page = admin.get("/admin/texts").await.body;
    let open = open_areas(&page);
    assert_eq!(open.len(), 1, "{page}");
    let group = page
        .split(&format!("data-area=\"{}\"", open[0]))
        .nth(1)
        .and_then(|g| g.split("</details>").next())
        .unwrap();
    assert!(
        group.contains("/admin/texts/kernel.inbound.file_too_large\""),
        "{group}"
    );
    assert!(
        group.contains("<span class=\"chip err\">ต้องแก้ไข 1 ข้อความ</span>"),
        "{group}"
    );
    assert!(group.contains("class=\"field-error\""), "{group}");
    let problems = harpseal_testing::a11y::problems(&page);
    assert!(problems.is_empty(), "{problems:?}");
}

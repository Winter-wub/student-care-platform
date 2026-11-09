//! The roster's help where the work is (#127): beside the file input, the
//! columns `[roster]` maps (label, may it be empty, an example), the file
//! rules and the template; the same columns under a refused header; and
//! after a confirmed import, the next step "แจ้งนักศึกษา".

mod roster_support;

use harpseal::DeploymentConfig;
use harpseal::modules::announcements::Announcements;
use roster_support::*;

/// [`boot`] with the Announcements Module, the Officer allowed to send.
async fn boot_sending() -> (harpseal_testing::Harness, harpseal_testing::TestApp) {
    let h = harpseal_testing::Harness::new();
    let toml = CONFIG.replace(
        "\"kernel.roster.view\", \"kernel.roster.import\"]",
        "\"kernel.roster.view\", \"kernel.roster.import\",\n           \"announcements.view\", \"announcements.send\"]",
    );
    assert_ne!(toml, CONFIG);
    let config = DeploymentConfig::from_toml(&toml).unwrap();
    let d = h
        .builder()
        .config(config)
        .module(Announcements)
        .build()
        .unwrap();
    let app = h.boot(d).await;
    (h, app)
}

/// The help panel beside the upload form.
fn help(page: &str) -> String {
    let at = page.find("id=\"upload-help\"").expect("the help panel");
    page[at..at + page[at..].find("</aside>").unwrap()].to_string()
}

/// The panel's line for the column `header`.
fn column<'a>(help: &'a str, header: &str) -> &'a str {
    let at = help
        .find(&format!("<span class=\"name code-head\">{header}</span>"))
        .unwrap_or_else(|| panic!("{header} in {help}"));
    &help[at..at + help[at..].find("</li>").unwrap()]
}

#[tokio::test]
async fn the_upload_form_has_the_columns_rules_and_template_beside_it() {
    let (h, app) = boot().await;
    let b = signed_in(&h, &app, "Officer").await;
    let page = b.get("/admin/roster").await.body;
    // Side by side: the form, then the panel, in one `.cols`.
    let pair = page.find("class=\"cols upload-pair\"").expect("the pair");
    let form = page.find("action=\"/admin/roster/upload\"").unwrap();
    assert!(pair < form && form < page.find("id=\"upload-help\"").unwrap());
    // No longer hidden in a marker-less <details>.
    assert!(!page.contains("<details class=\"card\">"), "{page}");
    let help = help(&page);
    assert!(help.contains("ไฟล์ CSV หรือ XLSX ขนาดไม่เกิน 4 MB"), "{help}");
    assert!(help.contains("CSV UTF-8"), "{help}");
    assert!(help.contains("เรียงลำดับใดก็ได้"), "{help}");
    for h in HEADERS.split(',') {
        column(&help, h);
    }
    let code = column(&help, "รหัสนักศึกษา");
    assert!(
        code.contains("ต้องมีค่า") && code.contains("68010001"),
        "{code}"
    );
    let email = column(&help, "อีเมล");
    assert!(email.contains("เว้นว่างได้"), "{email}");
    let tags = column(&help, "กลุ่ม");
    assert!(tags.contains("ชมรมดนตรี;ทุนเรียนดี"), "the separator: {tags}");
    let programme = column(&help, "หลักสูตร");
    assert!(programme.contains("MSC, MSC-Intl") && programme.contains("ตัวอย่าง:"));
    assert!(column(&help, "เกรดเฉลี่ย").contains("เว้นว่างได้"));
    assert!(
        help.contains("href=\"/admin/roster/template.csv\" download"),
        "{help}"
    );
}

#[tokio::test]
async fn staff_who_may_only_look_get_the_help_without_the_form() {
    let (h, app) = boot().await;
    let b = signed_in(&h, &app, "Viewer").await;
    let page = b.get("/admin/roster").await.body;
    assert!(!page.contains("action=\"/admin/roster/upload\""));
    column(&help(&page), "รหัสนักศึกษา");
}

#[tokio::test]
async fn a_refused_header_shows_the_same_columns() {
    let (h, app) = boot().await;
    let b = signed_in(&h, &app, "Officer").await;
    let r = upload(
        &b,
        "roster.csv",
        "รหัส,ชื่อ\n68010001,นกน้อย\n".as_bytes(),
        "full",
    )
    .await;
    let id = import_id(&r);
    let page = b.get(&format!("/admin/roster/{id}")).await.body;
    let at = page
        .find("id=\"expected-headers\"")
        .expect("the header help");
    let card = &page[at..];
    assert!(
        card.contains("<span class=\"name code-head\">ชั้นปี</span>"),
        "{card}"
    );
    assert!(card.contains("ไฟล์ที่ต้องเตรียม"), "{card}");
}

#[tokio::test]
async fn a_confirmed_import_offers_the_next_step() {
    let (h, app) = boot_sending().await;
    let b = signed_in(&h, &app, "Officer").await;
    let id = previewed(&b, "roster.csv", CSV_50, "upsert").await;
    // Before the confirm: no next step yet.
    let before = b.get(&format!("/admin/roster/{id}")).await.body;
    assert!(!before.contains("id=\"next-step\""));
    let r = confirm(&b, &id).await;
    assert_eq!(r.status, 303, "{}", r.body);
    run_jobs(&h, &app).await;
    assert_eq!(import_text(&h, &id, "state").await, "done");
    let page = b.get(&format!("/admin/roster/{id}")).await.body;
    let at = page.find("id=\"next-step\"").expect("the next step");
    let next = &page[at..at + page[at..].find("</section>").unwrap()];
    // A roster file cannot name its Audience: a bare draft, Staff choose.
    assert!(
        next.contains("<a class=\"btn\" href=\"/admin/announcements/new\">แจ้งนักศึกษา</a>"),
        "{next}"
    );
    assert!(next.contains("เลือกกลุ่มผู้รับเอง"), "{next}");
    // Back to the imports stays available.
    assert!(next.contains("href=\"/admin/roster\""), "{next}");
    // The HTMX fragment carries it too, so it appears when the run ends.
    let req = http::Request::get(format!("/admin/roster/{id}"))
        .header("hx-request", "true")
        .body(axum::body::Body::empty())
        .unwrap();
    let frag = b.send(req).await.body;
    assert!(frag.starts_with("<div id=\"roster-progress\""), "{frag}");
    assert!(frag.contains("id=\"next-step\""), "{frag}");
}

#[tokio::test]
async fn staff_who_cannot_send_get_only_the_way_back() {
    let (h, app) = boot().await;
    let b = signed_in(&h, &app, "Officer").await;
    let id = previewed(&b, "roster.csv", CSV_50, "upsert").await;
    confirm(&b, &id).await;
    run_jobs(&h, &app).await;
    let page = b.get(&format!("/admin/roster/{id}")).await.body;
    let at = page.find("id=\"next-step\"").expect("the next step");
    let next = &page[at..at + page[at..].find("</section>").unwrap()];
    assert!(!next.contains("/admin/announcements/new"), "{next}");
    assert!(next.contains("href=\"/admin/roster\""), "{next}");
}

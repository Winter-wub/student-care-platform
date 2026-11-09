//! Empty pages that teach (#127): each main list with nothing yet says
//! what goes there and offers the action that fills it, only to Staff
//! holding its Permission; a search or filter with no match offers to
//! clear it. Plus the draft "แจ้งนักศึกษา" opens (the Audience prefilled
//! only when named exactly) and the editor's preview before any text.

use harpseal::modules::announcements::Announcements;
use harpseal::{DeploymentConfig, Statement};
use harpseal_testing::{Browser, Harness, TestApp};

const PASSWORD: &str = "kelp forest at dawn";

const CONFIG: &str = r#"
name = "Seal Bay University (lists)"
default_locale = "th"
utc_offset = "+07:00"

[roles]
Administrator = [
    "kernel.student.view", "kernel.roster.view", "kernel.roster.import",
    "kernel.staff.manage", "kernel.audit.view", "kernel.security.view",
    "kernel.setting.edit",
    "announcements.view", "announcements.send",
]
Reader = ["kernel.student.view", "announcements.view"]
"#;

async fn boot() -> (Harness, TestApp) {
    let h = Harness::new();
    let d = h
        .builder()
        .config(DeploymentConfig::from_toml(CONFIG).unwrap())
        .module(Announcements)
        .build()
        .unwrap();
    let app = h.boot(d).await;
    h.create_staff("admin@sealbay.example", PASSWORD, &["Administrator"])
        .await;
    (h, app)
}

async fn as_who<'a>(h: &Harness, app: &'a TestApp, who: &str, role: &str) -> Browser<'a> {
    let email = format!("{who}@sealbay.example");
    if who != "admin" {
        h.create_staff(&email, PASSWORD, &[role]).await;
    }
    let b = app.browser();
    b.login(&email, PASSWORD).await;
    b
}

/// The empty-state card, if any.
fn empty(body: &str) -> Option<String> {
    let at = body.find("<div class=\"card empty\">")?;
    Some(body[at..at + body[at..].find("</div>").unwrap()].to_string())
}

#[tokio::test]
async fn an_empty_students_list_points_to_the_roster_for_who_may_import() {
    let (h, app) = boot().await;
    let admin = as_who(&h, &app, "admin", "").await;
    let card = empty(&admin.get("/admin/students").await.body).expect("the empty state");
    assert!(card.contains("รายชื่อนักศึกษามาจากไฟล์ทะเบียน"), "{card}");
    assert!(
        card.contains("<a class=\"btn\" href=\"/admin/roster\">นำเข้ารายชื่อ</a>"),
        "{card}"
    );
    let reader = as_who(&h, &app, "reader", "Reader").await;
    let card = empty(&reader.get("/admin/students").await.body).expect("the empty state");
    assert!(card.contains("รายชื่อนักศึกษามาจากไฟล์ทะเบียน"), "{card}");
    assert!(
        !card.contains("href="),
        "no action without the Permission: {card}"
    );
}

#[tokio::test]
async fn a_search_with_no_match_offers_to_clear_it() {
    let (h, app) = boot().await;
    h.db()
        .execute(Statement::new(
            "INSERT INTO student (id, student_code, display_name, locale, created_at, updated_at) \
             VALUES ('s1', '68010001', 'นกน้อย ทะเลใส', 'th', 0, 0)",
        ))
        .await
        .unwrap();
    let b = as_who(&h, &app, "admin", "").await;
    let card = empty(&b.get("/admin/students?q=zzz").await.body).expect("no match");
    assert!(card.contains("ไม่พบนักศึกษาที่ตรงกับคำค้น"), "{card}");
    assert!(card.contains("href=\"/admin/students\""), "{card}");
    // With Students and no search: the table, no empty state.
    assert_eq!(empty(&b.get("/admin/students").await.body), None);
}

#[tokio::test]
async fn an_empty_announcements_list_offers_to_write_one_to_who_may_send() {
    let (h, app) = boot().await;
    let admin = as_who(&h, &app, "admin", "").await;
    let card = empty(&admin.get("/admin/announcements").await.body).expect("empty");
    assert!(card.contains("ประกาศที่ส่งถึงนักศึกษาจะแสดงที่นี่"), "{card}");
    assert!(
        card.contains("href=\"/admin/announcements/new\">เขียนประกาศ</a>"),
        "{card}"
    );
    let reader = as_who(&h, &app, "reader", "Reader").await;
    let card = empty(&reader.get("/admin/announcements").await.body).expect("empty");
    assert!(!card.contains("href="), "{card}");
}

#[tokio::test]
async fn the_staff_list_with_only_the_first_account_invites_more() {
    let (h, app) = boot().await;
    let b = as_who(&h, &app, "admin", "").await;
    let page = b.get("/admin/staff").await.body;
    let card = empty(&page).expect("only one account");
    assert!(card.contains("ตอนนี้มีแค่บัญชีของคุณ"), "{card}");
    assert!(card.contains("href=\"/admin/staff/new\""), "{card}");
    // The one action, once.
    assert_eq!(page.matches("href=\"/admin/staff/new\"").count(), 1);
    h.create_staff("reader@sealbay.example", PASSWORD, &["Reader"])
        .await;
    let page = b.get("/admin/staff").await.body;
    assert_eq!(empty(&page), None);
    assert_eq!(page.matches("href=\"/admin/staff/new\"").count(), 1);
}

#[tokio::test]
async fn the_audit_log_tells_an_empty_filter_from_an_empty_log() {
    let (h, app) = boot().await;
    let b = as_who(&h, &app, "admin", "").await;
    let card = empty(&b.get("/admin/audit?action=nothing.like.this").await.body).expect("no match");
    assert!(card.contains("ไม่พบรายการที่ตรงกับตัวกรองนี้"), "{card}");
    assert!(
        card.contains("href=\"/admin/audit\">ล้างตัวกรอง</a>"),
        "{card}"
    );
    let card = empty(&b.get("/admin/security?kind=nothing").await.body).expect("no match");
    assert!(card.contains("href=\"/admin/security\""), "{card}");
    // No Security Event at all, no filter: what the page is for.
    let card = empty(&b.get("/admin/security").await.body).expect("an empty log");
    assert!(card.contains("เหตุการณ์ความปลอดภัย"), "{card}");
    assert!(!card.contains("href="), "{card}");
}

/// The value of the hand-picked list and which scope radio is checked.
fn audience(page: &str) -> (String, Vec<String>) {
    let at = page.find("name=\"students\"").expect("the list field");
    let list = page[at..]
        .split('>')
        .nth(1)
        .unwrap()
        .split("</textarea")
        .next();
    let checked = page
        .split("name=\"scope\" value=\"")
        .skip(1)
        .filter(|r| r.split('>').next().unwrap().contains("checked"))
        .map(|r| r.split('"').next().unwrap().to_string())
        .collect();
    (list.unwrap_or_default().to_string(), checked)
}

#[tokio::test]
async fn the_next_step_opens_a_draft_with_the_named_students() {
    let (h, app) = boot().await;
    let b = as_who(&h, &app, "admin", "").await;
    let href = harpseal::ui::announce_href(&["68010001".into(), "68010002".into()]);
    assert_eq!(
        href,
        "/admin/announcements/new?scope=students&students=68010001+68010002"
    );
    let page = b.get(&href).await.body;
    let (list, checked) = audience(&page);
    assert_eq!(list, "68010001\n68010002");
    assert_eq!(checked, ["students"]);
    // A bare form, or any other query: nothing chosen (#120).
    for q in [
        "",
        "?scope=everyone",
        "?scope=groups&students=1",
        "?students=68010001",
    ] {
        let (list, checked) = audience(&b.get(&format!("/admin/announcements/new{q}")).await.body);
        assert!(
            list.is_empty() && checked.is_empty(),
            "{q}: {list} {checked:?}"
        );
    }
    // Nothing is created by opening it.
    let n = app.count("SELECT COUNT(*) AS n FROM announcement").await;
    assert_eq!(n, 0);
}

#[test]
fn a_list_too_long_for_a_link_names_no_one() {
    use harpseal::ui::{MAX_PREFILL, MAX_PREFILL_QUERY, announce_href};
    const BARE: &str = "/admin/announcements/new";
    let codes = |n: usize, width: usize| -> Vec<String> {
        (0..n).map(|i| format!("{:0width$}", i)).collect()
    };
    assert_eq!(announce_href(&[]), BARE);
    // Over the count: no one, however short.
    assert_eq!(announce_href(&codes(MAX_PREFILL + 1, 3)), BARE);
    assert_ne!(announce_href(&codes(MAX_PREFILL, 4)), BARE);
    // Within the count but over the query length: no one.
    let long = codes(800, 10);
    assert_eq!(
        announce_href(&long),
        BARE,
        "800 × 11 bytes > {MAX_PREFILL_QUERY}"
    );
    let fits = codes(500, 10);
    let href = announce_href(&fits);
    assert!(
        href.len() - BARE.len() - 1 <= MAX_PREFILL_QUERY,
        "{}",
        href.len()
    );
    // Repeats are dropped, in order, before counting.
    assert_eq!(
        announce_href(&["68010002".into(), "68010001".into(), "68010002".into()]),
        "/admin/announcements/new?scope=students&students=68010002+68010001"
    );
    let repeated = vec!["68010001".to_string(); MAX_PREFILL + 5];
    assert_ne!(
        announce_href(&repeated),
        BARE,
        "one Student, named many times"
    );
    assert_eq!(
        announce_href(&["a b&c".into()]),
        "/admin/announcements/new?scope=students&students=a%20b%26c"
    );
}

#[tokio::test]
async fn the_editor_previews_say_what_will_appear_before_any_text() {
    let (h, app) = boot().await;
    let b = as_who(&h, &app, "admin", "").await;
    let page = b.get("/admin/announcements/new").await.body;
    assert!(page.contains("ข้อความที่พิมพ์จะแสดงที่นี่"), "{page}");
    assert!(
        !page.contains("<div class=\"mini-preview\""),
        "no empty card"
    );
    assert!(page.contains("หัวข้อและข้อความจะแสดงในการ์ดนี้"), "{page}");
    // Once typed, the preview shows the text instead.
    let csrf = b.csrf().unwrap();
    let r = b
        .post(
            "/admin/announcements/preview",
            &format!("_csrf={csrf}&title_th=x&body_th=%E0%B8%AA%E0%B8%A7%E0%B8%B1%E0%B8%AA%E0%B8%94%E0%B8%B5"),
        )
        .await;
    assert!(r.body.contains("class=\"mini-preview\""), "{}", r.body);
    assert!(!r.body.contains("ข้อความที่พิมพ์จะแสดงที่นี่"), "{}", r.body);
}

#[tokio::test]
async fn overrides_of_texts_no_page_reads_still_build_and_are_not_offered() {
    // A pinned Deployment that overrode the old empty-list texts (#127
    // replaced them) keeps building; the Texts page does not offer them.
    let deprecated = [
        "kernel.admin.none",
        "kernel.students.none",
        "announcements.page.none",
        "kernel.roster.none",
        "kernel.roster.expected",
        "kernel.roster.header",
        "kernel.roster.holds",
        "kernel.roster.required",
        "kernel.roster.optional",
    ];
    let lines: Vec<String> = deprecated
        .iter()
        .map(|k| format!("\"{k}\" = \"ทับ {k}\""))
        .collect();
    let toml = format!("{CONFIG}\n[texts.th]\n{}\n", lines.join("\n"));
    let h = Harness::new();
    let d = h
        .builder()
        .config(DeploymentConfig::from_toml(&toml).unwrap())
        .module(Announcements)
        .build()
        .expect("overrides of deprecated texts still build");
    let app = h.boot(d).await;
    h.create_staff("admin@sealbay.example", PASSWORD, &["Administrator"])
        .await;
    let b = as_who(&h, &app, "admin", "").await;
    let page = b.get("/admin/texts?q=announcements.page").await.body;
    assert!(!page.contains("ทับ announcements.page.none"), "{page}");
    assert!(!page.contains("announcements.page.none"), "{page}");
}

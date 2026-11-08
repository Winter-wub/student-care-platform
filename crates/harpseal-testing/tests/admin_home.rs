//! The back-office shell and the term status home (#123): navigation
//! grouped and filtered by Permission, `aria-current`, the skip link, a
//! Module page in the Administrator group, the Kernel's status cards in
//! each state, a Module's card behind its Permission, and the technical
//! details on the administration pages.

use axum::routing::get;
use harpseal::modules::announcements::Announcements;
use harpseal::{
    AppState, CardState, CatalogEntry, CurrentStaff, DeploymentConfig, Error, Module,
    ModuleRegistrar, NavGroup, Permission, Statement, StatusCard,
};
use harpseal_testing::{Browser, Harness, START_MS, TestApp};

const PASSWORD: &str = "kelp forest at dawn";

const CONFIG: &str = r#"
name = "Seal Bay University (home)"
default_locale = "th"
utc_offset = "+07:00"

[roles]
Administrator = [
    "kernel.student.view", "kernel.roster.view", "kernel.usage.view",
    "kernel.backup.manage", "kernel.staff.manage", "kernel.setting.edit",
    "kernel.audit.view", "announcements.view", "announcements.send", "pages.tool",
]
Viewer = ["kernel.student.view"]
"#;
const QUOTA: &str = "\n[quotas.fake]\nmetered_per_month = 300\n";

/// A Module with a daily page, an Administrator page and two cards.
struct Pages;

const TEXTS: &[CatalogEntry] = &[
    CatalogEntry::new("pages.classes", "th", "รายวิชา"),
    CatalogEntry::new("pages.tool", "th", "เครื่องมือ"),
    CatalogEntry::new("pages.broken", "th", "การ์ดที่อ่านไม่ได้"),
];

async fn tool_card(_: AppState, _: CurrentStaff) -> harpseal::Result<StatusCard> {
    Ok(StatusCard::new(CardState::Warn, "มีงานค้าง 3 รายการ").action("เปิด", "/admin/tool"))
}

async fn broken_card(_: AppState, _: CurrentStaff) -> harpseal::Result<StatusCard> {
    Err(Error::Invalid("no such table".into()))
}

impl Module for Pages {
    fn name(&self) -> &'static str {
        "pages"
    }
    fn register(&self, r: &mut ModuleRegistrar) {
        r.permission(Permission::new("pages.tool", "Use the tool"))
            .messages(TEXTS)
            .back_office_page(
                "kernel.student.view",
                "/classes",
                "pages.classes",
                15,
                get(|| async { "classes" }),
            )
            .back_office_page_in(
                NavGroup::Admin,
                "pages.tool",
                "/tool",
                "pages.tool",
                95,
                get(|| async { "tool" }),
            )
            .status_card("pages.tool", "pages.tool", 50, tool_card)
            .status_card("pages.tool", "pages.broken", 60, broken_card);
    }
}

async fn boot(quota: bool) -> (Harness, TestApp, String) {
    let h = Harness::new();
    let toml = format!("{CONFIG}{}", if quota { QUOTA } else { "" });
    let d = h
        .builder()
        .config(DeploymentConfig::from_toml(&toml).unwrap())
        .module(Announcements)
        .module(Pages)
        .build()
        .unwrap();
    let app = h.boot(d).await;
    let admin = h
        .create_staff("admin@sealbay.example", PASSWORD, &["Administrator"])
        .await;
    h.create_staff("viewer@sealbay.example", PASSWORD, &["Viewer"])
        .await;
    (h, app, admin)
}

async fn as_who<'a>(app: &'a TestApp, who: &str) -> Browser<'a> {
    let b = app.browser();
    b.login(&format!("{who}@sealbay.example"), PASSWORD).await;
    b
}

async fn exec(h: &Harness, sql: &str, args: &[&str]) {
    let mut s = Statement::new(sql.to_string());
    for a in args {
        s = s.bind(*a);
    }
    h.db().execute(s).await.unwrap();
}

/// The `data-state` and the text of the status card titled `title`.
fn card(body: &str, title: &str) -> Option<(String, String)> {
    body.split("<li class=\"status-card\"")
        .skip(1)
        .find_map(|li| {
            let li = li.split("</li>").next()?;
            li.contains(&format!("<h2>{title} ")).then(|| {
                let state = li.split("data-state=\"").nth(1)?.split('"').next()?;
                Some((state.to_string(), li.to_string()))
            })?
        })
}

fn state(body: &str, title: &str) -> String {
    card(body, title).map(|(s, _)| s).unwrap_or_default()
}

#[tokio::test]
async fn the_nav_groups_follow_permissions_and_a_module_page_can_join_admin() {
    let (_h, app, _) = boot(false).await;
    let admin = as_who(&app, "admin").await.get("/admin").await.body;
    let (daily, admin_group) = (
        admin.find("id=\"nav-daily\"").unwrap(),
        admin.find("id=\"nav-admin\"").unwrap(),
    );
    let at = |href: &str| admin.find(&format!("href=\"{href}\"")).unwrap();
    for p in ["/admin/students", "/admin/classes", "/admin/announcements"] {
        assert!(daily < at(p) && at(p) < admin_group, "{p} under งานประจำ");
    }
    for p in [
        "/admin/staff",
        "/admin/roles",
        "/admin/texts",
        "/admin/usage",
        "/admin/backups",
    ] {
        assert!(admin_group < at(p), "{p} under ผู้ดูแลระบบ");
    }
    assert!(admin_group < at("/admin/tool"), "the Module's Admin page");
    assert!(admin.contains("งานประจำ") && admin.contains("ผู้ดูแลระบบ"));

    let viewer = as_who(&app, "viewer").await.get("/admin").await.body;
    assert!(viewer.contains("id=\"nav-daily\""));
    assert!(!viewer.contains("id=\"nav-admin\""), "no empty group");
    assert!(!viewer.contains("href=\"/admin/tool\""));
    assert!(!viewer.contains("href=\"/admin/staff\""));
}

#[tokio::test]
async fn the_current_page_is_marked_and_the_skip_link_leads_to_main() {
    let (_h, app, _) = boot(false).await;
    let b = as_who(&app, "admin").await;
    let current = |body: &str| {
        let n = body.matches("aria-current=\"page\"").count();
        assert_eq!(n, 1, "exactly one current page");
        let before = body.split("aria-current=\"page\"").next().unwrap();
        before
            .rsplit("href=\"")
            .next()
            .unwrap()
            .split('"')
            .next()
            .unwrap()
            .to_string()
    };
    assert_eq!(current(&b.get("/admin").await.body), "/admin");
    let roles = b.get("/admin/roles").await.body;
    assert_eq!(current(&roles), "/admin/roles");
    // A page under an entry marks the entry, not a longer sibling.
    let detail = b.get("/admin/audit/nope").await;
    assert_eq!(detail.status, 404);
    assert_eq!(current(&detail.body), "/admin/audit");
    // The menu button names the current page.
    assert!(roles.contains("<span class=\"menu-here\">บทบาทและสิทธิ์</span>"));

    let body = roles.split("<body").nth(1).unwrap();
    let first_link = body.find("<a ").unwrap();
    assert!(body[first_link..].starts_with("<a class=\"skip\" href=\"#main\">ข้ามไปยังเนื้อหา</a>"));
    assert!(roles.contains("<main id=\"main\" tabindex=\"-1\">"));
    // Collapsible without JS: a <details> holds the navigation.
    assert!(roles.contains("<details class=\"menu\"><summary>"));
}

#[tokio::test]
async fn the_roster_card_reads_empty_ok_then_warn() {
    let (h, app, admin) = boot(false).await;
    let b = as_who(&app, "admin").await;
    let (title, ms) = ("รายชื่อนักศึกษา", START_MS.to_string());
    let home = b.get("/admin").await.body;
    assert_eq!(state(&home, title), "empty");
    assert!(home.contains("ยังไม่ได้นำเข้ารายชื่อ"));
    let insert = "INSERT INTO roster_import (id, staff_id, mode, state, file_key, file_name, \
                  total_rows, created_at, finished_at) VALUES (?, ?, 'full', ?, 'k', ?, 50, ?, ?)";
    exec(&h, insert, &["r1", &admin, "done", "term1.csv", &ms, &ms]).await;
    let home = b.get("/admin").await.body;
    assert_eq!(state(&home, title), "ok");
    assert!(card(&home, title).unwrap().1.contains("50 แถว"));
    let later = (START_MS + 60_000).to_string();
    exec(
        &h,
        insert,
        &["r2", &admin, "failed", "term2.csv", &later, &later],
    )
    .await;
    let (st, li) = card(&b.get("/admin").await.body, title).unwrap();
    assert_eq!(st, "warn");
    assert!(li.contains("term2.csv") && li.contains("href=\"/admin/roster/r2\""));
}

#[tokio::test]
async fn the_quota_card_reads_empty_ok_then_warn() {
    let (h, app, _) = boot(false).await;
    let home = as_who(&app, "admin").await.get("/admin").await.body;
    assert_eq!(state(&home, "โควตาข้อความ"), "empty", "no Quota set");

    let (h2, app2, _) = boot(true).await;
    drop(h);
    let b = as_who(&app2, "admin").await;
    let upsert = "INSERT INTO usage_daily (channel, day, metered) VALUES ('fake', '2026-11-05', ?) \
                  ON CONFLICT (channel, day) DO UPDATE SET metered = excluded.metered";
    exec(&h2, upsert, &["30"]).await;
    let (st, li) = card(&b.get("/admin").await.body, "โควตาข้อความ").unwrap();
    assert_eq!(st, "ok");
    assert!(li.contains("30 จาก 300"), "{li}");
    exec(&h2, upsert, &["250"]).await;
    assert_eq!(state(&b.get("/admin").await.body, "โควตาข้อความ"), "warn");
}

#[tokio::test]
async fn the_backup_card_reads_empty_ok_then_warn() {
    let (h, app, _) = boot(false).await;
    let b = as_who(&app, "admin").await;
    assert_eq!(state(&b.get("/admin").await.body, "สำรองข้อมูล"), "empty");
    let insert = "INSERT INTO backup_run (id, prefix, state, origin, started_at, updated_at, \
                  finished_at) VALUES (?, ?, ?, 'schedule', ?, ?, ?)";
    let ms = START_MS.to_string();
    exec(&h, insert, &["b1", "backups/1/", "done", &ms, &ms, &ms]).await;
    assert_eq!(state(&b.get("/admin").await.body, "สำรองข้อมูล"), "ok");
    let later = (START_MS + 86_400_000).to_string();
    exec(
        &h,
        insert,
        &["b2", "backups/2/", "failed", &later, &later, &later],
    )
    .await;
    assert_eq!(state(&b.get("/admin").await.body, "สำรองข้อมูล"), "warn");
}

#[tokio::test]
async fn the_announcements_card_reads_empty_ok_then_warn_when_overdue() {
    let (h, app, _) = boot(false).await;
    let b = as_who(&app, "admin").await;
    let home = b.get("/admin").await.body;
    assert_eq!(state(&home, "ประกาศ"), "empty");
    assert!(
        card(&home, "ประกาศ")
            .unwrap()
            .1
            .contains("/admin/announcements/new")
    );
    let insert = "INSERT INTO announcement (id, status, sent_at, created_at, updated_at) \
                  VALUES (?, ?, ?, 0, 0)";
    exec(&h, insert, &["a1", "sent", &START_MS.to_string()]).await;
    assert_eq!(state(&b.get("/admin").await.body, "ประกาศ"), "ok");
    // Drafts are a fact, not a warning.
    let draft =
        "INSERT INTO announcement (id, status, created_at, updated_at) VALUES (?, 'draft', 0, 0)";
    exec(&h, draft, &["a2"]).await;
    let (st, li) = card(&b.get("/admin").await.body, "ประกาศ").unwrap();
    assert_eq!(st, "ok");
    assert!(li.contains("ร่างที่ยังไม่ได้ส่ง 1 ฉบับ"), "{li}");
    // A schedule 15 minutes past that has not gone out is a warning.
    let scheduled = "INSERT INTO announcement (id, status, send_at, created_at, updated_at) \
                     VALUES ('a3', 'scheduled', ?, 0, 0)";
    exec(&h, scheduled, &[&(START_MS + 3_600_000).to_string()]).await;
    assert_eq!(
        state(&b.get("/admin").await.body, "ประกาศ"),
        "ok",
        "not yet due"
    );
    h.clock().advance_minutes(60 + 16);
    let (st, li) = card(&b.get("/admin").await.body, "ประกาศ").unwrap();
    assert_eq!(st, "warn");
    assert!(
        li.contains("ยังไม่ได้ส่ง") && li.contains("href=\"/admin/announcements\""),
        "{li}"
    );
}

#[tokio::test]
async fn a_module_card_shows_only_with_its_permission_and_a_failing_one_stays_contained() {
    let (_h, app, _) = boot(false).await;
    let admin = as_who(&app, "admin").await.get("/admin").await.body;
    let (st, li) = card(&admin, "เครื่องมือ").expect("the Module's card");
    assert_eq!(st, "warn");
    assert!(li.contains("มีงานค้าง 3 รายการ") && li.contains("href=\"/admin/tool\""));
    let (st, li) = card(&admin, "การ์ดที่อ่านไม่ได้").expect("the failing card, by its title");
    assert_eq!(st, "warn");
    assert!(li.contains("data-failed") && li.contains("อ่านข้อมูลส่วนนี้ไม่ได้"));
    assert!(!admin.contains("no such table"), "never the error text");
    // Sorted by order: the roster (10) first, the Module's (50) after the backup (40).
    let pos = |t: &str| admin.find(&format!("<h2>{t} ")).unwrap();
    assert!(pos("รายชื่อนักศึกษา") < pos("สำรองข้อมูล"));
    assert!(pos("สำรองข้อมูล") < pos("เครื่องมือ"));

    let viewer = as_who(&app, "viewer").await.get("/admin").await.body;
    assert!(card(&viewer, "เครื่องมือ").is_none());
    assert!(
        !viewer.contains("status-card"),
        "no card the Viewer may see"
    );
    assert!(viewer.contains("ยังไม่มีงานให้ติดตามที่นี่"));
}

#[tokio::test]
async fn the_administration_pages_put_raw_details_behind_a_disclosure() {
    let (_h, app, _) = boot(true).await;
    let b = as_who(&app, "admin").await;
    for p in [
        "/admin/texts",
        "/admin/roles",
        "/admin/backups",
        "/admin/usage",
    ] {
        let body = b.get(p).await.body;
        assert!(
            body.contains("<details class=\"tech\"><summary>ข้อมูลทางเทคนิค</summary>"),
            "{p}"
        );
    }
    // The audit log lists its rows' IPs and request ids only in there.
    let audit = b.get("/admin/audit").await.body;
    let (open, tech) = audit
        .split_once("<details class=\"tech\">")
        .expect("audit details");
    assert!(!open.contains("รหัสคำขอ"), "no request id column");
    assert!(tech.contains("รหัสคำขอ"));
    // Each technical line links to its row, so an IP can be matched to it.
    let rows = tech.split("</ul>").next().unwrap();
    let lines = rows.matches("<li>").count();
    assert!(lines > 0);
    assert_eq!(rows.matches("<li><a href=\"/admin/audit/").count(), lines);
    // A Role's Permissions read as descriptions; ids are technical.
    let roles = b.get("/admin/roles").await.body;
    let (open, tech) = roles.split_once("<details class=\"tech\">").unwrap();
    assert!(!open.contains(">kernel.student.view<"));
    assert!(tech.contains(">kernel.student.view<"));
}

#[tokio::test]
async fn build_refuses_a_card_with_an_undeclared_permission_or_title() {
    struct Bad(&'static str, &'static str);
    impl Module for Bad {
        fn name(&self) -> &'static str {
            "bad"
        }
        fn register(&self, r: &mut ModuleRegistrar) {
            r.messages(&[CatalogEntry::new("bad.title", "th", "การ์ด")])
                .status_card(self.0, self.1, 10, tool_card);
        }
    }
    let h = Harness::new();
    let e = h
        .builder()
        .module(Bad("bad.nobody", "bad.title"))
        .build()
        .err();
    assert!(
        matches!(
            e,
            Some(harpseal::BuildError::UnguardedStatusCard {
                module: "bad",
                permission: "bad.nobody"
            })
        ),
        "{e:?}"
    );
    let e = h
        .builder()
        .module(Bad("kernel.student.view", "bad.missing"))
        .build()
        .err();
    assert!(
        matches!(e, Some(harpseal::BuildError::UnknownText { .. })),
        "{e:?}"
    );
}

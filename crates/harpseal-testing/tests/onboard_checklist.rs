//! The first-run checklist on the home (#127): shown only while the
//! Deployment is fresh (no Student yet); four steps, each ticking itself
//! from data already kept (branding from the TOML or a saved setting, a
//! second Staff account, an announcement sending or sent); only the steps
//! the Staff member may take, and no roster step where `[roster]` maps no
//! file.

use harpseal::modules::announcements::Announcements;
use harpseal::{DeploymentConfig, Statement};
use harpseal_testing::{Browser, Harness, TestApp};

const PASSWORD: &str = "kelp forest at dawn";

const CONFIG: &str = r#"
name = "Seal Bay University (setup)"
default_locale = "th"
utc_offset = "+07:00"

[roles]
Administrator = [
    "kernel.student.view", "kernel.roster.view", "kernel.roster.import",
    "kernel.staff.manage", "kernel.setting.edit",
    "announcements.view", "announcements.send",
]
Sender = ["announcements.view", "announcements.send"]
Viewer = ["kernel.student.view"]

[roster]
student_code = "รหัสนักศึกษา"
display_name = "ชื่อ-สกุล"
email = "อีเมล"
"#;

async fn boot() -> (Harness, TestApp) {
    boot_with(CONFIG).await
}

async fn boot_with(toml: &str) -> (Harness, TestApp) {
    let h = Harness::new();
    let d = h
        .builder()
        .config(DeploymentConfig::from_toml(toml).unwrap())
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

async fn exec(h: &Harness, sql: &str) {
    h.db()
        .execute(Statement::new(sql.to_string()))
        .await
        .unwrap();
}

/// The checklist block, if drawn.
fn block(body: &str) -> Option<String> {
    let at = body.find("id=\"setup\"")?;
    Some(body[at..at + body[at..].find("</section>").unwrap()].to_string())
}

/// Each step's link and whether it is ticked, in order.
fn steps(block: &str) -> Vec<(String, bool)> {
    block
        .split("<li")
        .skip(1)
        .map(|li| {
            let href = li
                .split("href=\"")
                .nth(1)
                .unwrap()
                .split('"')
                .next()
                .unwrap();
            (href.to_string(), li.starts_with(" data-done"))
        })
        .collect()
}

#[tokio::test]
async fn a_fresh_deployment_lists_every_step_undone_with_its_page() {
    let (h, app) = boot().await;
    let b = as_who(&h, &app, "admin", "").await;
    let body = b.get("/admin").await.body;
    let setup = block(&body).expect("the checklist on a fresh home");
    assert!(setup.contains("เริ่มต้นใช้งาน"), "{setup}");
    assert!(setup.contains("เหลืออีก 4 จาก 4 ขั้น"), "{setup}");
    let want = [
        "/admin/branding",
        "/admin/roster",
        "/admin/staff",
        "/admin/announcements/new",
    ];
    let got = steps(&setup);
    assert_eq!(
        got,
        want.iter()
            .map(|h| (h.to_string(), false))
            .collect::<Vec<_>>()
    );
    // Before the status ledger: the first thing on the page.
    assert!(body.find("id=\"setup\"") < body.find("class=\"card term\""));
}

const STUDENT: &str = "INSERT INTO student (id, student_code, display_name, locale, created_at, updated_at) \
     VALUES ('s1', '68010001', 'นกน้อย ทะเลใส', 'th', 0, 0)";

#[tokio::test]
async fn steps_tick_themselves_from_data_while_the_deployment_is_fresh() {
    let (h, app) = boot().await;
    let b = as_who(&h, &app, "admin", "").await;
    let ticked = |body: &str| -> Vec<bool> {
        steps(&block(body).expect("fresh: the checklist"))
            .into_iter()
            .map(|(_, d)| d)
            .collect()
    };
    h.create_staff("officer@sealbay.example", PASSWORD, &["Viewer"])
        .await;
    assert_eq!(
        ticked(&b.get("/admin").await.body),
        [false, false, true, false]
    );
    // Neither a draft nor a scheduled one is the first announcement.
    exec(
        &h,
        "INSERT INTO announcement (id, status, created_by, created_at, updated_at) \
         VALUES ('a1', 'draft', (SELECT id FROM staff LIMIT 1), 0, 0)",
    )
    .await;
    assert!(!ticked(&b.get("/admin").await.body)[3], "a draft");
    exec(
        &h,
        "UPDATE announcement SET status = 'scheduled' WHERE id = 'a1'",
    )
    .await;
    assert!(
        !ticked(&b.get("/admin").await.body)[3],
        "scheduled, not sent"
    );
    exec(
        &h,
        "UPDATE announcement SET status = 'sent' WHERE id = 'a1'",
    )
    .await;
    // Branding saved on /admin/branding.
    exec(
        &h,
        "INSERT INTO setting (key, locale, value, updated_at) \
         VALUES ('kernel.branding.primary_color', '', '#0f5c7a', 0)",
    )
    .await;
    let body = b.get("/admin").await.body;
    assert_eq!(ticked(&body), [true, false, true, true]);
    assert!(block(&body).unwrap().contains("เหลืออีก 1 จาก 4 ขั้น"));
    // The first Student: established, the block goes; the ledger stays.
    exec(&h, STUDENT).await;
    let body = b.get("/admin").await.body;
    assert_eq!(block(&body), None);
    assert!(body.contains("class=\"card term\""), "the ledger stays");
}

#[tokio::test]
async fn an_established_deployment_with_toml_branding_sees_no_checklist() {
    let toml = format!("{CONFIG}\n[branding]\nname = \"มหาวิทยาลัยซีลเบย์\"\n");
    let (h, app) = boot_with(&toml).await;
    let b = as_who(&h, &app, "admin", "").await;
    // Still fresh: the TOML's branding counts as done.
    let setup = block(&b.get("/admin").await.body).expect("fresh");
    assert!(steps(&setup)[0].1, "branding from [branding]: {setup}");
    exec(&h, STUDENT).await;
    assert_eq!(block(&b.get("/admin").await.body), None);
}

#[tokio::test]
async fn without_a_roster_mapping_there_is_no_roster_step() {
    let toml = CONFIG.split("[roster]").next().unwrap();
    let (h, app) = boot_with(toml).await;
    let b = as_who(&h, &app, "admin", "").await;
    let setup = block(&b.get("/admin").await.body).expect("fresh");
    let got: Vec<String> = steps(&setup).into_iter().map(|(h, _)| h).collect();
    assert_eq!(
        got,
        [
            "/admin/branding",
            "/admin/staff",
            "/admin/announcements/new"
        ]
    );
}

#[tokio::test]
async fn staff_see_only_the_steps_their_permissions_allow() {
    let (h, app) = boot().await;
    let sender = as_who(&h, &app, "sender", "Sender").await;
    let setup = block(&sender.get("/admin").await.body).expect("the send step");
    let got: Vec<String> = steps(&setup).into_iter().map(|(h, _)| h).collect();
    assert_eq!(got, ["/admin/announcements/new"]);
    assert!(setup.contains("เหลืออีก 1 จาก 1 ขั้น"), "{setup}");
    // No step allowed: no block at all.
    let viewer = as_who(&h, &app, "viewer", "Viewer").await;
    assert_eq!(block(&viewer.get("/admin").await.body), None);
}

#[tokio::test]
async fn without_the_announcements_module_its_step_is_not_drawn() {
    let h = Harness::new();
    // The same Roles without the Module's Permissions.
    let toml = CONFIG.replace("\n    \"announcements.view\", \"announcements.send\",", "");
    let toml = toml.replace(
        "Sender = [\"announcements.view\", \"announcements.send\"]\n",
        "",
    );
    let d = h
        .builder()
        .config(DeploymentConfig::from_toml(&toml).unwrap())
        .build()
        .unwrap();
    let app = h.boot(d).await;
    h.create_staff("admin@sealbay.example", PASSWORD, &["Administrator"])
        .await;
    let b = as_who(&h, &app, "admin", "").await;
    let setup = block(&b.get("/admin").await.body).expect("three steps");
    let got: Vec<String> = steps(&setup).into_iter().map(|(h, _)| h).collect();
    assert_eq!(got, ["/admin/branding", "/admin/roster", "/admin/staff"]);
}

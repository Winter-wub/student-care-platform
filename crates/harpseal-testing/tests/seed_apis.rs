//! Seed and CLI APIs (#164): a roster attribute's values with their
//! counts, and the display name, primary colour and LINE package saved
//! from code, by a Staff member holding `kernel.setting.edit` or by the
//! system, with the pages' checks and audit rows.

use harpseal::branding;
use harpseal::roster::{self, ValuesScope};
use harpseal::usage::{self, LinePackage};
use harpseal::{Actor, DeploymentConfig, Error, RequestCtx, Statement};
use harpseal_testing::{Harness, TestApp};

const CONFIG: &str = r##"
name = "Seal Bay University (seed APIs)"
default_locale = "th"
utc_offset = "+07:00"

[branding]
primary_color = "#0b5cad"

[roles]
Administrator = ["kernel.setting.edit"]
Viewer = ["kernel.student.view"]

[roster]
student_code = "รหัสนักศึกษา"
display_name = "ชื่อ-สกุล"

[roster.attributes.year]
column = "ชั้นปี"
type = "integer"
student_visible = true

[roster.attributes.programme]
column = "หลักสูตร"
type = "enum"
values = ["MSC", "MSC-Intl"]
student_visible = true

# Visible, but too fine to count: one Student's GPA, nickname or birthday.
[roster.attributes.gpa]
column = "เกรดเฉลี่ย"
type = "number"
student_visible = true

[roster.attributes.nickname]
column = "ชื่อเล่น"
type = "text"
student_visible = true

[roster.attributes.born]
column = "วันเกิด"
type = "date"
student_visible = true

# Coarse, but Staff-only.
[roster.attributes.scholarship]
column = "ทุน"
type = "enum"
values = ["full", "none"]

[quotas.line]
metered_per_month = 300
"##;

async fn boot(line: bool) -> (Harness, TestApp, Actor, Actor) {
    let h = Harness::new();
    let mut b = h
        .builder()
        .config(DeploymentConfig::from_toml(CONFIG).unwrap());
    if line {
        b = b.channel(h.fake_channel().with_kind("line"));
    }
    let app = h.boot(b.build().unwrap()).await;
    let pw = "kelp forest at dawn";
    let admin = h
        .create_staff("admin@sealbay.example", pw, &["Administrator"])
        .await;
    let viewer = h
        .create_staff("viewer@sealbay.example", pw, &["Viewer"])
        .await;
    (h, app, Actor::Staff(admin), Actor::Staff(viewer))
}

async fn student(h: &Harness, code: &str, status: &str, attrs: &str) {
    let st = Statement::new(
        "INSERT INTO student (id, student_code, display_name, locale, status, attributes, \
         created_at, updated_at) VALUES (?, ?, 'นักศึกษาสาธิต', 'th', ?, ?, 0, 0)",
    )
    .bind(format!("s-{code}"))
    .bind(code)
    .bind(status)
    .bind(attrs);
    h.db().execute(st).await.unwrap();
}

fn pairs(v: &[roster::AttributeValue]) -> Vec<(&str, u64)> {
    v.iter().map(|a| (a.value.as_str(), a.students)).collect()
}

/// The non-Staff actors every setter refuses.
fn outsiders() -> [Actor; 3] {
    [
        Actor::Student("s-1".into()),
        Actor::Job("job-1".into()),
        Actor::System,
    ]
}

#[tokio::test]
async fn attribute_values_count_active_students_by_value() {
    let (h, app, _, _) = boot(false).await;
    let d = app.deployment();
    for (code, status, attrs) in [
        ("68010001", "active", r#"{"year":1,"programme":"MSC"}"#),
        ("68010002", "active", r#"{"year":2,"programme":"MSC"}"#),
        (
            "68010003",
            "active",
            r#"{"year":10,"programme":"MSC-Intl"}"#,
        ),
        ("68010004", "active", r#"{"year":2,"programme":"MSC-Intl"}"#),
        ("68010005", "left", r#"{"year":2,"programme":"MSC"}"#),
        ("68010006", "active", r#"{"programme":"MSC"}"#),
        ("68010007", "active", r#"{"year":null}"#),
    ] {
        student(&h, code, status, attrs).await;
    }
    let years = roster::attribute_values(d, "year", &ValuesScope::active())
        .await
        .unwrap();
    assert_eq!(
        pairs(&years),
        [("1", 1), ("2", 2), ("10", 1)],
        "numeric order"
    );
    let mut all = ValuesScope::active();
    all.include_left = true;
    let years = roster::attribute_values(d, "year", &all).await.unwrap();
    assert_eq!(pairs(&years), [("1", 1), ("2", 3), ("10", 1)]);
    let programmes = roster::attribute_values(d, "programme", &ValuesScope::active())
        .await
        .unwrap();
    assert_eq!(pairs(&programmes), [("MSC", 3), ("MSC-Intl", 2)]);
    let scoped = ValuesScope::active().with("programme", &["MSC"]);
    let years = roster::attribute_values(d, "year", &scoped).await.unwrap();
    assert_eq!(pairs(&years), [("1", 1), ("2", 1)]);

    for (key, scope) in [
        ("faculty", ValuesScope::active()),
        ("year", ValuesScope::active().with("faculty", &["x"])),
        ("year", ValuesScope::active().with("programme", &[" "])),
    ] {
        let r = roster::attribute_values(d, key, &scope).await;
        assert!(matches!(r, Err(Error::Invalid(_))), "{key}: {r:?}");
    }
}

#[tokio::test]
async fn fine_or_staff_only_attributes_are_never_counted() {
    let (h, app, _, _) = boot(false).await;
    let d = app.deployment();
    student(
        &h,
        "68010001",
        "active",
        r#"{"year":3,"programme":"MSC","gpa":3.97,"nickname":"นกน้อย","born":"2004-05-06","scholarship":"full"}"#,
    )
    .await;
    let refused = |r: harpseal::Result<Vec<roster::AttributeValue>>, why: &str| match r {
        Err(Error::Invalid(m)) => assert!(m.contains(why), "{why}: {m}"),
        other => panic!("expected a refusal ({why}), got {other:?}"),
    };
    for key in ["gpa", "nickname", "born"] {
        refused(
            roster::attribute_values(d, key, &ValuesScope::active()).await,
            "only `enum` and `integer`",
        );
        // Nor as a scope: a count by GPA singles one Student out too.
        let scope = ValuesScope::active().with(key, &["3.97"]);
        refused(
            roster::attribute_values(d, "year", &scope).await,
            "only `enum` and `integer`",
        );
    }
    refused(
        roster::attribute_values(d, "scholarship", &ValuesScope::active()).await,
        "Staff-only",
    );
    let scope = ValuesScope::active().with("scholarship", &["full"]);
    refused(
        roster::attribute_values(d, "programme", &scope).await,
        "Staff-only",
    );
}

/// (actor kind, after) of each `action` row, Staff before system (the
/// test clock does not move, so `at` cannot order them).
async fn audits(app: &TestApp, action: &'static str) -> Vec<(String, String)> {
    let sql = match action {
        "branding.update" => {
            "SELECT actor_kind, after FROM audit_log WHERE action = 'branding.update' ORDER BY actor_kind"
        }
        _ => {
            "SELECT actor_kind, after FROM audit_log WHERE action = 'setting.update' ORDER BY actor_kind"
        }
    };
    let rows = app.query(sql).await;
    (0..rows.len())
        .map(|i| {
            let t = |c: &str| {
                rows.get(i, c)
                    .and_then(|v| v.as_text())
                    .unwrap_or("")
                    .to_string()
            };
            (t("actor_kind"), t("after"))
        })
        .collect()
}

#[tokio::test]
async fn branding_is_saved_by_an_editor_or_the_system_with_the_pages_checks() {
    let (_h, app, admin, viewer) = boot(false).await;
    let d = app.deployment();
    let ctx = RequestCtx::cli();
    let unset = branding::saved(d).await.unwrap();
    assert_eq!((unset.name, unset.primary_color), (None, None));

    for actor in outsiders().iter().chain([&viewer]) {
        let r = branding::set_name(d, "มหาวิทยาลัยอ่าวแมวน้ำ", actor, &ctx).await;
        assert!(
            matches!(r, Err(Error::Forbidden { .. })),
            "{actor:?}: {r:?}"
        );
        let r = branding::set_primary_color(d, "#123456", actor, &ctx).await;
        assert!(
            matches!(r, Err(Error::Forbidden { .. })),
            "{actor:?}: {r:?}"
        );
    }
    for bad in ["yellow", "#12345"] {
        let r = branding::set_primary_color(d, bad, &admin, &ctx).await;
        assert!(
            matches!(&r, Err(Error::Invalid(m)) if m.contains("kernel.branding.err_")),
            "{r:?}"
        );
    }
    let r = branding::set_primary_color(d, "#ffff00", &admin, &ctx).await;
    assert!(
        matches!(&r, Err(Error::Invalid(m)) if m.contains("err_contrast")),
        "{r:?}"
    );
    let r = branding::set_name(d, "  ", &admin, &ctx).await;
    assert!(
        matches!(&r, Err(Error::Invalid(m)) if m.contains("err_name_empty")),
        "{r:?}"
    );
    assert!(audits(&app, "branding.update").await.is_empty());

    assert!(
        branding::set_name(d, " มหาวิทยาลัยอ่าวแมวน้ำ ", &admin, &ctx)
            .await
            .unwrap()
    );
    assert!(
        !branding::set_name(d, "มหาวิทยาลัยอ่าวแมวน้ำ", &admin, &ctx)
            .await
            .unwrap()
    );
    assert!(
        branding::set_primary_color_as_system(d, "#1A4D7A", &ctx)
            .await
            .unwrap()
    );
    let saved = branding::saved(d).await.unwrap();
    assert_eq!(saved.name.as_deref(), Some("มหาวิทยาลัยอ่าวแมวน้ำ"));
    assert_eq!(
        saved.primary_color.as_deref(),
        Some("#1a4d7a"),
        "kept with the name"
    );
    let rows = audits(&app, "branding.update").await;
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].0, "staff");
    assert_eq!(rows[1].0, "system");
    assert!(
        rows[1].1.contains("มหาวิทยาลัยอ่าวแมวน้ำ"),
        "the name is kept: {}",
        rows[1].1
    );

    // The TOML's colour is no setting: it follows the file again.
    assert!(
        branding::set_primary_color_as_system(d, "#0B5CAD", &ctx)
            .await
            .unwrap()
    );
    assert_eq!(branding::saved(d).await.unwrap().primary_color, None);
    assert!(
        branding::set_name_as_system(d, "Seal Bay", &ctx)
            .await
            .unwrap()
    );
    let html = app.get("/admin/login").await.body;
    assert!(html.contains("Seal Bay"), "the chrome follows the save");
}

#[tokio::test]
async fn the_line_package_is_saved_by_an_editor_or_the_system() {
    let (_h, app, admin, viewer) = boot(true).await;
    let d = app.deployment();
    let ctx = RequestCtx::cli();
    assert_eq!(
        usage::line_package(d).await.unwrap(),
        LinePackage::FollowConfig
    );
    for actor in outsiders().iter().chain([&viewer]) {
        let r = usage::set_line_package(d, LinePackage::Basic, actor, &ctx).await;
        assert!(
            matches!(r, Err(Error::Forbidden { .. })),
            "{actor:?}: {r:?}"
        );
    }
    for bad in [0, 100_000_001] {
        let r = usage::set_line_package(d, LinePackage::Custom(bad), &admin, &ctx).await;
        assert!(matches!(r, Err(Error::Invalid(_))), "{bad}: {r:?}");
    }
    // `[quotas.line]` 300 is in force: choosing it as Custom changes nothing.
    assert!(
        !usage::set_line_package(d, LinePackage::Custom(300), &admin, &ctx)
            .await
            .unwrap()
    );
    assert!(audits(&app, "setting.update").await.is_empty());

    assert!(
        usage::set_line_package(d, LinePackage::Basic, &admin, &ctx)
            .await
            .unwrap()
    );
    assert_eq!(usage::line_package(d).await.unwrap(), LinePackage::Basic);
    assert!(
        usage::set_line_package_as_system(d, LinePackage::Custom(5000), &ctx)
            .await
            .unwrap()
    );
    assert_eq!(
        usage::line_package(d).await.unwrap(),
        LinePackage::Custom(5000)
    );
    let rows = audits(&app, "setting.update").await;
    assert_eq!(rows.len(), 2, "{rows:?}");
    assert_eq!(rows[0].0, "staff");
    assert!(rows[0].1.contains(r#""value":"basic""#), "{rows:?}");
    assert_eq!(rows[1].0, "system");
    assert!(rows[1].1.contains(r#""value":"custom:5000""#), "{rows:?}");
    assert!(
        usage::set_line_package_as_system(d, LinePackage::FollowConfig, &ctx)
            .await
            .unwrap()
    );
    assert_eq!(
        usage::line_package(d).await.unwrap(),
        LinePackage::FollowConfig
    );
    assert_eq!(
        app.count("SELECT count(*) FROM audit_log WHERE action = 'setting.reset'")
            .await,
        1
    );
}

#[tokio::test]
async fn the_line_package_needs_a_line_channel() {
    let (_h, app, _, _) = boot(false).await;
    let r =
        usage::set_line_package_as_system(app.deployment(), LinePackage::Pro, &RequestCtx::cli())
            .await;
    assert!(
        matches!(&r, Err(Error::Invalid(m)) if m.contains("err_no_line")),
        "{r:?}"
    );
}

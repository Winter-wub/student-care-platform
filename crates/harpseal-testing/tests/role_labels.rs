//! A Deployment labels its Roles and Locales in Thai through `[texts]`
//! (#125): `kernel.role.<stored name>` for a Role of `[roles]` (or the
//! CLI's Administrator) and `kernel.locale.<tag>` for one of its Locales.
//! The pages show the label; the stored name never changes. A label for a
//! Role or Locale the Deployment does not have is a typo, refused.

mod admin_support;
mod support;

use admin_support::signed_in;
use harpseal::{BuildError, DeploymentConfig};
use harpseal_testing::Harness;

const TOML: &str = r#"
name = "Seal Bay University (labels)"
default_locale = "th"
utc_offset = "+07:00"

[roles]
Administrator = ["kernel.student.view", "kernel.staff.manage"]
"General Staff" = ["kernel.student.view"]

[texts.th]
"kernel.role.General Staff" = "เจ้าหน้าที่ทั่วไป"
"kernel.locale.th" = "ภาษาไทย"
"#;

fn refused(toml: &str) -> BuildError {
    let h = Harness::new();
    let config = DeploymentConfig::from_toml(toml).unwrap();
    match h.builder().config(config).build() {
        Err(e) => e,
        Ok(_) => panic!("built"),
    }
}

#[tokio::test]
async fn a_role_label_shows_and_the_stored_name_stays() {
    let h = Harness::new();
    let config = DeploymentConfig::from_toml(TOML).unwrap();
    let app = h.boot(h.builder().config(config).build().unwrap()).await;
    let (admin, _) = signed_in(&h, &app, "Administrator").await;
    h.create_staff(
        "clerk@sealbay.example",
        "kelp forest at dawn",
        &["General Staff"],
    )
    .await;
    let list = admin.get("/admin/staff").await.body;
    assert!(
        list.contains("<span class=\"chip\">เจ้าหน้าที่ทั่วไป</span>"),
        "{list}"
    );
    assert!(
        list.contains("<span class=\"chip\">ผู้ดูแลระบบ</span>"),
        "{list}"
    );
    assert!(!list.contains(">General Staff<") && !list.contains(">Administrator<"));
    let new = admin.get("/admin/staff/new").await.body;
    assert!(
        new.contains("value=\"General Staff\""),
        "posts the stored name"
    );
    assert!(new.contains(">ภาษาไทย</option>"), "{new}");
    let names = h
        .db()
        .query(harpseal::Statement::new(
            "SELECT name FROM role ORDER BY name",
        ))
        .await
        .unwrap();
    let names: Vec<&str> = (0..names.rows.len())
        .filter_map(|i| names.get(i, "name").and_then(|v| v.as_text()))
        .collect();
    assert_eq!(names, ["Administrator", "General Staff"]);
}

#[test]
fn a_label_for_a_role_or_locale_the_deployment_lacks_is_refused() {
    let base =
        "name = \"X\"\ndefault_locale = \"th\"\n[roles]\nOfficer = [\"kernel.student.view\"]\n";
    for (key, text) in [("kernel.role.Oficer", "ผิด"), ("kernel.locale.ja", "ญี่ปุ่น")]
    {
        let toml = format!("{base}[texts.th]\n\"{key}\" = \"{text}\"\n");
        assert_eq!(
            refused(&toml),
            BuildError::UnknownText {
                source: "deployment.toml [texts.th]".into(),
                key: key.into(),
            }
        );
    }
}

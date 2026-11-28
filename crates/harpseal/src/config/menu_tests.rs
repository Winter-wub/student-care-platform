use serde_json::{Map, Value as Json, json};

use crate::DeploymentConfig;

const STATUS: &str = "[roster.attributes.status]\ncolumn = \"สถานะ\"\ntype = \"enum\"\n\
    values = [\"current\", \"graduated\", \"on_leave\"]\n\
    [roster.attributes.note]\ncolumn = \"หมายเหตุ\"\ntype = \"text\"\noptional = true\n";

fn config(menu: &str) -> crate::Result<DeploymentConfig> {
    DeploymentConfig::from_toml(&format!(
        "name = \"X\"\ndefault_locale = \"th\"\n{STATUS}{menu}"
    ))
}

fn attrs(v: Json) -> Map<String, Json> {
    v.as_object().cloned().unwrap_or_default()
}

const GRAD: &str = "[[menu.variants]]\nkey = \"grad\"\nwhen = { status = [\"graduated\"] }\n\
    entries = [\"announcements.latest\"]\n";

#[test]
fn the_first_matching_variant_is_the_students() {
    let c = config(&format!(
        "{GRAD}[[menu.variants]]\nkey = \"away\"\nwhen = {{ status = [\"graduated\", \"on_leave\"] }}\n\
         app_links = [\"/app/fees\"]\n"
    ))
    .unwrap();
    let pick = |v: Json| c.menu.variant_for(&attrs(v)).map(|v| v.key.as_str());
    assert_eq!(pick(json!({ "status": "graduated" })), Some("grad"));
    assert_eq!(pick(json!({ "status": "on_leave" })), Some("away"));
    assert_eq!(pick(json!({ "status": "current" })), None);
    assert_eq!(pick(json!({})), None, "no status: the whole Menu");
    assert_eq!(pick(json!({ "status": 1 })), None);
    let grad = c.menu.variant("grad").unwrap();
    assert!(grad.keeps_entry("announcements.latest") && !grad.keeps_entry("x.y"));
    assert!(
        !grad.keeps_app_link("/app/fees"),
        "app_links default to none"
    );
    assert!(c.menu.variant("away").unwrap().keeps_app_link("/app/fees"));
    assert_eq!(
        c.menu.attributes().into_iter().collect::<Vec<_>>(),
        ["status"]
    );
}

#[test]
fn no_variants_by_default() {
    let c = config("").unwrap();
    assert!(c.menu.variants.is_empty());
    assert!(
        c.menu
            .variant_for(&attrs(json!({ "status": "graduated" })))
            .is_none()
    );
}

#[test]
fn bad_variants_are_refused() {
    let cases = [
        (
            "key = \"Grad\"\nwhen = { status = [\"graduated\"] }",
            "key: 1 to 8",
        ),
        (
            "key = \"\"\nwhen = { status = [\"graduated\"] }",
            "key: 1 to 8",
        ),
        (
            "key = \"graduates\"\nwhen = { status = [\"graduated\"] }",
            "key: 1 to 8",
        ),
        ("key = \"g\"\nwhen = {}", "names one attribute"),
        (
            "key = \"g\"\nwhen = { nope = [\"x\"] }",
            "no attribute `nope`",
        ),
        ("key = \"g\"\nwhen = { note = [\"x\"] }", "must be an enum"),
        ("key = \"g\"\nwhen = { status = [] }", "at least one value"),
        (
            "key = \"g\"\nwhen = { status = [\"left\"] }",
            "no value `left`",
        ),
        (
            "key = \"g\"\nwhen = { status = [\"current\", \"current\"] }",
            "listed twice",
        ),
        (
            "key = \"g\"\nwhen = { status = [\"current\"] }\nextra = 1",
            "unknown field",
        ),
    ];
    for (body, want) in cases {
        let e = config(&format!("[[menu.variants]]\n{body}\n")).unwrap_err();
        assert!(e.to_string().contains(want), "{body}: {e}");
    }
    let twice = config(&format!("{GRAD}{GRAD}")).unwrap_err();
    assert!(twice.to_string().contains("used twice"), "{twice}");
}

#[test]
fn the_sql_condition_names_the_attribute_and_its_values() {
    let c = config(GRAD).unwrap();
    let (sql, binds) = c.menu.variants[0].sql("s.attributes");
    assert_eq!(sql, "(COALESCE(json_extract(s.attributes, ?), '') IN (?))");
    assert_eq!(binds, ["$.\"status\"", "graduated"]);
}

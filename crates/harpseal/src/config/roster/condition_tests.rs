use serde_json::{Map, Value as Json, json};

use crate::DeploymentConfig;
use crate::message::Locale;

const STATUS: &str = "[roster.attributes.status]\ncolumn = \"สถานะ\"\ntype = \"enum\"\n\
    values = [\"current\", \"graduated\", \"on_leave\"]\n\
    value_labels = { graduated = { th = \"ศิษย์เก่า\" } }\nlabel = { th = \"สถานะ\" }\n";

fn config(attrs: &str) -> crate::Result<DeploymentConfig> {
    DeploymentConfig::from_toml(&format!(
        "name = \"X\"\ndefault_locale = \"th\"\n{STATUS}{attrs}"
    ))
}

const YEAR: &str = "[roster.attributes.year]\ncolumn = \"ชั้นปี\"\ntype = \"integer\"\n\
    optional_when = { status = [\"graduated\"] }\n";
const COHORT: &str = "[roster.attributes.cohort]\ncolumn = \"รุ่น\"\ntype = \"integer\"\n\
    optional = true\nrequired_when = { status = [\"graduated\"] }\nlabel = { th = \"รุ่น\" }\n";

fn attrs(v: Json) -> Map<String, Json> {
    v.as_object().cloned().unwrap_or_default()
}

#[test]
fn optional_when_and_required_when_follow_the_other_value() {
    let c = config(&format!("{YEAR}{COHORT}")).unwrap();
    let (year, cohort) = (&c.roster.attributes["year"], &c.roster.attributes["cohort"]);
    let graduate = attrs(json!({ "status": "graduated" }));
    let current = attrs(json!({ "status": "current" }));
    let none = Map::new();
    assert!(!year.required_for(&graduate));
    assert!(year.required_for(&current));
    assert!(year.required_for(&none), "no status: the year is required");
    assert!(cohort.required_for(&graduate));
    assert!(!cohort.required_for(&current));
    assert!(!cohort.required_for(&none));
    // A blank cell parses for both; the row decides.
    assert_eq!(year.parse(" "), Ok(None));
    assert!(year.may_be_empty() && cohort.may_be_empty());
    assert_eq!(
        year.condition(),
        Some(("status", &["graduated".to_string()][..]))
    );
    let th = Locale::th();
    assert_eq!(
        year.condition_text(&c.roster, &th, &th).as_deref(),
        Some("สถานะ: ศิษย์เก่า")
    );
}

#[test]
fn a_bad_condition_stops_the_build() {
    let year = |cond: &str| {
        config(&format!(
            "[roster.attributes.year]\ncolumn = \"ชั้นปี\"\ntype = \"integer\"\n{cond}\n\
             [roster.attributes.note]\ncolumn = \"หมายเหตุ\"\ntype = \"text\"\noptional = true\n"
        ))
    };
    assert!(year("optional_when = { status = [\"graduated\", \"on_leave\"] }").is_ok());
    for bad in [
        // An unknown attribute, not an enum, itself, an unknown value.
        "optional_when = { nope = [\"x\"] }",
        "optional_when = { note = [\"x\"] }",
        "optional_when = { year = [\"1\"] }",
        "optional_when = { status = [\"left\"] }",
        // No value, a value twice, two attributes.
        "optional_when = { status = [] }",
        "optional_when = { status = [\"graduated\", \"graduated\"] }",
        "optional_when = { status = [\"graduated\"], note = [\"x\"] }",
        // optional_when on an optional one; required_when on a required one.
        "optional = true\noptional_when = { status = [\"graduated\"] }",
        "required_when = { status = [\"graduated\"] }",
        // Both at once.
        "optional = true\noptional_when = { status = [\"graduated\"] }\nrequired_when = { status = [\"current\"] }",
    ] {
        assert!(year(bad).is_err(), "accepted: {bad}");
    }
    // A condition on an attribute that has one of its own.
    assert!(
        config(&format!(
            "{YEAR}[roster.attributes.kind]\ncolumn = \"k\"\ntype = \"enum\"\nvalues = [\"a\"]\n\
             optional_when = {{ status = [\"graduated\"] }}\n\
             [roster.attributes.x]\ncolumn = \"x\"\ntype = \"text\"\noptional_when = {{ kind = [\"a\"] }}\n"
        ))
        .is_err()
    );
}

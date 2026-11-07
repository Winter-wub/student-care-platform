use super::*;

fn spec(kind: AttributeType, values: &[&str], optional: bool) -> AttributeSpec {
    AttributeSpec {
        column: "c".into(),
        kind,
        values: values.iter().map(|v| v.to_string()).collect(),
        label: BTreeMap::new(),
        optional,
        shown_in_list: false,
        student_visible: false,
        order: 0,
    }
}

#[test]
fn values_are_parsed_by_type() {
    let int = spec(AttributeType::Integer, &[], false);
    assert_eq!(int.parse(" 3 "), Ok(Some(Json::from(3))));
    assert_eq!(int.parse("3.5"), Err(AttrError::NotInteger));
    assert_eq!(int.parse(""), Err(AttrError::Required));
    let num = spec(AttributeType::Number, &[], true);
    assert_eq!(num.parse("3.5"), Ok(Some(Json::from(3.5))));
    assert_eq!(num.parse("x"), Err(AttrError::NotNumber));
    assert_eq!(num.parse("NaN"), Err(AttrError::NotNumber));
    assert_eq!(num.parse(""), Ok(None));
    // #70: numbers stored as text, the same reader as a Module's.
    assert_eq!(int.parse("1,000"), Ok(Some(Json::from(1000))));
    assert_eq!(int.parse("12,500.00"), Ok(Some(Json::from(12500))));
    assert_eq!(int.parse("12.500"), Err(AttrError::NotInteger));
    assert_eq!(int.parse("-"), Err(AttrError::Required));
    assert_eq!(num.parse(" 12,500.50 "), Ok(Some(Json::from(12500.5))));
    assert_eq!(num.parse("-"), Ok(None), "a lone dash is no value");
    assert_eq!(num.parse("-2.5"), Ok(Some(Json::from(-2.5))));
    assert_eq!(num.parse("12,5"), Err(AttrError::NotNumber));
    // Integers are exact (never through f64), as before #70.
    assert_eq!(
        int.parse("9007199254740993"),
        Ok(Some(Json::from(9_007_199_254_740_993_i64)))
    );
    assert_eq!(int.parse("3.00000000000000001"), Err(AttrError::NotInteger));
    assert_eq!(
        int.parse("99999999999999999999"),
        Err(AttrError::NotInteger)
    );
    // Forms the old parsers accepted stay accepted…
    assert_eq!(int.parse("+5"), Ok(Some(Json::from(5))));
    assert_eq!(num.parse("+5"), Ok(Some(Json::from(5.0))));
    assert_eq!(num.parse(".5"), Ok(Some(Json::from(0.5))));
    assert_eq!(num.parse("5."), Ok(Some(Json::from(5.0))));
    // …except exponents and more digits than an f64 holds (declared).
    assert_eq!(num.parse("1e3"), Err(AttrError::NotNumber));
    assert_eq!(num.parse("1234567890123456"), Err(AttrError::NotNumber));
    let en = spec(AttributeType::Enum, &["MSC", "MSC-Intl"], false);
    assert_eq!(en.parse("MSC-Intl"), Ok(Some(Json::from("MSC-Intl"))));
    assert_eq!(en.parse("msc"), Err(AttrError::NotInList));
    let date = spec(AttributeType::Date, &[], false);
    assert_eq!(date.parse("2028-02-29"), Ok(Some(Json::from("2028-02-29"))));
    for bad in [
        "2027-02-29",
        "2026-13-01",
        "2026-1-01",
        "26-01-01x",
        "2026/01/01",
    ] {
        assert_eq!(date.parse(bad), Err(AttrError::NotDate), "{bad}");
    }
}

#[test]
fn schema_errors_are_refused() {
    let base = "name = \"X\"\ndefault_locale = \"th\"\n";
    let ok = "[roster.attributes.year]\ncolumn = \"ชั้นปี\"\ntype = \"integer\"\n\
              label = { th = \"ชั้นปี\", en = \"Year\" }";
    let c = crate::DeploymentConfig::from_toml(&format!("{base}{ok}")).unwrap();
    let year = &c.roster.attributes["year"];
    assert_eq!(year.label_in("year", &Locale::en(), &Locale::th()), "Year");
    assert_eq!(
        year.label_in("year", &Locale("fr".into()), &Locale::th()),
        "ชั้นปี"
    );
    for extra in [
        "[roster.attributes.Year]\ncolumn = \"a\"\ntype = \"text\"",
        "[roster.attributes.p]\ncolumn = \"a\"\ntype = \"enum\"",
        "[roster.attributes.p]\ncolumn = \"a\"\ntype = \"text\"\nvalues = [\"x\"]",
        "[roster.attributes.p]\ncolumn = \"a\"\ntype = \"enum\"\nvalues = [\"x\", \"x\"]",
        "[roster.attributes.p]\ncolumn = \"email\"\ntype = \"text\"",
        "[roster.attributes.p]\ncolumn = \"a\"\ntype = \"bool\"",
        "[roster]\ntags = { column = \"g\", separator = \"\" }",
    ] {
        assert!(
            crate::DeploymentConfig::from_toml(&format!("{base}{extra}")).is_err(),
            "accepted: {extra}"
        );
    }
}

#[test]
fn modes_default_to_both_with_full_preselected() {
    let c = crate::DeploymentConfig::from_toml("name = \"X\"\ndefault_locale = \"th\"\n").unwrap();
    assert_eq!(c.roster.modes, ["full", "upsert"]);
    assert_eq!(c.roster.default_mode, "full");
    assert!(c.roster.allows_mode("full") && c.roster.allows_mode("upsert"));
    assert!(!c.roster.allows_mode("") && !c.roster.allows_mode("replace"));
}

#[test]
fn modes_are_chosen_and_checked() {
    let base = "name = \"X\"\ndefault_locale = \"th\"\n[roster]\n";
    let ok = |extra: &str| crate::DeploymentConfig::from_toml(&format!("{base}{extra}"));
    let c = ok("modes = [\"upsert\"]\ndefault_mode = \"upsert\"").unwrap();
    assert!(c.roster.allows_mode("upsert") && !c.roster.allows_mode("full"));
    assert!(c.roster.is_mapped());
    // The default is `full`, so an upsert-only list must name its default.
    for bad in [
        "modes = [\"upsert\"]",
        "modes = []",
        "modes = []\ndefault_mode = \"full\"",
        "modes = [\"full\"]\ndefault_mode = \"upsert\"",
        "modes = [\"full\", \"full\"]",
        "modes = [\"merge\"]\ndefault_mode = \"merge\"",
        "default_mode = \"Full\"",
        "mode = [\"upsert\"]",
    ] {
        assert!(ok(bad).is_err(), "accepted: {bad}");
    }
}

#[test]
fn a_config_changed_in_code_is_checked_too() {
    let mut r = RosterConfig {
        modes: vec!["upsert".into()],
        ..RosterConfig::default()
    };
    assert!(r.check().is_err(), "default_mode full is not in modes");
    r.default_mode = "upsert".into();
    assert!(r.check().is_ok());
}

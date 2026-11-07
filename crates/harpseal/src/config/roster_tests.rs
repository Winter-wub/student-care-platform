use super::*;

fn spec(kind: AttributeType, values: &[&str], optional: bool) -> AttributeSpec {
    AttributeSpec {
        column: "c".into(),
        kind,
        values: values.iter().map(|v| v.to_string()).collect(),
        label: BTreeMap::new(),
        min: None,
        max: None,
        value_labels: BTreeMap::new(),
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
    assert!(
        r.check(&Locale::th(), &Default::default()).is_err(),
        "default_mode full is not in modes"
    );
    r.default_mode = "upsert".into();
    assert!(r.check(&Locale::th(), &Default::default()).is_ok());
}

#[test]
fn blocking_defaults_to_none_and_names_warning_classes() {
    let base = "name = \"X\"\ndefault_locale = \"th\"\n[roster]\n";
    let ok = |extra: &str| crate::DeploymentConfig::from_toml(&format!("{base}{extra}"));
    let none = ok("").unwrap();
    assert!(none.roster.blocking.is_empty());
    assert!(WARNING_CLASSES.iter().all(|c| !none.roster.blocks(c)));
    let c = ok("blocking = [\"invalid_email\", \"duplicate_email\"]").unwrap();
    assert!(c.roster.blocks("invalid_email") && c.roster.blocks("duplicate_email"));
    assert!(!c.roster.blocks("missing_email") && !c.roster.blocks("empty_attribute"));
    assert!(c.roster.is_mapped());
    for bad in [
        // Problems that always block cannot be listed (nor made warnings).
        "blocking = [\"duplicate_code\"]",
        "blocking = [\"required\"]",
        "blocking = [\"bad_email\"]",
        "blocking = [\"missing_email\", \"missing_email\"]",
        "blocking = \"invalid_email\"",
    ] {
        assert!(ok(bad).is_err(), "accepted: {bad}");
    }
}

fn toml(attribute: &str) -> crate::Result<crate::DeploymentConfig> {
    crate::DeploymentConfig::from_toml(&format!(
        "name = \"X\"\ndefault_locale = \"th\"\n[roster.attributes.a]\ncolumn = \"c\"\n{attribute}"
    ))
}

#[test]
fn min_and_max_bound_integers_and_numbers_inclusively() {
    let c = toml("type = \"integer\"\nmin = 1\nmax = 6").unwrap();
    let year = &c.roster.attributes["a"];
    assert_eq!(year.range_text().as_deref(), Some("1–6"));
    for ok in ["1", "6", " 3 ", "+2"] {
        assert!(year.parse(ok).is_ok(), "{ok}");
    }
    for bad in ["0", "7", "-1", "600"] {
        assert_eq!(year.parse(bad), Err(AttrError::OutOfRange), "{bad}");
    }
    assert_eq!(year.parse("x"), Err(AttrError::NotInteger), "type first");
    assert_eq!(AttrError::OutOfRange.key(), "kernel.students.err_range");
    let gpa = toml("type = \"number\"\nmin = 0\nmax = 4.0\noptional = true").unwrap();
    let gpa = &gpa.roster.attributes["a"];
    assert_eq!(gpa.range_text().as_deref(), Some("0–4.0"));
    assert!(gpa.parse("4").is_ok() && gpa.parse("0").is_ok() && gpa.parse("3.99").is_ok());
    assert_eq!(gpa.parse("4.01"), Err(AttrError::OutOfRange));
    assert_eq!(gpa.parse("-0.5"), Err(AttrError::OutOfRange));
    assert_eq!(gpa.parse(""), Ok(None), "empty is not out of range");
    let open = toml("type = \"integer\"\nmin = 1").unwrap();
    let open = &open.roster.attributes["a"];
    assert_eq!(open.range_text().as_deref(), Some("≥ 1"));
    assert!(open.parse("900000").is_ok() && open.parse("0").is_err());
    let none = toml("type = \"integer\"").unwrap();
    assert_eq!(none.roster.attributes["a"].range_text(), None);
    assert!(
        none.roster.attributes["a"].parse("-5").is_ok(),
        "no rule, no check"
    );
}

#[test]
fn a_bad_range_or_label_stops_the_build() {
    let labels = "type = \"enum\"\nvalues = [\"x\", \"y\"]\n";
    for bad in [
        "type = \"integer\"\nmin = 6\nmax = 1",
        "type = \"integer\"\nmin = 1.5",
        "type = \"integer\"\nmax = \"6\"",
        "type = \"text\"\nmin = 1",
        "type = \"date\"\nmax = 3",
        &format!("{labels}min = 1"),
        "type = \"integer\"\nvalue_labels = { x = { th = \"x\" } }",
        &format!("{labels}value_labels = {{ z = {{ th = \"Z\" }} }}"),
        &format!("{labels}value_labels = {{ x = {{ th = \" \" }} }}"),
        // Two values share a label in one Locale (case-insensitively).
        &format!("{labels}value_labels = {{ x = {{ en = \"Open\" }}, y = {{ en = \"open\" }} }}"),
        // A default-Locale label is another value, so a cell could mean either.
        &format!("{labels}value_labels = {{ x = {{ th = \"y\" }} }}"),
        &format!("{labels}value_labels = {{ x = {{ th = \"Y\" }} }}"),
        // Unknown keys are still refused.
        &format!("{labels}labels = {{ x = {{ th = \"X\" }} }}"),
        "type = \"integer\"\nminimum = 1",
    ] {
        assert!(toml(bad).is_err(), "accepted: {bad}");
    }
    // The same label in two Locales, or one value labelled in one Locale only.
    let ok = format!(
        "{labels}value_labels = {{ x = {{ th = \"เปิด\", en = \"Open\" }}, y = {{ th = \"ปิด\", en = \"Open2\" }} }}"
    );
    assert!(toml(&ok).is_ok());
    // A label equal to its own value is fine.
    assert!(
        toml(&format!(
            "{labels}value_labels = {{ x = {{ th = \"x\" }} }}"
        ))
        .is_ok()
    );
    // What a value SHOWS decides: an English label that is another value's
    // text clashes while that value has no English label (it shows its value)…
    let clash = format!("{labels}value_labels = {{ x = {{ th = \"x\", en = \"y\" }} }}");
    let err = toml(&clash).unwrap_err().to_string();
    assert!(err.contains("`x` and `y` both show `y` in en"), "{err}");
    // …and a label in one Locale can clash through the default Locale's.
    let via_default =
        format!("{labels}value_labels = {{ x = {{ th = \"เปิด\" }}, y = {{ th = \"เปิด\" }} }}");
    assert!(toml(&via_default).is_err());
    let via_fallback =
        format!("{labels}value_labels = {{ x = {{ th = \"เปิด\" }}, y = {{ en = \"เปิด\" }} }}");
    assert!(toml(&via_fallback).is_err(), "y shows x's Thai label in en");
    // Both labelled, the English labels may be each other's values.
    let swapped = format!(
        "{labels}value_labels = {{ x = {{ th = \"x\", en = \"y\" }}, y = {{ en = \"x\" }} }}"
    );
    assert!(toml(&swapped).is_ok());
    // Labels in one Locale only, the rest falling back, are fine when distinct.
    let one = format!("{labels}value_labels = {{ x = {{ en = \"Open\" }} }}");
    assert!(toml(&one).is_ok());
}

#[test]
fn a_label_locale_must_be_a_configured_one() {
    let attr = "[roster.attributes.a]\ncolumn = \"c\"\ntype = \"enum\"\nvalues = [\"x\"]\n";
    let src = |labels: &str, texts: &str| {
        crate::DeploymentConfig::from_toml(&format!(
            "name = \"X\"\ndefault_locale = \"th\"\n{texts}{attr}value_labels = {labels}"
        ))
    };
    assert!(src("{ x = { th = \"เอ็กซ์\", en = \"X\" } }", "").is_ok());
    for typo in ["TH", "fr", "th-TH", "en "] {
        let err = src(&format!("{{ x = {{ \"{typo}\" = \"X\" }} }}"), "").unwrap_err();
        assert!(err.to_string().contains("not configured"), "{typo}: {err}");
    }
    // A Locale with a `[texts]` table is configured.
    let fr = "[texts.fr]\n\"fees.title\" = \"Frais\"\n";
    assert!(src("{ x = { fr = \"X\" } }", fr).is_ok());
}

#[test]
fn labels_fall_back_to_the_default_locale_then_the_value() {
    let src = "type = \"enum\"\nvalues = [\"current\", \"left\", \"new\"]\n\
               value_labels = { current = { th = \"กำลังศึกษา\", en = \"Studying\" }, left = { th = \"พ้นสภาพ\" } }";
    let c = toml(src).unwrap();
    let a = &c.roster.attributes["a"];
    let (th, en, fr) = (Locale::th(), Locale::en(), Locale("fr".into()));
    assert_eq!(a.value_label("current", &en, &th), "Studying");
    assert_eq!(a.value_label("left", &en, &th), "พ้นสภาพ", "default Locale");
    assert_eq!(a.value_label("current", &fr, &th), "กำลังศึกษา");
    assert_eq!(a.value_label("new", &en, &th), "new", "the value itself");
    assert_eq!(a.labelled("current", &en, &th), "Studying (current)");
    assert_eq!(a.labelled("new", &en, &th), "new");
    let stored = Json::from("current");
    assert_eq!(a.shown(Some(&stored), &en, &th), "Studying");
    assert_eq!(a.shown(Some(&Json::from("gone")), &en, &th), "gone");
    assert_eq!(a.shown(None, &en, &th), "");
    // Without labels a value shows as it is.
    let plain = toml("type = \"enum\"\nvalues = [\"x\"]").unwrap();
    assert_eq!(
        plain.roster.attributes["a"].shown(Some(&Json::from("x")), &en, &th),
        "x"
    );
}

#[test]
fn the_import_reads_a_value_or_its_default_locale_label() {
    let src = "type = \"enum\"\nvalues = [\"current\", \"Alumni\"]\n\
               value_labels = { current = { th = \"กำลังศึกษา\", en = \"Studying\" }, Alumni = { th = \"ศิษย์เก่า Bay\" } }";
    let c = toml(src).unwrap();
    let a = &c.roster.attributes["a"];
    let th = Locale::th();
    let value = |v: &str| Ok(Some(Json::from(v)));
    assert_eq!(a.parse_import("current", &th), value("current"));
    assert_eq!(
        a.parse_import(" กำลังศึกษา ", &th),
        value("current"),
        "trimmed"
    );
    assert_eq!(
        a.parse_import("ศิษย์เก่า bay", &th),
        value("Alumni"),
        "ASCII in either case"
    );
    assert_eq!(a.parse_import("ศิษย์เก่า BAY", &th), value("Alumni"));
    // Not the English label, not a value in the wrong case, not a lookalike.
    for bad in ["Studying", "CURRENT", "alumni", "กำลังศึกษาอยู่", ""] {
        assert!(a.parse_import(bad, &th).is_err(), "{bad}");
    }
    // `parse` (the edit form, a Module) still reads the value only.
    assert_eq!(a.parse("กำลังศึกษา"), Err(AttrError::NotInList));
    // The label is read in the Locale it is given, not a fixed one.
    assert_eq!(a.parse_import("Studying", &Locale::en()), value("current"));
}

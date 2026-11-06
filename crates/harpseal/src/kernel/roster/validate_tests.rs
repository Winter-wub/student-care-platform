use super::*;
use crate::kernel::roster::file::parse;
use crate::kernel::roster::test_config;

const HEAD: &str = "รหัสนักศึกษา,ชื่อ-สกุล,อีเมล,ภาษา,กลุ่ม,หลักสูตร,ชั้นปี,เกรดเฉลี่ย\n";

fn check(body: &str) -> Result<Valid, Vec<Problem>> {
    let c = test_config();
    let rows = parse(format!("{HEAD}{body}").as_bytes(), "r.csv", &c.roster).unwrap();
    validate(&rows, &c)
}

fn reasons(body: &str) -> Vec<(usize, String, String)> {
    check(body)
        .unwrap_err()
        .into_iter()
        .map(|p| (p.row, p.column, p.reason))
        .collect()
}

fn one(row: usize, column: &str, reason: &str) -> Vec<(usize, String, String)> {
    vec![(row, column.into(), reason.into())]
}

#[test]
fn a_valid_row_becomes_a_record() {
    let v = check(
        "68010001,นก ทะเล,s68010001@student.sealbay.example,,\"ปี1; MSC;;ปี1\",MSC,1,3.5\n\
         68010002,ปลา ทอง,,en,,MSC-Intl,2,\n",
    )
    .unwrap();
    assert_eq!(v.empty_email, 1);
    let r = &v.records[0];
    assert_eq!((r.row, r.code.as_str()), (2, "68010001"));
    assert_eq!(r.fields.locale, Locale::th(), "empty locale: the default");
    assert_eq!(r.tags, vec!["ปี1".to_string(), "MSC".into()]);
    assert_eq!(r.fields.attributes["year"], 1);
    assert_eq!(r.fields.attributes["gpa"], 3.5);
    assert_eq!(r.fields.status, "active");
    let r = &v.records[1];
    assert_eq!(r.fields.email, None);
    assert_eq!(r.fields.locale, Locale::en());
    assert!(!r.fields.attributes.contains_key("gpa"), "optional, empty");
    assert!(r.tags.is_empty());
}

#[test]
fn a_bad_student_id_is_refused() {
    assert_eq!(
        reasons("6801000X,นก,,,,MSC,1,\n"),
        one(2, "รหัสนักศึกษา", "bad_code")
    );
    assert_eq!(reasons(",นก,,,,MSC,1,\n"), one(2, "รหัสนักศึกษา", "required"));
}

#[test]
fn a_duplicate_student_id_is_refused_on_its_second_row() {
    assert_eq!(
        reasons("68010001,นก,,,,MSC,1,\n68010001,ปลา,,,,MSC,1,\n"),
        one(3, "รหัสนักศึกษา", "duplicate_code")
    );
}

#[test]
fn a_bad_email_is_refused() {
    for bad in [
        "nok",
        "nok@sealbay",
        "a b@x.example",
        "a@-x.example",
        "a@x..example",
    ] {
        assert_eq!(
            reasons(&format!("68010001,นก,{bad},,,MSC,1,\n")),
            one(2, "อีเมล", "bad_email"),
            "{bad}"
        );
    }
    assert!(is_email("first.last+tag@student.sealbay.example"));
}

#[test]
fn attribute_types_enums_and_required_values() {
    assert_eq!(
        reasons("68010001,นก,,,,MBA,1,\n"),
        one(2, "หลักสูตร", "not_in_list")
    );
    assert_eq!(
        reasons("68010001,นก,,,,MSC,one,\n"),
        one(2, "ชั้นปี", "not_integer")
    );
    assert_eq!(
        reasons("68010001,นก,,,,MSC,1,x\n"),
        one(2, "เกรดเฉลี่ย", "not_number")
    );
    assert_eq!(
        reasons("68010001,นก,,,,,1,\n"),
        one(2, "หลักสูตร", "required")
    );
    assert_eq!(
        reasons("68010001,,,,,MSC,1,\n"),
        one(2, "ชื่อ-สกุล", "required")
    );
}

#[test]
fn tags_locale_and_every_problem_is_listed() {
    assert_eq!(
        reasons("68010001,นก,,,\"ปี1,MSC\",MSC,1,\n"),
        one(2, "กลุ่ม", "tag_separator")
    );
    assert_eq!(
        reasons("68010001,นก,,ภาษาไทย,,MSC,1,\n"),
        one(2, "ภาษา", "bad_locale")
    );
    let all = reasons("x,,bad,,,MBA,,\n");
    assert_eq!(all.len(), 5, "{all:?}");
}

#[test]
fn file_errors_become_reports() {
    let p = file_problems(&FileError::Headers {
        missing: vec!["อีเมล".into()],
        unexpected: vec!["หมายเหตุ".into()],
    });
    assert_eq!(p[0], Problem::new(1, "อีเมล", "missing_header"));
    assert_eq!(p[1], Problem::new(1, "หมายเหตุ", "unexpected_header"));
    assert_eq!(
        file_problems(&FileError::Unsupported),
        vec![Problem::new(0, "", "unsupported")]
    );
}

#[test]
fn more_than_20_tags_are_refused() {
    let tags: Vec<String> = (0..21).map(|i| format!("g{i}")).collect();
    let line = format!("68010001,นก,,,\"{}\",MSC,1,\n", tags.join(";"));
    assert_eq!(reasons(&line), one(2, "กลุ่ม", "too_many_tags"));
    let ok = format!("68010001,นก,,,\"{}\",MSC,1,\n", tags[..20].join(";"));
    assert!(check(&ok).is_ok());
}

#[test]
fn without_a_pattern_codes_follow_the_chat_flow_default() {
    let mut c = test_config();
    c.student.id_pattern = None;
    let rows = |codes: &str| {
        let body = format!("{HEAD}{codes}");
        parse(body.as_bytes(), "r.csv", &c.roster).unwrap()
    };
    let bad = validate(&rows("abc,นก,,,,MSC,1,\nab cd,ปลา,,,,MSC,1,\n"), &c).unwrap_err();
    assert_eq!(bad.len(), 2, "{bad:?}");
    assert!(bad.iter().all(|p| p.reason == "bad_code"));
    assert!(validate(&rows("SBU-2026-01,นก,,,,MSC,1,\n"), &c).is_ok());
}

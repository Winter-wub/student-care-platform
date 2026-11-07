//! Workbooks with several sheets, title rows and footers (#70), through
//! the roster's read and validation.

use super::*;
use crate::kernel::roster::test_config;
use crate::kernel::roster::validate::{Problem, validate};

const COHORTS: &[u8] = include_bytes!("../../../tests/fixtures/roster/cohorts.xlsx");
const NO_HEADER: &[u8] = include_bytes!("../../../tests/fixtures/roster/cohorts_no_header.xlsx");
const DUPLICATE: &[u8] = include_bytes!("../../../tests/fixtures/roster/cohorts_duplicate.xlsx");

fn at(p: &Problem) -> (&str, usize, &str, &str) {
    (
        p.sheet.as_str(),
        p.row,
        p.column.as_str(),
        p.reason.as_str(),
    )
}

#[test]
fn every_sheet_is_read_past_its_titles_and_footer() {
    let c = test_config();
    let rows = parse(COHORTS, "cohorts.xlsx", &c.roster).unwrap();
    let got: Vec<(&str, usize, Option<&str>)> = rows
        .iter()
        .map(|r| (r.sheet(), r.number, r.get("รหัสนักศึกษา")))
        .collect();
    assert_eq!(
        got,
        vec![
            ("ปี 1", 5, Some("68010001")),
            ("ปี 1", 6, Some("68010002")),
            ("ปี 1", 7, Some("68010003")),
            ("ปี 2", 2, Some("67010001")),
            ("ปี 2", 3, Some("67010002")),
        ],
        "titles, the totals row, the notes row and the empty sheet are skipped"
    );
    let valid = validate(&rows, &c).unwrap();
    assert_eq!(valid.records.len(), 5);
    // Numbers stored as text: " 2 " and "3.50".
    let y2 = &valid.records[3].fields.attributes;
    assert_eq!(y2["year"], serde_json::json!(2));
    assert_eq!(y2["gpa"], serde_json::json!(3.5));
    assert_eq!(valid.records[1].fields.attributes.get("gpa"), None);
}

#[test]
fn a_sheet_without_the_header_is_refused_by_name() {
    let c = test_config();
    let e = parse(NO_HEADER, "c.xlsx", &c.roster).unwrap_err();
    let problems = crate::kernel::roster::validate::file_problems(&e);
    assert_eq!(problems.len(), 1, "one clear problem: {problems:?}");
    assert_eq!(at(&problems[0]), ("สรุป", 0, "", "no_header_row"));
}

#[test]
fn a_header_with_a_wrong_column_names_its_sheet_and_row() {
    let c = test_config();
    let text = "รายชื่อ\n\nรหัสนักศึกษา,ชื่อ,อีเมล,ภาษา,กลุ่ม,หลักสูตร,ชั้นปี,เกรดเฉลี่ย\n";
    let e = parse(text.as_bytes(), "a.csv", &c.roster).unwrap_err();
    let problems = crate::kernel::roster::validate::file_problems(&e);
    let got: Vec<_> = problems.iter().map(at).collect();
    assert_eq!(
        got,
        vec![
            ("", 3, "ชื่อ-สกุล", "missing_header"),
            ("", 3, "ชื่อ", "unexpected_header")
        ]
    );
}

#[test]
fn a_duplicate_id_across_sheets_and_an_error_cell_carry_sheet_and_row() {
    let c = test_config();
    let rows = parse(DUPLICATE, "d.xlsx", &c.roster).unwrap();
    let problems = validate(&rows, &c).unwrap_err();
    let got: Vec<_> = problems.iter().map(at).collect();
    assert_eq!(
        got,
        vec![
            ("ปี 2", 2, "รหัสนักศึกษา", "duplicate_code"),
            ("ปี 2", 3, "เกรดเฉลี่ย", "unreadable"),
        ]
    );
    assert_eq!(problems[1].cell, "H3");
    assert_eq!(problems[0].cell, "", "only a cell at fault names one");
}

#[test]
fn only_empty_sheets_make_an_empty_file() {
    let c = test_config();
    let empty = include_bytes!("../../../tests/fixtures/roster/empty_sheet.xlsx");
    assert_eq!(parse(empty, "e.xlsx", &c.roster), Err(FileError::Empty));
    let header = expected_headers(&c.roster).join(",");
    let only_header = format!("{header}\n");
    assert_eq!(
        parse(only_header.as_bytes(), "a.csv", &c.roster),
        Ok(vec![])
    );
}

#[test]
fn problems_stored_before_sheets_read_back() {
    let old: Problem =
        serde_json::from_str(r#"{"row":2,"column":"อีเมล","reason":"bad_email"}"#).unwrap();
    assert_eq!(old, Problem::new(2, "อีเมล", "bad_email"));
    assert_eq!(
        serde_json::to_string(&old).unwrap(),
        r#"{"row":2,"column":"อีเมล","reason":"bad_email"}"#,
        "a CSV's problems store as before"
    );
}

const TRAILING: &[u8] = include_bytes!("../../../tests/fixtures/roster/cohorts_trailing.xlsx");
const HIDDEN: &[u8] = include_bytes!("../../../tests/fixtures/roster/cohorts_hidden.xlsx");

#[test]
fn skipped_footer_rows_and_hidden_sheets_are_listed() {
    let c = test_config();
    let r = read(COHORTS, "cohorts.xlsx", &c.roster).unwrap();
    let got: Vec<_> = r
        .skipped
        .iter()
        .map(|s| (s.sheet.as_str(), s.row, s.reason.as_str(), s.text.as_str()))
        .collect();
    assert_eq!(
        got,
        vec![
            ("ปี 1", 8, "footer", "รวม | 3"),
            ("ปี 2", 5, "footer", "หมายเหตุ: ข้อมูล ณ วันที่ 1 พฤศจิกายน 2569"),
        ]
    );
    let h = read(HIDDEN, "h.xlsx", &c.roster).unwrap();
    assert_eq!(h.rows.len(), 2, "the visible sheet only");
    assert_eq!(
        (
            h.skipped[1].sheet.as_str(),
            h.skipped[1].row,
            h.skipped[1].reason.as_str()
        ),
        ("lists", 0, "hidden_sheet")
    );
}

#[test]
fn an_id_less_student_and_an_unreadable_id_below_the_data_are_reported() {
    let c = test_config();
    let r = read(TRAILING, "t.xlsx", &c.roster).unwrap();
    assert_eq!(r.skipped.len(), 1, "only the totals row");
    let problems = validate(&r.rows, &c).unwrap_err();
    let got: Vec<_> = problems.iter().map(at).collect();
    assert!(
        got.contains(&("ปี 1", 6, "รหัสนักศึกษา", "required")),
        "{got:?}"
    );
    assert!(
        got.contains(&("ปี 1", 7, "รหัสนักศึกษา", "unreadable")),
        "{got:?}"
    );
    assert!(!got.iter().any(|p| p.1 == 8), "{got:?}");
}

#[test]
fn a_repeated_roster_header_is_reported_not_overwritten() {
    let c = test_config();
    let text = format!("{},อีเมล\n", expected_headers(&c.roster).join(","));
    let e = parse(text.as_bytes(), "a.csv", &c.roster).unwrap_err();
    let problems = crate::kernel::roster::validate::file_problems(&e);
    assert_eq!(
        problems.iter().map(at).collect::<Vec<_>>(),
        vec![("", 1, "อีเมล", "unexpected_header")]
    );
}

#[test]
fn a_calculated_gpa_column_imports_as_main_did() {
    let c = test_config();
    let calc = include_bytes!("../../../tests/fixtures/roster/calculated.xlsx");
    let rows = parse(calc, "c.xlsx", &c.roster).unwrap();
    let valid = validate(&rows, &c).unwrap();
    let a = |i: usize| &valid.records[i].fields.attributes;
    assert_eq!(a(0)["gpa"], serde_json::json!(3.33333333333333));
    assert_eq!(a(1)["gpa"], serde_json::json!(2.66666666666667));
    assert_eq!(
        a(2)["year"],
        serde_json::json!(1),
        "=0.1*10 is a whole year"
    );
}

#[test]
fn hidden_sheets_are_named_when_they_hold_the_data_or_a_roster() {
    let c = test_config();
    let only = include_bytes!("../../../tests/fixtures/roster/only_hidden.xlsx");
    let e = parse(only, "o.xlsx", &c.roster).unwrap_err();
    assert_eq!(e, FileError::OnlyHidden(vec!["ปี 2".into()]));
    let problems = crate::kernel::roster::validate::file_problems(&e);
    assert_eq!(at(&problems[0]), ("ปี 2", 0, "", "only_hidden"));
    let roster = include_bytes!("../../../tests/fixtures/roster/cohorts_hidden_roster.xlsx");
    let r = read(roster, "h.xlsx", &c.roster).unwrap();
    assert_eq!(r.rows.len(), 3, "the visible sheet");
    let hidden: Vec<_> = r
        .skipped
        .iter()
        .filter(|s| s.row == 0)
        .map(|s| (s.sheet.as_str(), s.reason.as_str()))
        .collect();
    assert_eq!(hidden, vec![("ปี 2", "hidden_roster_sheet")]);
}

#[test]
fn a_number_only_row_below_the_data_is_a_student_without_an_id() {
    let c = test_config();
    let text = format!(
        "{}\n68010001,นก,,,,MSC,1,\n,,,,,,2,3.25\n",
        expected_headers(&c.roster).join(",")
    );
    let rows = parse(text.as_bytes(), "a.csv", &c.roster).unwrap();
    assert_eq!(rows.len(), 2, "no label: not a footer");
    let problems = validate(&rows, &c).unwrap_err();
    assert!(
        problems
            .iter()
            .any(|p| at(p) == ("", 3, "รหัสนักศึกษา", "required"))
    );
}

#[test]
fn a_hidden_tab_with_the_id_column_but_not_every_column_is_a_roster_sheet() {
    let c = test_config();
    let partial = include_bytes!("../../../tests/fixtures/roster/cohorts_hidden_partial.xlsx");
    let r = read(partial, "p.xlsx", &c.roster).unwrap();
    let hidden: Vec<_> = r
        .skipped
        .iter()
        .filter(|s| s.row == 0)
        .map(|s| (s.sheet.as_str(), s.reason.as_str()))
        .collect();
    assert_eq!(hidden, vec![("ปี 0", "hidden_roster_sheet")]);
    // A hidden sheet without the id column stays a plain hidden sheet.
    let lists = read(HIDDEN, "h.xlsx", &c.roster).unwrap();
    assert!(lists.skipped.iter().any(|s| s.reason == "hidden_sheet"));
}

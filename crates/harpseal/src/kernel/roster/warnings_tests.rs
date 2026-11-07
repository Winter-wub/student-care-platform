use super::*;
use crate::kernel::roster::file::parse;
use crate::kernel::roster::test_config;
use crate::kernel::roster::validate::{Valid, validate, validate_against};
use std::collections::{HashMap, HashSet};

const HEAD: &str = "รหัสนักศึกษา,ชื่อ-สกุล,อีเมล,ภาษา,กลุ่ม,หลักสูตร,ชั้นปี,เกรดเฉลี่ย\n";
const MAIL: &str = "s@student.sealbay.example";

fn read(body: &str, c: &crate::DeploymentConfig, taken: &Taken) -> Result<Valid, Vec<Problem>> {
    let rows = parse(format!("{HEAD}{body}").as_bytes(), "r.csv", &c.roster).unwrap();
    validate_against(&rows, c, taken)
}

/// `(row, reason)` of every warning.
fn warned(body: &str, taken: &Taken) -> Vec<(usize, String)> {
    let v = read(body, &test_config(), taken).unwrap();
    assert!(v.warnings.iter().all(|p| p.severity == Severity::Warning));
    v.warnings.into_iter().map(|p| (p.row, p.reason)).collect()
}

fn w(row: usize, reason: &str) -> (usize, String) {
    (row, reason.into())
}

fn none() -> Taken {
    Taken::default()
}

#[test]
fn a_file_with_only_warnings_is_valid_and_lists_each_class() {
    let got = warned(
        "68010001,นก,,,,MSC,1,3.5\n\
         68010002,ปลา,nok@sealbay,,,MSC,1,3.5\n\
         68010003,กุ้ง,ok@student.sealbay.example,,,MSC,1,\n",
        &none(),
    );
    assert_eq!(
        got,
        vec![
            w(2, "missing_email"),
            w(3, "invalid_email"),
            w(4, "empty_attribute")
        ]
    );
}

#[test]
fn an_email_two_rows_share_warns_on_both_whatever_its_case_in_file_order() {
    let got = warned(
        &format!(
            "68010001,นก,{MAIL},,,MSC,1,1\n68010002,ปลา,,,,MSC,1,1\n\
             68010003,กุ้ง,x@x.example,,,MSC,1,1\n68010004,หอย,{},,,MSC,1,1\n",
            MAIL.to_uppercase()
        ),
        &none(),
    );
    assert_eq!(
        got,
        vec![
            w(2, "duplicate_email"),
            w(3, "missing_email"),
            w(5, "duplicate_email")
        ]
    );
}

fn owner(code: &str, active: bool, status_held: bool, email_held: bool, full: bool) -> Taken {
    let o = Owner {
        code: code.into(),
        active,
        status_held,
        email_held,
    };
    Taken {
        by_email: HashMap::from([(MAIL.to_string(), vec![o])]),
        hand_set: HashSet::new(),
        full,
    }
}

#[test]
fn an_email_an_existing_student_keeps_warns_unless_it_is_theirs_or_moving() {
    let row = |code: &str| format!("{code},นก,{MAIL},,,MSC,1,1\n");
    let dup = vec![w(2, "duplicate_email")];
    // Another active Student outside the file has it.
    assert_eq!(
        warned(
            &row("68010001"),
            &owner("68010009", true, false, false, false)
        ),
        dup
    );
    // The row's own Student has it.
    assert!(
        warned(
            &row("68010001"),
            &owner("68010001", true, false, false, false)
        )
        .is_empty()
    );
    // A left Student outside the file does not keep it ...
    assert!(
        warned(
            &row("68010001"),
            &owner("68010009", false, false, false, false)
        )
        .is_empty()
    );
    // ... nor an active one a full import is about to mark left,
    assert!(
        warned(
            &row("68010001"),
            &owner("68010009", true, false, false, true)
        )
        .is_empty()
    );
    // unless Staff set the status by hand, so the import keeps them active.
    assert_eq!(
        warned(
            &row("68010001"),
            &owner("68010009", true, true, false, true)
        ),
        dup
    );
    // Another Student is in the file too, and the file changes their email ...
    let moving = format!("{}68010009,ปลา,y@y.example,,,MSC,1,1\n", row("68010001"));
    assert!(warned(&moving, &owner("68010009", true, false, false, false)).is_empty());
    // ... unless Staff set that email by hand, so the file keeps it.
    assert_eq!(
        warned(&moving, &owner("68010009", true, false, true, false)),
        dup
    );
    // A left Student the file lists comes back, and keeps a hand-set email.
    assert_eq!(
        warned(&moving, &owner("68010009", false, false, true, false)),
        dup
    );
    assert!(warned(&moving, &owner("68010009", false, true, true, false)).is_empty());
}

#[test]
fn a_row_whose_email_staff_set_by_hand_is_not_compared() {
    // Both rows carry the same address, but the file's value for the second
    // Student is not stored: no duplicate.
    let body = format!("68010001,นก,{MAIL},,,MSC,1,1\n68010002,ปลา,{MAIL},,,MSC,1,1\n");
    let mut taken = none();
    taken.hand_set.insert("68010002".into());
    assert!(warned(&body, &taken).is_empty());
}

#[test]
fn empty_optional_cells_make_one_warning_per_column_with_a_count_and_first_rows() {
    let rows: String = (1..=7)
        .map(|n| format!("6801000{n},นก{n},n{n}@student.sealbay.example,,,MSC,1,\n"))
        .collect();
    let v = read(&rows, &test_config(), &none()).unwrap();
    assert_eq!(v.warnings.len(), 1);
    let p = &v.warnings[0];
    assert_eq!((p.row, p.column.as_str(), p.count), (2, "เกรดเฉลี่ย", 7));
    assert_eq!(p.rows, vec![2, 3, 4, 5, 6]);
    assert_eq!(
        classes(&v.warnings)["empty_attribute"],
        7,
        "cells, not columns"
    );
    assert_eq!(total(&v.warnings), 7);
    // Blocking, it is still one problem for the column.
    let mut c = test_config();
    c.roster.blocking = vec!["empty_attribute".into()];
    let all = read(&rows, &c, &none()).unwrap_err();
    assert_eq!(
        (all.len(), all[0].severity, all[0].count),
        (1, Severity::Blocking, 7)
    );
}

#[test]
fn a_deployment_makes_a_warning_class_blocking() {
    let mut c = test_config();
    let body = "68010001,นก,nok@sealbay,,,MSC,1,\n68010002,ปลา,,,,MSC,1,1\n";
    assert!(read(body, &c, &none()).is_ok());
    c.roster.blocking = vec!["invalid_email".into()];
    let all = read(body, &c, &none()).unwrap_err();
    let got: Vec<_> = all
        .iter()
        .map(|p| (p.row, p.reason.as_str(), p.severity))
        .collect();
    assert_eq!(
        got,
        vec![
            (2, "invalid_email", Severity::Blocking),
            (3, "missing_email", Severity::Warning),
            // One per column, after the rows' own problems.
            (2, "empty_attribute", Severity::Warning),
        ]
    );
    // The other classes still only warn.
    c.roster.blocking = vec!["duplicate_email".into(), "empty_attribute".into()];
    assert_eq!(read(body, &c, &none()).unwrap_err().len(), 3);
    c.roster.blocking = vec!["missing_email".into()];
    let all = read(body, &c, &none()).unwrap_err();
    assert_eq!(all.iter().filter(|p| p.severity.is_blocking()).count(), 1);
}

#[test]
fn an_excel_error_cell_is_unreadable_not_an_empty_attribute() {
    let c = test_config();
    let book = include_bytes!("../../../tests/fixtures/roster/formula_error.xlsx");
    let rows = parse(book, "f.xlsx", &c.roster).unwrap();
    let got = validate(&rows, &c).unwrap_err();
    // The error cell is in the gpa column: its "empty optional" is dropped.
    assert!(
        got.iter()
            .all(|p| p.column != "เกรดเฉลี่ย" || p.reason == "unreadable")
    );
}

#[test]
fn only_a_warning_carries_its_severity_in_the_stored_json() {
    let blocking = Problem::new(2, "อีเมล", "required");
    assert_eq!(
        serde_json::to_string(&blocking).unwrap(),
        r#"{"row":2,"column":"อีเมล","reason":"required"}"#
    );
    let warning = Problem::new(2, "อีเมล", "missing_email").with_severity(false);
    let json = serde_json::to_string(&warning).unwrap();
    assert!(json.ends_with(r#""severity":"warning"}"#), "{json}");
    assert_eq!(serde_json::from_str::<Problem>(&json).unwrap(), warning);
    let mut many = Problem::new(2, "c", "empty_attribute");
    many.count = 7;
    many.rows = vec![2, 3];
    let json = serde_json::to_string(&many).unwrap();
    assert!(json.ends_with(r#""count":7,"rows":[2,3]}"#), "{json}");
    // A report stored before #71 reads as blocking.
    let old: Problem =
        serde_json::from_str(r#"{"row":3,"column":"c","reason":"bad_email"}"#).unwrap();
    assert!(old.severity.is_blocking());
}

#[test]
fn a_report_keeps_at_most_1000_and_counts_every_class() {
    let warn = |r| Problem::new(r, "c", "missing_email").with_severity(false);
    let all = vec![warn(2), Problem::new(3, "c", "required"), warn(4)];
    assert_eq!(classes(&all)["missing_email"], 2);
    let rows: Vec<_> = kept(all).iter().map(|p| p.row).collect();
    assert_eq!(rows, vec![2, 3, 4]);
    let many: Vec<_> = (0..1200).map(warn).collect();
    assert_eq!(kept(many).len(), MAX_PROBLEMS);
}

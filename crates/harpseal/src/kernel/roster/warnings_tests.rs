use super::*;
use crate::kernel::roster::file::parse;
use crate::kernel::roster::test_config;
use crate::kernel::roster::validate::{Valid, validate, validate_against};

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

#[test]
fn a_file_with_only_warnings_is_valid_and_lists_each_class() {
    let got = warned(
        "68010001,นก,,,,MSC,1,3.5\n\
         68010002,ปลา,nok@sealbay,,,MSC,1,3.5\n\
         68010003,กุ้ง,ok@student.sealbay.example,,,MSC,1,\n",
        &Taken::new(),
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
fn an_email_two_rows_share_warns_on_both_whatever_its_case() {
    let got = warned(
        &format!(
            "68010001,นก,{MAIL},,,MSC,1,1\n68010002,ปลา,x@x.example,,,MSC,1,1\n\
             68010003,กุ้ง,{},,,MSC,1,1\n",
            MAIL.to_uppercase()
        ),
        &Taken::new(),
    );
    assert_eq!(got, vec![w(2, "duplicate_email"), w(4, "duplicate_email")]);
}

fn owner(code: &str, held: bool) -> Taken {
    let o = Owner {
        code: code.into(),
        held,
    };
    Taken::from([(MAIL.to_string(), vec![o])])
}

#[test]
fn an_email_an_existing_student_has_warns_unless_it_is_theirs_or_moving() {
    let row = |code: &str| format!("{code},นก,{MAIL},,,MSC,1,1\n");
    // Another Student outside the file has it.
    let got = warned(&row("68010001"), &owner("68010009", false));
    assert_eq!(got, vec![w(2, "duplicate_email")]);
    // The row's own Student has it.
    assert!(warned(&row("68010001"), &owner("68010001", false)).is_empty());
    // Another Student is in the file too, and the file changes their email.
    let moving = format!("{}68010009,ปลา,y@y.example,,,MSC,1,1\n", row("68010001"));
    assert!(warned(&moving, &owner("68010009", false)).is_empty());
    // ... unless Staff set that email by hand, so the file keeps it.
    let got = warned(&moving, &owner("68010009", true));
    assert_eq!(got, vec![w(2, "duplicate_email")]);
}

#[test]
fn a_deployment_makes_a_warning_class_blocking() {
    let mut c = test_config();
    let body = "68010001,นก,nok@sealbay,,,MSC,1,\n68010002,ปลา,,,,MSC,1,1\n";
    assert!(read(body, &c, &Taken::new()).is_ok());
    c.roster.blocking = vec!["invalid_email".into()];
    let all = read(body, &c, &Taken::new()).unwrap_err();
    let got: Vec<_> = all
        .iter()
        .map(|p| (p.row, p.reason.as_str(), p.severity))
        .collect();
    assert_eq!(
        got,
        vec![
            (2, "invalid_email", Severity::Blocking),
            (2, "empty_attribute", Severity::Warning),
            (3, "missing_email", Severity::Warning),
        ]
    );
    // The other classes still only warn.
    c.roster.blocking = vec!["duplicate_email".into(), "empty_attribute".into()];
    assert_eq!(read(body, &c, &Taken::new()).unwrap_err().len(), 3);
    c.roster.blocking = vec!["missing_email".into()];
    let all = read(body, &c, &Taken::new()).unwrap_err();
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
    // A report stored before #71 reads as blocking.
    let old: Problem =
        serde_json::from_str(r#"{"row":3,"column":"c","reason":"bad_email"}"#).unwrap();
    assert!(old.severity.is_blocking());
}

#[test]
fn a_report_keeps_blocking_problems_first_and_counts_every_class() {
    let warn = |r| Problem::new(r, "c", "missing_email").with_severity(false);
    let all = vec![warn(2), Problem::new(3, "c", "required"), warn(4)];
    assert_eq!(classes(&all)["missing_email"], 2);
    let rows: Vec<_> = kept(all).iter().map(|p| p.row).collect();
    assert_eq!(rows, vec![3, 2, 4]);
    let many: Vec<_> = (0..1200).map(warn).collect();
    assert_eq!(kept(many).len(), MAX_PROBLEMS);
}

use calamine::{ExcelDateTime, ExcelDateTimeType};

use super::*;
use crate::kernel::roster::test_config;

const CSV: &[u8] = include_bytes!("../../../tests/fixtures/roster/seal_bay_50.csv");
const XLSX: &[u8] = include_bytes!("../../../tests/fixtures/roster/seal_bay_50.xlsx");

#[test]
fn the_csv_fixture_reads_50_rows() {
    let c = test_config();
    let rows = parse(CSV, "roster.csv", &c.roster).unwrap();
    assert_eq!(rows.len(), 50);
    let r = &rows[0];
    assert_eq!(r.number, 2);
    assert_eq!(r.get("รหัสนักศึกษา"), Some("68010001"));
    assert_eq!(r.get("ชื่อ-สกุล"), Some("นกน้อย ทะเลใส"));
    assert_eq!(r.get("กลุ่ม"), Some("ปี1;MSC"));
    assert_eq!(rows[9].get("อีเมล"), None, "empty cell is absent");
}

#[test]
fn the_xlsx_fixture_reads_the_same_cells() {
    let c = test_config();
    let x = parse(XLSX, "roster.xlsx", &c.roster).unwrap();
    let v = parse(CSV, "roster.csv", &c.roster).unwrap();
    assert_eq!(x.len(), 50);
    for (a, b) in x.iter().zip(&v) {
        assert_eq!(a.number, b.number);
        assert_eq!(a.get("รหัสนักศึกษา"), b.get("รหัสนักศึกษา"), "no .0");
        assert_eq!(a.get("ชื่อ-สกุล"), b.get("ชื่อ-สกุล"));
        assert_eq!(a.get("ชั้นปี"), b.get("ชั้นปี"));
        assert_eq!(a.get("กลุ่ม"), b.get("กลุ่ม"));
    }
    // Detected by its bytes when the name says nothing.
    assert_eq!(parse(XLSX, "roster", &c.roster).unwrap().len(), 50);
}

#[test]
fn semicolons_bom_crlf_and_empty_rows() {
    let c = test_config();
    let text = "\u{feff} รหัสนักศึกษา ;ชื่อ-สกุล;อีเมล;ภาษา;กลุ่ม;หลักสูตร;ชั้นปี;เกรดเฉลี่ย\r\n\
                ;;;;;;;\r\n\
                68010001; นก ทะเล ;;;;MSC;1;\r\n";
    let rows = parse(text.as_bytes(), "a.CSV", &c.roster).unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].number, 3);
    assert_eq!(rows[0].get("ชื่อ-สกุล"), Some("นก ทะเล"));
    assert_eq!(rows[0].get("อีเมล"), None);
}

#[test]
fn headers_must_match_exactly() {
    let c = test_config();
    let text = "รหัสนักศึกษา,ชื่อ,อีเมล,ภาษา,กลุ่ม,หลักสูตร,ชั้นปี,เกรดเฉลี่ย,หมายเหตุ\n";
    match parse(text.as_bytes(), "a.csv", &c.roster) {
        Err(FileError::Headers {
            missing,
            unexpected,
        }) => {
            assert_eq!(missing, vec!["ชื่อ-สกุล".to_string()]);
            assert_eq!(unexpected, vec!["ชื่อ".to_string(), "หมายเหตุ".into()]);
        }
        other => panic!("{other:?}"),
    }
    let twice = "รหัสนักศึกษา,ชื่อ-สกุล,อีเมล,ภาษา,กลุ่ม,หลักสูตร,ชั้นปี,เกรดเฉลี่ย,อีเมล\n";
    assert!(matches!(
        parse(twice.as_bytes(), "a.csv", &c.roster),
        Err(FileError::Headers { unexpected, .. }) if unexpected == vec!["อีเมล".to_string()]
    ));
}

#[test]
fn other_files_are_refused() {
    let c = test_config();
    assert_eq!(parse(b"x", "a.pdf", &c.roster), Err(FileError::Unsupported));
    assert_eq!(
        parse(&[0xff, 0xfe, 0x00], "a.csv", &c.roster),
        Err(FileError::NotUtf8)
    );
    assert_eq!(parse(b"\n\n", "a.csv", &c.roster), Err(FileError::Empty));
    assert_eq!(
        parse(b"PK\x03\x04garbage", "a.xlsx", &c.roster),
        Err(FileError::Xlsx)
    );
    assert!(matches!(
        parse(b"a\n\"x\n", "a.csv", &c.roster),
        Err(FileError::Csv(_))
    ));
}

#[test]
fn xlsx_cells_render_as_text() {
    assert_eq!(cell_text(&Data::Float(68010001.0)), "68010001");
    assert_eq!(cell_text(&Data::Float(2.5)), "2.5");
    assert_eq!(cell_text(&Data::Int(3)), "3");
    assert_eq!(cell_text(&Data::Bool(true)), "TRUE");
    assert_eq!(cell_text(&Data::Empty), "");
    // 2026-11-05 is serial 46331 in the 1900 system.
    let date = ExcelDateTime::new(46331.0, ExcelDateTimeType::DateTime, false);
    assert_eq!(cell_text(&Data::DateTime(date)), "2026-11-05");
    let at_noon = ExcelDateTime::new(46331.5, ExcelDateTimeType::DateTime, false);
    assert_eq!(cell_text(&Data::DateTime(at_noon)), "2026-11-05 12:00:00");
}

#[test]
fn a_sparse_sheet_is_refused_before_anything_is_allocated() {
    let c = test_config();
    let sparse = include_bytes!("../../../tests/fixtures/roster/sparse.xlsx");
    let t = std::time::Instant::now();
    let got = parse(sparse, "sparse.xlsx", &c.roster);
    assert!(
        matches!(got, Err(FileError::TooManyRows | FileError::TooManyColumns)),
        "{got:?}"
    );
    assert!(
        t.elapsed() < std::time::Duration::from_secs(2),
        "refused quickly"
    );
}

#[test]
fn an_empty_first_sheet_is_an_empty_file() {
    let c = test_config();
    let empty = include_bytes!("../../../tests/fixtures/roster/empty_sheet.xlsx");
    assert_eq!(parse(empty, "e.xlsx", &c.roster), Err(FileError::Empty));
}

#[test]
fn formula_cells_read_their_cached_value_and_error_cells_their_code() {
    let c = test_config();
    let book = include_bytes!("../../../tests/fixtures/roster/formula_error.xlsx");
    let rows = parse(book, "f.xlsx", &c.roster).unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].get("รหัสนักศึกษา"), Some("68010001"));
    assert_eq!(rows[0].get("ชั้นปี"), Some("2"));
    assert_eq!(rows[0].get("เกรดเฉลี่ย"), Some("#DIV/0!"));
    // The error code then fails the number check like any bad cell.
    let problems = crate::kernel::roster::validate::validate(&rows, &c).unwrap_err();
    assert_eq!(problems[0].reason, "not_number");
}

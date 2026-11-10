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
    // Told by its name only (#46): no extension is refused.
    assert_eq!(
        parse(XLSX, "roster", &c.roster),
        Err(FileError::Unsupported)
    );
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
        Err(FileError::Headers(sheets)) => {
            let HeaderMismatch {
                missing,
                unexpected,
                found,
                ..
            } = &sheets[0];
            assert_eq!(*missing, vec!["ชื่อ-สกุล".to_string()]);
            assert_eq!(*unexpected, vec!["ชื่อ".to_string(), "หมายเหตุ".into()]);
            // The row as found, in file order, for the page (#134).
            assert_eq!(found.len(), 9);
            assert_eq!(found[1], "ชื่อ");
            let e = FileError::Headers(sheets.clone());
            assert_eq!(e.found_headers()[0].found, *found);
        }
        other => panic!("{other:?}"),
    }
    let twice = "รหัสนักศึกษา,ชื่อ-สกุล,อีเมล,ภาษา,กลุ่ม,หลักสูตร,ชั้นปี,เกรดเฉลี่ย,อีเมล\n";
    assert!(matches!(
        parse(twice.as_bytes(), "a.csv", &c.roster),
        Err(FileError::Headers(s)) if s[0].unexpected == vec!["อีเมล".to_string()]
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
fn formula_cells_read_their_cached_value_and_error_cells_are_unreadable() {
    let c = test_config();
    let book = include_bytes!("../../../tests/fixtures/roster/formula_error.xlsx");
    let rows = parse(book, "f.xlsx", &c.roster).unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].get("รหัสนักศึกษา"), Some("68010001"));
    assert_eq!(rows[0].get("ชั้นปี"), Some("2"));
    // #70: an error cell is unreadable, never the text of its code.
    assert_eq!(rows[0].get("เกรดเฉลี่ย"), None);
    let unreadable: Vec<_> = rows[0].unreadable().collect();
    assert_eq!(unreadable, vec![("เกรดเฉลี่ย", "#DIV/0!", "H2")]);
    let problems = crate::kernel::roster::validate::validate(&rows, &c).unwrap_err();
    let problems: Vec<_> = problems
        .iter()
        .filter(|p| p.severity.is_blocking())
        .collect();
    assert_eq!(problems.len(), 1, "{problems:?}");
    assert_eq!(problems[0].reason, "unreadable");
    assert_eq!(
        (
            problems[0].sheet.as_str(),
            problems[0].row,
            problems[0].cell.as_str()
        ),
        ("รายชื่อ", 2, "H2")
    );
    assert!(cell_text(&Data::Error(calamine::CellErrorType::Ref)).starts_with('#'));
}

#[test]
fn file_names_are_made_safe() {
    assert_eq!(
        safe_name("C:\\Users\\x\\รายชื่อ 2569.xlsx"),
        "รายชื่อ 2569.xlsx"
    );
    assert_eq!(safe_name("../../etc/passwd"), "passwd");
    assert_eq!(safe_name(".."), "roster");
    assert_eq!(safe_name("a:b\n.csv"), "a_b_.csv");
    assert_eq!(safe_name(""), "roster");
    for n in ["x", "..", "a/b", "ok.csv"] {
        assert!(crate::check_file_key(&format!("roster/id/{}", safe_name(n))).is_ok());
    }
}

#[test]
fn cells_without_a_header_are_kept_by_column_and_txt_is_refused() {
    let c = test_config();
    let text = format!(
        "{},,\r\n68000001,นก,,th,,MSC,1,,ลืม,\r\n,,,,,,,,,,ท้าย\r\n",
        expected_headers(&c.roster).join(",")
    );
    let rows = parse(text.as_bytes(), "a.csv", &c.roster).unwrap();
    assert_eq!(rows[0].unheaded, vec!["I".to_string()]);
    assert_eq!(rows[1].unheaded, vec!["K".to_string()]);
    assert_eq!(rows[0].get("ชื่อ-สกุล"), Some("นก"));
    assert_eq!(
        parse(text.as_bytes(), "a.txt", &c.roster),
        Err(FileError::Unsupported)
    );
    assert_eq!(column_name(0), "A");
    assert_eq!(column_name(25), "Z");
    assert_eq!(column_name(26), "AA");
    assert_eq!(column_name(63), "BL");
}

#[test]
fn an_xlsx_is_told_by_its_name_only() {
    let c = test_config();
    assert_eq!(
        parse(XLSX, "roster.xlsx", &c.roster).map(|r| r.len()),
        Ok(50)
    );
    for name in ["roster.zip", "roster", "roster.XLSX.bak"] {
        assert_eq!(
            parse(XLSX, name, &c.roster),
            Err(FileError::Unsupported),
            "{name}"
        );
    }
    assert!(is_xlsx("A.XLSX") && !is_xlsx("a.csv"));
}

#[test]
fn a_file_may_leave_out_the_columns_that_may_be_empty() {
    // #154: email, Locale, Tags and the optional GPA left out.
    let c = test_config();
    let text = "รหัสนักศึกษา,ชื่อ-สกุล,หลักสูตร,ชั้นปี\n68010001,นก,MSC,1\n";
    let r = read(text.as_bytes(), "a.csv", &c.roster).unwrap();
    assert_eq!(r.rows.len(), 1);
    let gone = ["อีเมล", "ภาษา", "กลุ่ม", "เกรดเฉลี่ย"].map(String::from);
    assert_eq!(
        r.absent,
        vec![Absent {
            sheet: String::new(),
            columns: gone.to_vec()
        }]
    );
    assert_eq!(absent_in(&r.absent, ""), &gone[..]);
    // A whole file has nothing absent.
    let full = read(CSV, "roster.csv", &c.roster).unwrap();
    assert!(full.absent.is_empty());
    // A required attribute is still required.
    let text = "รหัสนักศึกษา,ชื่อ-สกุล,หลักสูตร\n68010001,นก,MSC\n";
    match read(text.as_bytes(), "a.csv", &c.roster) {
        Err(FileError::Headers(m)) => assert_eq!(m[0].missing, vec!["ชั้นปี".to_string()]),
        other => panic!("{other:?}"),
    }
}

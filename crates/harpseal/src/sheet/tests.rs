use super::*;

const FEES: &[u8] = include_bytes!("../../tests/fixtures/roster/fees.xlsx");
const COHORTS: &[u8] = include_bytes!("../../tests/fixtures/roster/cohorts.xlsx");

fn fees_spec() -> TableSpec {
    TableSpec::new(["รหัสนักศึกษา", "ชื่อ-สกุล", "ยอดค้างชำระ"])
}

#[test]
fn numbers_stored_as_text_read_as_one_number() {
    for s in ["12,500", " 12500 ", "12,500.00", "12500", "\u{a0}12,500\t"] {
        assert_eq!(number(s, Blank::Missing), Ok(Some(12500.0)), "{s:?}");
    }
    assert_eq!(number("1,234,567.5", Blank::Missing), Ok(Some(1_234_567.5)));
    assert_eq!(number("-1,250", Blank::Missing), Ok(Some(-1250.0)));
    assert_eq!(number("0.25", Blank::Missing), Ok(Some(0.25)));
    // `.` is only ever the decimal point: never twelve thousand.
    assert_eq!(number("12.500", Blank::Missing), Ok(Some(12.5)));
    for s in [
        "12.500.000",
        "12,5",
        "1,2345",
        "1234,567",
        ",500",
        "12,",
        "12.",
        ".5",
        "1e3",
        "NaN",
        "inf",
        "+5",
        "--5",
        "12 500",
        "฿12,500",
        "สิบ",
    ] {
        assert_eq!(number(s, Blank::Missing), Err(NotANumber), "{s:?}");
    }
}

#[test]
fn blank_and_dash_follow_the_callers_rule() {
    for s in ["", "  ", "-", " - "] {
        assert_eq!(number(s, Blank::Missing), Ok(None), "{s:?}");
        assert_eq!(number(s, Blank::Zero), Ok(Some(0.0)), "{s:?}");
    }
    assert_eq!(number("5", Blank::Zero), Ok(Some(5.0)));
}

#[test]
fn a_module_reads_its_own_upload_with_the_same_rules() {
    let book = Workbook::open(FEES, "fees.xlsx").unwrap();
    assert_eq!(book.sheets().len(), 1);
    let sheet = &book.sheets()[0];
    assert_eq!(sheet.name(), "ภาคเรียน 2");
    let table = sheet.table(&fees_spec()).unwrap();
    assert_eq!(table.header_row(), 3, "the title row is skipped");
    assert_eq!(table.footer(), &[12, 13], "the totals and notes rows");
    let rows = table.rows();
    assert_eq!(rows.len(), 8);
    assert_eq!(
        rows.iter().map(Row::number).collect::<Vec<_>>(),
        (4..=11).collect::<Vec<_>>()
    );
    let amount = |i: usize, blank| number(rows[i].get("ยอดค้างชำระ").unwrap_or(""), blank);
    for i in 0..4 {
        assert_eq!(amount(i, Blank::Missing), Ok(Some(12500.0)), "row {i}");
    }
    assert_eq!(amount(4, Blank::Missing), Ok(None), "-");
    assert_eq!(amount(4, Blank::Zero), Ok(Some(0.0)), "-");
    assert_eq!(amount(5, Blank::Zero), Ok(Some(0.0)), "blank");
    assert_eq!(rows[0].get("รหัสนักศึกษา"), Some("68010001"), "no .0");
    assert_eq!(rows[0].sheet(), "ภาคเรียน 2");
}

#[test]
fn excel_error_cells_are_unreadable_with_their_sheet_and_cell() {
    let book = Workbook::open(FEES, "fees.xlsx").unwrap();
    let table = book.sheets()[0].table(&fees_spec()).unwrap();
    let rows = table.rows();
    let (r, na) = (&rows[6], &rows[7]);
    assert_eq!(r.get("ยอดค้างชำระ"), None, "never the text of the code");
    assert_eq!(
        r.cell("ยอดค้างชำระ"),
        Some(&Cell::Unreadable {
            code: "#REF!".into(),
            cell: "C10".into()
        })
    );
    assert_eq!(
        na.unreadable().collect::<Vec<_>>(),
        vec![("ยอดค้างชำระ", "#N/A", "C11")]
    );
    assert_eq!((na.sheet(), na.number()), ("ภาคเรียน 2", 11));
    assert_eq!(rows[0].unreadable().count(), 0);
}

#[test]
fn every_sheet_is_read_and_an_empty_one_is_empty() {
    let book = Workbook::open(COHORTS, "cohorts.xlsx").unwrap();
    let names: Vec<&str> = book.sheets().iter().map(Sheet::name).collect();
    assert_eq!(names, vec!["ปี 1", "ปี 2", "ว่าง"]);
    assert!(book.sheets()[2].is_empty());
    let spec = TableSpec::new(["รหัสนักศึกษา", "ชื่อ-สกุล"]);
    let y1 = book.sheets()[0].table(&spec).unwrap();
    assert_eq!(
        (y1.header_row(), y1.rows().len(), y1.footer()),
        (4, 3, &[8][..])
    );
    let y2 = book.sheets()[1].table(&spec).unwrap();
    assert_eq!(
        (y2.header_row(), y2.rows().len(), y2.footer()),
        (1, 2, &[5][..])
    );
    let missing = book.sheets()[2].table(&spec).unwrap_err();
    assert_eq!((missing.sheet.as_str(), missing.row), ("ว่าง", 0));
}

#[test]
fn a_csv_is_one_unnamed_sheet_with_the_same_rules() {
    let text =
        "\u{feff}Seal Bay University\r\n\r\nid;amount\r\n68010001;\"1,250\"\r\nTotal;\"1,250\"\r\n";
    let book = Workbook::open(text.as_bytes(), "a.CSV").unwrap();
    let t = book.sheets()[0]
        .table(&TableSpec::new(["id", "amount"]))
        .unwrap();
    assert_eq!(book.sheets()[0].name(), "");
    assert_eq!((t.header_row(), t.footer()), (3, &[5][..]));
    assert_eq!(t.rows()[0].get("amount"), Some("1,250"));
}

#[test]
fn the_footer_never_swallows_a_data_row_or_an_unheaded_cell() {
    let spec = TableSpec::new(["id", "name", "year", "programme"]);
    let read = |body: &str| {
        let text = format!("id,name,year,programme\n68010001,Nok,1,MSC\n{body}");
        let book = Workbook::open(text.as_bytes(), "a.csv").unwrap();
        let t = book.sheets()[0].table(&spec).unwrap();
        (t.rows().len(), t.footer().to_vec())
    };
    assert_eq!(read("Total,1\n"), (1, vec![3]));
    assert_eq!(read(",Total: 1\nNote: draft\n"), (1, vec![3, 4]));
    assert_eq!(read(",Pla,2,MSC\n"), (2, vec![]), "a row missing its id");
    // The rule's edge: two cells and no key read as a footer.
    assert_eq!(read(",Pla,2\n"), (1, vec![3]));
    assert_eq!(read("6801000X,Pla\n"), (2, vec![]), "a mistyped id");
    assert_eq!(
        read(",,,,late\n"),
        (2, vec![]),
        "a cell right of the header"
    );
    assert_eq!(read("Total,1,,,x\n"), (2, vec![]));
    // Only rows below the last data row: a gap row in the middle stays.
    assert_eq!(read("Total,1\n68010002,Pla,2\n"), (3, vec![]));
    // A caller's own key rule: letter codes are keys.
    let letters = spec.clone().is_key(|s| !s.is_empty() && !s.contains(' '));
    let text = "id,name,year,programme\nABCD,Nok,1,MSC\nEFGH,Pla\n";
    let book = Workbook::open(text.as_bytes(), "a.csv").unwrap();
    assert_eq!(book.sheets()[0].table(&letters).unwrap().rows().len(), 2);
    assert_eq!(book.sheets()[0].table(&spec).unwrap().rows().len(), 1);
}

#[test]
fn a_header_is_searched_in_the_first_rows_only() {
    let spec = TableSpec::new(["id", "name"]);
    let titles = "title\n".repeat(HEADER_SEARCH_ROWS - 1);
    let found = format!("{titles}id,name\n1,a\n");
    let book = Workbook::open(found.as_bytes(), "a.csv").unwrap();
    assert_eq!(book.sheets()[0].table(&spec).unwrap().header_row(), 10);
    let late = format!("{titles}title\nid,name\n1,a\n");
    let book = Workbook::open(late.as_bytes(), "a.csv").unwrap();
    let e = book.sheets()[0].table(&spec).unwrap_err();
    assert_eq!((e.row, e.missing.len()), (1, 2), "closest: the first row");
    let near = "title\nid,nam\n";
    let e = Workbook::open(near.as_bytes(), "a.csv").unwrap().sheets()[0]
        .table(&spec)
        .unwrap_err();
    assert_eq!((e.row, e.missing), (2, vec!["name".to_string()]));
    assert_eq!(e.headers, vec!["id".to_string(), "nam".into()]);
}

#[test]
fn bad_files_and_limits_are_refused() {
    assert_eq!(Workbook::open(b"x", "a.pdf"), Err(SheetError::Unsupported));
    assert_eq!(Workbook::open(&[0xff], "a.csv"), Err(SheetError::NotUtf8));
    assert_eq!(
        Workbook::open(b"a\n\"x\n", "a.csv"),
        Err(SheetError::Csv { row: 2 })
    );
    assert_eq!(Workbook::open(b"PK", "a.xlsx"), Err(SheetError::Xlsx));
    let small = Limits::new(2, 64);
    let rows = "a\n".repeat(2 + Limits::SLACK + 1);
    assert_eq!(
        Workbook::open_with(rows.as_bytes(), "a.csv", small),
        Err(SheetError::TooManyRows)
    );
    let few = Limits::new(1, 2);
    assert_eq!(
        Workbook::open_with(COHORTS, "c.xlsx", few),
        Err(SheetError::TooManyColumns)
    );
    // A sheet may hold SLACK rows beyond the data limit (titles, footer).
    let fits = "a\n".repeat(2 + Limits::SLACK);
    assert!(Workbook::open_with(fits.as_bytes(), "a.csv", small).is_ok());
}

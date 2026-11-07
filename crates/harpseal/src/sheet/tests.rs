use super::*;

const FEES: &[u8] = include_bytes!("../../tests/fixtures/roster/fees.xlsx");
const COHORTS: &[u8] = include_bytes!("../../tests/fixtures/roster/cohorts.xlsx");
const TRAILING: &[u8] = include_bytes!("../../tests/fixtures/roster/cohorts_trailing.xlsx");
const CALC_FEES: &[u8] = include_bytes!("../../tests/fixtures/roster/calculated_fees.xlsx");
const LONG: &[u8] = include_bytes!("../../tests/fixtures/roster/long_numbers.xlsx");
const HIDDEN: &[u8] = include_bytes!("../../tests/fixtures/roster/cohorts_hidden.xlsx");

const AMOUNT: &str = "ยอดค้างชำระ";

fn fees_spec() -> TableSpec {
    TableSpec::new(["รหัสนักศึกษา", "ชื่อ-สกุล", AMOUNT]).skip_footer([AMOUNT])
}

/// `(data row numbers, skipped row numbers)` of a CSV under `spec`.
fn split(text: &str, spec: &TableSpec) -> (Vec<usize>, Vec<usize>) {
    let book = Workbook::open(text.as_bytes(), "a.csv").unwrap();
    let t = book.sheets()[0].table(spec).unwrap();
    (
        t.rows().iter().map(Row::number).collect(),
        t.footer().iter().map(|s| s.row).collect(),
    )
}

#[test]
fn a_module_reads_its_own_upload_with_the_same_rules() {
    let book = Workbook::open(FEES, "fees.xlsx").unwrap();
    assert_eq!(book.sheets().len(), 1);
    let sheet = &book.sheets()[0];
    assert_eq!(sheet.name(), "ภาคเรียน 2");
    let table = sheet.table(&fees_spec()).unwrap();
    assert_eq!(table.header_row(), 3, "the title row is skipped");
    let footer: Vec<_> = table
        .footer()
        .iter()
        .map(|s| (s.row, s.text.as_str()))
        .collect();
    assert_eq!(
        footer,
        vec![
            (12, "รวม | 50000"),
            (13, "หมายเหตุ: ยอด ณ วันที่ 1 พฤศจิกายน 2569")
        ],
        "the totals and notes rows, with their text"
    );
    let rows = table.rows();
    assert_eq!(
        rows.iter().map(Row::number).collect::<Vec<_>>(),
        (4..=11).collect::<Vec<_>>()
    );
    let satang = |i: usize| number_scaled(rows[i].get(AMOUNT).unwrap_or(""), 2, Blank::Zero);
    for i in 0..4 {
        assert_eq!(satang(i), Ok(Some(1_250_000)), "row {i}");
    }
    assert_eq!(satang(4), Ok(Some(0)), "-");
    assert_eq!(satang(5), Ok(Some(0)), "blank");
    assert_eq!(rows[0].get("รหัสนักศึกษา"), Some("68010001"), "no .0");
    assert_eq!(rows[0].sheet(), "ภาคเรียน 2");
}

#[test]
fn excel_error_cells_are_unreadable_with_their_sheet_and_cell() {
    let book = Workbook::open(FEES, "fees.xlsx").unwrap();
    let table = book.sheets()[0].table(&fees_spec()).unwrap();
    let rows = table.rows();
    let (r, na) = (&rows[6], &rows[7]);
    assert_eq!(r.get(AMOUNT), None, "never the text of the code");
    assert_eq!(
        r.cell(AMOUNT),
        Some(&Cell::Unreadable {
            code: "#REF!".into(),
            cell: "C10".into()
        })
    );
    assert_eq!(
        na.unreadable().collect::<Vec<_>>(),
        vec![(AMOUNT, "#N/A", "C11")]
    );
    assert_eq!((na.sheet(), na.number()), ("ภาคเรียน 2", 11));
}

#[test]
fn without_skip_footer_nothing_is_dropped() {
    let spec = TableSpec::new(["id", "name", "amount"]);
    let text = "id,name,amount\n68010001,Nok,1\n,Pla,\"12,500\"\n\
                Total,,\"12,501\"\nNote: draft\n";
    assert_eq!(split(text, &spec), (vec![2, 3, 4, 5], vec![]));
    // The same rows with the footer on: the totals and the note go.
    let on = spec.skip_footer(["amount"]);
    assert_eq!(split(text, &on), (vec![2, 3], vec![4, 5]));
    // Names as keys, footer off: no row is lost.
    let names = "Name,Amount\nNok Talay,\"1,000\"\nPla Thong,500\n";
    assert_eq!(
        split(names, &TableSpec::new(["Name", "Amount"])),
        (vec![2, 3], vec![])
    );
}

#[test]
fn the_footer_never_swallows_a_data_row() {
    let spec = TableSpec::new(["id", "name", "amount"]).skip_footer(["amount"]);
    let read = |body: &str| split(&format!("id,name,amount\n68010001,Nok,1\n{body}"), &spec);
    // Totals (values under numeric columns only) and notes are footers.
    assert_eq!(read("Total,,\"12,500\"\n"), (vec![2], vec![3]));
    assert_eq!(
        read("รวม 2 คน\nหมายเหตุ: 1 พ.ย. 2569\n"),
        (vec![2], vec![3, 4])
    );
    // No label, only numbers: a row whose id was lost, data.
    assert_eq!(read(",,\"12,500\"\n"), (vec![2, 3], vec![]), "no label");
    // A value under a required non-numeric column keeps the row as data.
    assert_eq!(
        read(",Pla,\"12,500\"\n"),
        (vec![2, 3], vec![]),
        "id missing"
    );
    assert_eq!(
        read(",Pla,\"12,500\"\n,Kung,\"10,000\"\nTotal,,\"22,500\"\n"),
        (vec![2, 3, 4], vec![5]),
        "two id-less students, then the totals"
    );
    assert_eq!(read(",Pla\n"), (vec![2, 3], vec![]));
    // Digits without letters in the key column: a malformed id, data.
    assert_eq!(read("6801 0002,,\"12,500\"\n"), (vec![2, 3], vec![]));
    assert_eq!(read("68-01,,5\n"), (vec![2, 3], vec![]));
    // A cell outside the header is reported, never skipped.
    assert_eq!(read("Total,,5,x\n"), (vec![2, 3], vec![]));
    // Only rows below the last data row.
    assert_eq!(read("Total,,1\n68010002,Pla,2\n"), (vec![2, 3, 4], vec![]));
}

#[test]
fn an_unreadable_key_and_an_id_less_student_in_xlsx_stay_data() {
    let book = Workbook::open(TRAILING, "t.xlsx").unwrap();
    let spec = TableSpec::new(["รหัสนักศึกษา", "ชื่อ-สกุล"]).skip_footer(["ชั้นปี"]);
    let t = book.sheets()[0].table(&spec).unwrap();
    let rows: Vec<usize> = t.rows().iter().map(Row::number).collect();
    assert_eq!(rows, vec![5, 6, 7], "row 6 has no id, row 7 a #REF! id");
    assert_eq!(t.rows()[2].unreadable().count(), 1);
    assert_eq!(
        t.footer().iter().map(|s| s.row).collect::<Vec<_>>(),
        vec![8]
    );
}

#[test]
fn a_table_keyed_by_names_or_codes_brings_its_own_key_test() {
    let names = "Name,Amount\nNok Talay,\"1,000\"\nPla Thong,500\n";
    // Without the footer, every row is data whatever the key looks like.
    let plain = TableSpec::new(["Name", "Amount"]);
    assert_eq!(split(names, &plain), (vec![2, 3], vec![]));
    // With it, the caller says what a key is: here, a known name.
    let known = ["Nok Talay", "Pla Thong"];
    let spec = TableSpec::new(["Name", "Amount"])
        .is_key(move |s| known.contains(&s))
        .skip_footer(["Amount"]);
    let text = format!("{names}Total,\"1,500\"\n");
    assert_eq!(split(&text, &spec), (vec![2, 3], vec![4]));
    let codes = TableSpec::new(["Code", "Amount"])
        .is_key(|s| s.len() == 2 && s.bytes().all(|b| b.is_ascii_uppercase()))
        .skip_footer(["Amount"]);
    let text = "Code,Amount\nAB,1\nCD,2\nรวม,3\n";
    assert_eq!(split(text, &codes), (vec![2, 3], vec![4]));
}

#[test]
fn a_repeated_header_is_an_error_or_its_extra_cells_are_unheaded() {
    let spec = TableSpec::new(["Student id", "Amount"]);
    let text = "Student id,Amount,Amount\n68010001,100,200\n";
    let book = Workbook::open(text.as_bytes(), "a.csv").unwrap();
    let e = book.sheets()[0]
        .table(&spec)
        .unwrap_err()
        .header()
        .cloned()
        .unwrap();
    assert_eq!((e.row, e.repeated.clone()), (1, vec!["Amount".to_string()]));
    assert!(e.missing.is_empty());
    // A repeated column the spec does not name: first wins, the rest are
    // unheaded (reported by column), never overwritten.
    let text = "Student id,Amount,Note,Note\n68010001,100,a,b\n";
    let book = Workbook::open(text.as_bytes(), "a.csv").unwrap();
    let t = book.sheets()[0].table(&spec).unwrap();
    assert_eq!(t.rows()[0].get("Note"), Some("a"));
    assert_eq!(t.rows()[0].unheaded(), &["D".to_string()]);
}

#[test]
fn every_sheet_is_read_and_hidden_ones_are_marked() {
    let book = Workbook::open(COHORTS, "cohorts.xlsx").unwrap();
    let names: Vec<&str> = book.sheets().iter().map(Sheet::name).collect();
    assert_eq!(names, vec!["ปี 1", "ปี 2", "ว่าง"]);
    assert!(book.sheets()[2].is_empty());
    assert!(book.sheets().iter().all(|s| !s.is_hidden()));
    let spec = TableSpec::new(["รหัสนักศึกษา", "ชื่อ-สกุล"]).skip_footer(["ชั้นปี"]);
    let y1 = book.sheets()[0].table(&spec).unwrap();
    assert_eq!(
        (y1.header_row(), y1.rows().len(), y1.footer()[0].row),
        (4, 3, 8)
    );
    let y2 = book.sheets()[1].table(&spec).unwrap();
    assert_eq!(
        (y2.header_row(), y2.rows().len(), y2.footer()[0].row),
        (1, 2, 5)
    );
    let missing = book.sheets()[2]
        .table(&spec)
        .unwrap_err()
        .header()
        .cloned()
        .unwrap();
    assert_eq!((missing.sheet.as_str(), missing.row), ("ว่าง", 0));
    let hidden = Workbook::open(HIDDEN, "h.xlsx").unwrap();
    let flags: Vec<_> = hidden
        .sheets()
        .iter()
        .map(|s| (s.name(), s.is_hidden()))
        .collect();
    assert_eq!(flags, vec![("ปี 2", false), ("lists", true)]);
}

#[test]
fn a_csv_is_one_unnamed_sheet_with_the_same_rules() {
    let text =
        "\u{feff}Seal Bay University\r\n\r\nid;amount\r\n68010001;\"1,250\"\r\nTotal;\"1,250\"\r\n";
    let spec = TableSpec::new(["id", "amount"]).skip_footer(["amount"]);
    let book = Workbook::open(text.as_bytes(), "a.CSV").unwrap();
    let t = book.sheets()[0].table(&spec).unwrap();
    assert_eq!(book.sheets()[0].name(), "");
    assert_eq!((t.header_row(), t.footer()[0].row), (3, 5));
    assert_eq!(t.rows()[0].get("amount"), Some("1,250"));
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
    let e = book.sheets()[0]
        .table(&spec)
        .unwrap_err()
        .header()
        .cloned()
        .unwrap();
    assert_eq!((e.row, e.missing.len()), (1, 2), "closest: the first row");
    let near = "title\nid,nam\n";
    let e = Workbook::open(near.as_bytes(), "a.csv").unwrap().sheets()[0]
        .table(&spec)
        .unwrap_err()
        .header()
        .cloned()
        .unwrap();
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
    let fits = "a\n".repeat(2 + Limits::SLACK);
    assert!(Workbook::open_with(fits.as_bytes(), "a.csv", small).is_ok());
    assert_eq!(
        Workbook::open_with(COHORTS, "c.xlsx", Limits::new(1, 2)),
        Err(SheetError::TooManyColumns)
    );
    // Sheets are counted before any is read: cohorts.xlsx has three.
    let two = Limits::default().max_sheets(2);
    assert_eq!(
        Workbook::open_with(COHORTS, "c.xlsx", two),
        Err(SheetError::TooManySheets)
    );
    assert_eq!(Limits::default().max_sheets, 50);
    assert!(Workbook::open_with(COHORTS, "c.xlsx", Limits::default().max_sheets(3)).is_ok());
}

#[test]
fn every_headed_column_counts_not_only_the_required_ones() {
    // The spec requires id and amount; the file also has a Name column.
    let spec = TableSpec::new(["Student id", "Amount"]).skip_footer(["Amount"]);
    let read = |body: &str| {
        let text = format!("Student id,Name,Amount\n68010001,Nok,1\n{body}");
        split(&text, &spec)
    };
    assert_eq!(read(",Pla,\"12,500\"\n"), (vec![2, 3], vec![]));
    assert_eq!(read("Total,,\"12,500\"\n"), (vec![2], vec![3]));
    assert_eq!(read(",,\"12,500\"\n"), (vec![2, 3], vec![]));
    // Known limit, documented: a name typed in the id column reads as a
    // label (`Total` and `Pla Thong` look alike); it is listed, not lost.
    assert_eq!(read("Pla Thong,,\"12,500\"\n"), (vec![2], vec![3]));
}

#[test]
fn a_footer_that_would_swallow_every_row_is_an_error() {
    let spec = TableSpec::new(["id", "amount"]).skip_footer(["amount"]);
    let book = Workbook::open(b"id,amount\nTotal,5\nNote: none\n", "a.csv").unwrap();
    match book.sheets()[0].table(&spec) {
        Err(TableError::OnlyFooter { sheet, rows }) => {
            assert_eq!(sheet, "");
            assert_eq!(rows.iter().map(|r| r.row).collect::<Vec<_>>(), vec![2, 3]);
        }
        other => panic!("{other:?}"),
    }
    // A header and nothing under it is an empty table, not an error.
    let book = Workbook::open(b"id,amount\n", "a.csv").unwrap();
    assert!(book.sheets()[0].table(&spec).unwrap().rows().is_empty());
}

#[test]
fn calculated_amounts_read_as_excel_shows_them() {
    let book = Workbook::open(CALC_FEES, "c.xlsx").unwrap();
    let t = book.sheets()[0].table(&fees_spec()).unwrap();
    let satang: Vec<_> = t
        .rows()
        .iter()
        .map(|r| number_scaled(r.get(AMOUNT).unwrap_or(""), 2, Blank::Missing))
        .collect();
    // 12500.000000000002 and 0.30000000000000004 read exactly; a third
    // of a baht has no exact satang and is refused, never rounded.
    assert_eq!(
        satang,
        vec![Ok(Some(1_250_000)), Ok(Some(30)), Err(NotANumber)]
    );
    let third = number(t.rows()[2].get(AMOUNT).unwrap_or(""), Blank::Missing);
    assert_eq!(third, Ok(Some(3.33333333333333)));
    assert_eq!(t.footer()[0].text, "รวม | 12503.6333333333");
}

#[test]
fn long_whole_numbers_stay_exact_or_are_unreadable() {
    let book = Workbook::open(LONG, "l.xlsx").unwrap();
    let spec = TableSpec::new(["รหัสอ้างอิง", "ยอด"]);
    let t = book.sheets()[0].table(&spec).unwrap();
    let rows = t.rows();
    assert_eq!(rows[0].get("รหัสอ้างอิง"), Some("1234567890123456"));
    assert_eq!(rows[1].get("รหัสอ้างอิง"), Some("9007199254740991"));
    // 2^53 and 2^53 + 1 read as the same f64: neither is printed.
    for (r, cell) in [(&rows[2], "A4"), (&rows[3], "A5")] {
        assert_eq!(r.get("รหัสอ้างอิง"), None);
        assert_eq!(
            r.unreadable().collect::<Vec<_>>(),
            vec![("รหัสอ้างอิง", TOO_LONG, cell)]
        );
    }
}

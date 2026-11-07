use crate::sheet::*;

#[test]
fn a_header_and_a_zero_total_is_an_empty_cohort() {
    let spec = TableSpec::new(["id", "amount"]).skip_footer(["amount"]);
    for text in ["id,amount\nรวม,0\n", "id,amount\nรวม,-\nหมายเหตุ: ไม่มี\n"]
    {
        let book = Workbook::open(text.as_bytes(), "a.csv").unwrap();
        let t = book.sheets()[0].table(&spec).unwrap();
        assert!(t.rows().is_empty(), "{text}");
        assert!(!t.footer().is_empty(), "still listed: {text}");
    }
}

#[test]
fn a_footer_total_can_be_checked_against_the_data() {
    let spec = TableSpec::new(["id", "name", "amount"]).skip_footer(["amount"]);
    let check = |text: &str| {
        let book = Workbook::open(text.as_bytes(), "a.csv").unwrap();
        book.sheets()[0]
            .table(&spec)
            .unwrap()
            .check_footer_total("amount", 2)
    };
    let head = "id,name,amount\n68010001,Nok,\"1,000\"\n68010002,Pla,500.50\n";
    assert_eq!(
        check(&format!("{head}รวม,,\"1,500.50\"\nหมายเหตุ: -\n")),
        Ok(())
    );
    assert_eq!(check(head), Ok(()), "no footer, nothing to check");
    // A name typed in the id column reads as a footer label; the total
    // gives it away.
    let text = "id,name,amount\n68010001,Nok,\"1,000\"\nPla Thong,,\"12,500\"\nรวม,,\"13,500\"\n";
    match check(text) {
        Err(TotalError::Mismatch {
            row, footer, data, ..
        }) => {
            assert_eq!((row, footer, data), (3, 1_250_000, 100_000));
        }
        other => panic!("{other:?}"),
    }
    let bad = "id,name,amount\n68010001,Nok,abc\nรวม,,1\n";
    assert!(matches!(
        check(bad),
        Err(TotalError::NotANumber { row: 2, .. })
    ));
}

use super::*;

#[test]
fn numbers_stored_as_text_read_as_one_number() {
    for s in [
        "12,500",
        " 12500 ",
        "12,500.00",
        "12500",
        "\u{a0}12,500\t",
        "+12,500",
    ] {
        assert_eq!(number(s, Blank::Missing), Ok(Some(12500.0)), "{s:?}");
    }
    assert_eq!(number("1,234,567.5", Blank::Missing), Ok(Some(1_234_567.5)));
    assert_eq!(number("-1,250", Blank::Missing), Ok(Some(-1250.0)));
    assert_eq!(number(".5", Blank::Missing), Ok(Some(0.5)));
    assert_eq!(number("5.", Blank::Missing), Ok(Some(5.0)));
    // `.` is only ever the decimal point: never twelve thousand.
    assert_eq!(number("12.500", Blank::Missing), Ok(Some(12.5)));
    for s in [
        "12.500.000",
        "12,5",
        "1,2345",
        "1234,567",
        "0,500",
        "00,500",
        ",500",
        "12,",
        ".",
        "+",
        "1e3",
        "NaN",
        "inf",
        "--5",
        "12 500",
        "12\u{a0}500",
        "(500)",
        "฿12,500",
        "๑๒",
        "\u{2212}5",
        "สิบ",
    ] {
        assert_eq!(number(s, Blank::Missing), Err(NotANumber), "{s:?}");
    }
}

#[test]
fn the_f64_path_refuses_what_it_cannot_hold_exactly() {
    assert_eq!(
        number("999,999,999,999,999", Blank::Missing),
        Ok(Some(999_999_999_999_999.0))
    );
    for s in [
        "99,999,999,999,999,999.99",
        "1234567890123456",
        "0.1234567890123456",
    ] {
        assert_eq!(number(s, Blank::Missing), Err(NotANumber), "{s:?}");
    }
    // Zeros around the digits do not count.
    assert_eq!(
        number("000123456789012345.000", Blank::Missing),
        Ok(Some(123_456_789_012_345.0))
    );
}

#[test]
fn money_reads_exactly_in_its_smallest_unit() {
    let satang = |s| number_scaled(s, 2, Blank::Missing);
    assert_eq!(satang("12,500"), Ok(Some(1_250_000)));
    assert_eq!(satang("12,500.5"), Ok(Some(1_250_050)));
    assert_eq!(satang("0.29"), Ok(Some(29)), "never 28.999…");
    assert_eq!(satang("-0.29"), Ok(Some(-29)));
    assert_eq!(satang("1.500"), Ok(Some(150)), "trailing zeros are exact");
    // 17 digits an f64 rounds (to 1e17) read exactly.
    assert_eq!(
        satang("9,999,999,999,999,999.99"),
        Ok(Some(999_999_999_999_999_999))
    );
    assert_eq!(satang("92,233,720,368,547,758.07"), Ok(Some(i64::MAX)));
    assert_eq!(satang("92,233,720,368,547,758.08"), Err(NotANumber), "i64");
    assert_eq!(satang("99,999,999,999,999,999.99"), Err(NotANumber));
    assert_eq!(satang("1.005"), Err(NotANumber), "never rounded");
    assert_eq!(satang("12,5"), Err(NotANumber));
    assert_eq!(
        number_scaled("12,500.00", 0, Blank::Missing),
        Ok(Some(12500))
    );
    assert_eq!(
        number_scaled("9007199254740993", 0, Blank::Missing),
        Ok(Some(9_007_199_254_740_993))
    );
    assert_eq!(
        number_scaled("3.00000000000000001", 0, Blank::Missing),
        Err(NotANumber)
    );
}

#[test]
fn blank_and_dash_follow_the_callers_rule() {
    for s in ["", "  ", "-", " - "] {
        assert_eq!(number(s, Blank::Missing), Ok(None), "{s:?}");
        assert_eq!(number(s, Blank::Zero), Ok(Some(0.0)), "{s:?}");
        assert_eq!(number_scaled(s, 2, Blank::Missing), Ok(None), "{s:?}");
        assert_eq!(number_scaled(s, 2, Blank::Zero), Ok(Some(0)), "{s:?}");
    }
    assert_eq!(number("5", Blank::Zero), Ok(Some(5.0)));
}

#[test]
fn a_scale_beyond_what_an_i64_holds_is_refused_not_a_panic() {
    assert_eq!(number_scaled("1", 70_000, Blank::Missing), Err(NotANumber));
    assert_eq!(
        number_scaled("1", MAX_SCALE + 1, Blank::Missing),
        Err(NotANumber)
    );
    assert_eq!(
        number_scaled("1", MAX_SCALE, Blank::Missing),
        Ok(Some(1_000_000_000_000_000_000))
    );
    assert_eq!(number_scaled("", 70_000, Blank::Zero), Err(NotANumber));
    // ±i64::MAX: i64::MIN itself is one short, documented.
    assert_eq!(
        number_scaled("-9223372036854775807", 0, Blank::Missing),
        Ok(Some(-i64::MAX))
    );
    assert_eq!(
        number_scaled("-9223372036854775808", 0, Blank::Missing),
        Err(NotANumber)
    );
}

//! Numbers stored as text (#70): `12,500`, ` 12500 ` and `12,500.00` are
//! the same number.

/// What a blank cell or a lone `-` means in a number column.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Blank {
    /// Zero (an amount nobody owes).
    Zero,
    /// No value (`Ok(None)`).
    Missing,
}

/// The text is not a number [`number`] reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NotANumber;

impl std::fmt::Display for NotANumber {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("not a number")
    }
}

impl std::error::Error for NotANumber {}

/// Reads a number written the Thai and English way: trimmed, an optional
/// leading `-`, digits with optional `,` thousands separators (groups of
/// three after the first), and an optional `.` decimal part. A blank cell
/// or a lone `-` follows `blank`. Anything else is refused, including
/// `12.500.000` and `12,5`: `.` is only ever the decimal point, so
/// `12.500` is twelve and a half, never twelve thousand five hundred.
///
/// ```
/// use harpseal::sheet::{number, Blank};
///
/// for s in ["12,500", " 12500 ", "12,500.00"] {
///     assert_eq!(number(s, Blank::Missing), Ok(Some(12500.0)));
/// }
/// assert_eq!(number("-", Blank::Zero), Ok(Some(0.0)));
/// assert_eq!(number("", Blank::Missing), Ok(None));
/// assert!(number("12,5", Blank::Missing).is_err());
/// ```
pub fn number(raw: &str, blank: Blank) -> Result<Option<f64>, NotANumber> {
    let s = raw.trim();
    if s.is_empty() || s == "-" {
        return Ok(match blank {
            Blank::Zero => Some(0.0),
            Blank::Missing => None,
        });
    }
    let (sign, body) = match s.strip_prefix('-') {
        Some(rest) => ("-", rest),
        None => ("", s),
    };
    let (whole, fraction) = match body.split_once('.') {
        Some((w, f)) => (w, Some(f)),
        None => (body, None),
    };
    if !grouped(whole) || fraction.is_some_and(|f| f.is_empty() || !all_digits(f)) {
        return Err(NotANumber);
    }
    let plain = format!(
        "{sign}{}{}",
        whole.replace(',', ""),
        fraction.map(|f| format!(".{f}")).unwrap_or_default()
    );
    plain
        .parse::<f64>()
        .ok()
        .filter(|n| n.is_finite())
        .map(Some)
        .ok_or(NotANumber)
}

/// `12500` or `12,500`: digits, or groups of three after a first group of
/// one to three.
fn grouped(whole: &str) -> bool {
    let mut groups = whole.split(',');
    let first = groups.next().unwrap_or_default();
    if !whole.contains(',') {
        return all_digits(first);
    }
    (1..=3).contains(&first.len())
        && all_digits(first)
        && groups.all(|g| g.len() == 3 && all_digits(g))
}

fn all_digits(s: &str) -> bool {
    !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit())
}

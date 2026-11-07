//! Numbers stored as text (#70): `12,500`, ` 12500 ` and `12,500.00` are
//! the same number.
//!
//! Accepted, after trimming white space at the ends: an optional `-` or
//! `+`, digits with optional `,` thousands separators (a first group of one
//! to three digits not starting with `0`, then groups of exactly three),
//! and an optional `.` decimal part (`.5` and `5.` read as 0.5 and 5).
//! `.` is only ever the decimal point: `12.500` is twelve and a half.
//!
//! Refused (normalise first if your files hold them): `12,5`, `0,500`,
//! `12.500.000`, exponents (`1e3`), `NaN` / `inf`, accounting negatives
//! `(500)`, currency signs (`฿`, `$`), Thai digits (`๑๒`), a space or
//! no-break space inside the number (`12 500`), the minus sign U+2212.

/// What a blank cell or a lone `-` means in a number column.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Blank {
    /// Zero (an amount nobody owes).
    Zero,
    /// No value (`Ok(None)`).
    Missing,
}

/// The text is not a number this module reads, or not exactly.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NotANumber;

impl std::fmt::Display for NotANumber {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("not a number")
    }
}

impl std::error::Error for NotANumber {}

/// Largest scale [`number_scaled`] takes: `10^18` still fits an `i64`.
pub const MAX_SCALE: u32 = 18;

/// Most significant digits [`number`] accepts: an `f64` holds 15 exactly.
pub const MAX_F64_DIGITS: usize = 15;

/// Reads a number as an `f64` (see the module docs for the format). A
/// blank cell or a lone `-` follows `blank`. More than
/// [`MAX_F64_DIGITS`] significant digits are refused rather than rounded;
/// for money use [`number_scaled`], which is exact.
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
    let Some(p) = parts(raw)? else {
        return Ok(blank_value(blank, 0.0));
    };
    let digits = format!("{}{}", p.whole, p.fraction);
    let significant = digits.trim_start_matches('0').trim_end_matches('0');
    if significant.len() > MAX_F64_DIGITS {
        return Err(NotANumber);
    }
    let plain = format!(
        "{}{}.{}0",
        if p.negative { "-" } else { "" },
        if p.whole.is_empty() { "0" } else { &p.whole },
        p.fraction
    );
    plain
        .parse::<f64>()
        .ok()
        .filter(|n| n.is_finite())
        .map(Some)
        .ok_or(NotANumber)
}

/// Reads a number exactly, as an integer count of `10^-scale` units:
/// `number_scaled("12,500.50", 2, ..)` is `1_250_050` satang. Digits
/// beyond `scale` must be zeros (`12.505` at scale 2 is refused, never
/// rounded), and a value that does not fit an `i64` is refused (also
/// `i64::MIN` itself: the range is `±i64::MAX`). Scale 0 reads whole
/// numbers (`12,500.00` is `12500`, `3.5` is refused); a scale above
/// [`MAX_SCALE`] is refused for any input.
///
/// ```
/// use harpseal::sheet::{number_scaled, Blank};
///
/// assert_eq!(number_scaled("12,500.5", 2, Blank::Missing), Ok(Some(1_250_050)));
/// assert_eq!(number_scaled("0.29", 2, Blank::Missing), Ok(Some(29)));
/// assert_eq!(number_scaled("-", 2, Blank::Zero), Ok(Some(0)));
/// assert!(number_scaled("1.005", 2, Blank::Missing).is_err());
/// ```
pub fn number_scaled(raw: &str, scale: u32, blank: Blank) -> Result<Option<i64>, NotANumber> {
    if scale > MAX_SCALE {
        return Err(NotANumber);
    }
    let Some(p) = parts(raw)? else {
        return Ok(blank_value(blank, 0));
    };
    let scale = scale as usize;
    let (kept, beyond) = p.fraction.split_at(p.fraction.len().min(scale));
    if beyond.bytes().any(|b| b != b'0') {
        return Err(NotANumber);
    }
    let digits = format!("{}{kept:0<scale$}", p.whole);
    let mut n: i64 = 0;
    for b in digits.bytes() {
        n = n
            .checked_mul(10)
            .and_then(|n| n.checked_add(i64::from(b - b'0')))
            .ok_or(NotANumber)?;
    }
    Ok(Some(if p.negative { -n } else { n }))
}

fn blank_value<T>(blank: Blank, zero: T) -> Option<T> {
    match blank {
        Blank::Zero => Some(zero),
        Blank::Missing => None,
    }
}

/// A number's sign and digit strings (separators removed).
struct Parts {
    negative: bool,
    whole: String,
    fraction: String,
}

/// `Ok(None)` for blank or a lone `-`.
fn parts(raw: &str) -> Result<Option<Parts>, NotANumber> {
    let s = raw.trim();
    if s.is_empty() || s == "-" {
        return Ok(None);
    }
    let (negative, body) = match s.as_bytes()[0] {
        b'-' => (true, &s[1..]),
        b'+' => (false, &s[1..]),
        _ => (false, s),
    };
    let (whole, fraction) = body.split_once('.').unwrap_or((body, ""));
    let digits = |t: &str| t.bytes().all(|b| b.is_ascii_digit());
    if (whole.is_empty() && fraction.is_empty()) || !grouped(whole) || !digits(fraction) {
        return Err(NotANumber);
    }
    Ok(Some(Parts {
        negative,
        whole: whole.replace(',', ""),
        fraction: fraction.into(),
    }))
}

/// `12500`, `12,500` or empty: digits, or a first group of one to three
/// digits not starting with `0`, then groups of three.
fn grouped(whole: &str) -> bool {
    let digits = |t: &str| t.bytes().all(|b| b.is_ascii_digit());
    if !whole.contains(',') {
        return digits(whole);
    }
    let mut groups = whole.split(',');
    let first = groups.next().unwrap_or_default();
    (1..=3).contains(&first.len())
        && digits(first)
        && !first.starts_with('0')
        && groups.all(|g| g.len() == 3 && digits(g))
}

#[cfg(test)]
#[path = "number_tests.rs"]
mod tests;

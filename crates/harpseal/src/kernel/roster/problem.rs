//! One problem of a roster file (#8, #71, #134), and the bad value it
//! echoes (#154).

use serde::{Deserialize, Serialize};

use super::warnings::Severity;

/// One problem of a file. `reason` is a code; the text is
/// `kernel.roster.reason_<code>`. `row` 0 is the file (or the sheet) as a
/// whole. `sheet` names the XLSX sheet (empty for a CSV) and `cell` the
/// cell (`H2`) when one cell is at fault; both are left out of the stored
/// JSON when empty, so reports stored before #70 read unchanged. So is
/// `severity` (#71) when it is `blocking`, which every older problem is.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Problem {
    pub(crate) row: usize,
    pub(crate) column: String,
    pub(crate) reason: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub(crate) sheet: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub(crate) cell: String,
    #[serde(default, skip_serializing_if = "Severity::is_blocking")]
    pub(crate) severity: Severity,
    /// Cells this problem stands for when it is one of several of a
    /// column (`empty_attribute`, #71); 0 for a problem of one place.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub(crate) count: u64,
    /// The first rows of such a problem.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) rows: Vec<usize>,
    /// The Student a problem of a warning class is about (#134): student
    /// code and name as the row holds them; empty for other problems.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub(crate) student: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub(crate) name: String,
    /// The cell's value as the file holds it (#154), cut to
    /// [`VALUE_CHARS`]: what Staff look for in the file. Empty for a
    /// problem of no one cell, or an empty one. Cleared with the Student
    /// a year on (`prune.rs`).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub(crate) value: String,
}

/// Longest bad value a problem echoes, in characters (then `…`).
pub(crate) const VALUE_CHARS: usize = 40;

/// `s` cut to `max` characters, with `…` when cut; control characters
/// (a line break in a cell) read as spaces.
/// Trimmed after that, so a value never starts with a space: the CSV
/// report guards a cell by its first character (`=`, `+`, `-`, `@`), and
/// ` =…` would slip past it.
pub(crate) fn shorten(s: &str, max: usize) -> String {
    let clean: String = s
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    let clean = clean.trim();
    if clean.chars().count() <= max {
        return clean.to_string();
    }
    let mut out: String = clean.chars().take(max.saturating_sub(1)).collect();
    out.push('…');
    out
}

fn is_zero(n: &u64) -> bool {
    *n == 0
}

impl Problem {
    pub(crate) fn new(row: usize, column: &str, reason: &str) -> Self {
        Self {
            row,
            column: column.into(),
            reason: reason.into(),
            sheet: String::new(),
            cell: String::new(),
            severity: Severity::Blocking,
            count: 0,
            rows: Vec::new(),
            student: String::new(),
            name: String::new(),
            value: String::new(),
        }
    }

    /// The same problem, echoing the cell's value `v` (shortened).
    pub(crate) fn showing(mut self, v: &str) -> Self {
        self.value = shorten(v.trim(), VALUE_CHARS);
        self
    }

    /// The same problem, naming the Student by `code` and `name`.
    pub(crate) fn about(mut self, code: &str, name: &str) -> Self {
        self.student = code.into();
        self.name = name.into();
        self
    }

    /// How many cells it counts as in a total: [`Problem::count`], else 1.
    pub(crate) fn weight(&self) -> u64 {
        self.count.max(1)
    }

    /// The same problem as a warning unless `blocks`.
    pub(crate) fn with_severity(mut self, blocks: bool) -> Self {
        self.severity = if blocks {
            Severity::Blocking
        } else {
            Severity::Warning
        };
        self
    }

    /// The same problem on `sheet`.
    pub(crate) fn on(mut self, sheet: &str) -> Self {
        self.sheet = sheet.into();
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_long_value_is_cut_and_a_line_break_reads_as_a_space() {
        assert_eq!(shorten("6801OO05", VALUE_CHARS), "6801OO05");
        let long = "ก".repeat(50);
        let cut = shorten(&long, VALUE_CHARS);
        assert_eq!(cut.chars().count(), VALUE_CHARS);
        assert!(cut.ends_with('…'));
        assert_eq!(shorten("a\nb", 10), "a b");
        // A line break before a formula is no space before it.
        assert_eq!(shorten("\n=HYPERLINK(1)", 40), "=HYPERLINK(1)");
        assert_eq!(shorten("\t\r+1", 40), "+1");
        assert_eq!(Problem::new(2, "x", "bad_code").showing(" 12 ").value, "12");
    }

    #[test]
    fn an_older_problem_reads_without_a_value() {
        let p: Problem =
            serde_json::from_str(r#"{"row":2,"column":"x","reason":"bad_code"}"#).unwrap();
        assert!(p.value.is_empty());
        let json = serde_json::to_string(&p).unwrap();
        assert!(!json.contains("value"));
    }
}

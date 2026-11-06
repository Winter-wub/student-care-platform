//! The pass/fail summary: one row per step with its time.

/// How a step ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    /// It ran and its checks held.
    Pass,
    /// It ran and a check failed.
    Fail,
    /// Not run: an earlier step failed.
    Skip,
}

impl Status {
    fn word(self) -> &'static str {
        match self {
            Status::Pass => "pass",
            Status::Fail => "FAIL",
            Status::Skip => "skip",
        }
    }
}

/// One row of the summary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Step {
    /// What was checked.
    pub name: String,
    /// The outcome.
    pub status: Status,
    /// Wall time in milliseconds (0 when skipped).
    pub ms: u128,
    /// What it saw, or why it failed.
    pub detail: String,
}

/// The table: step, status, ms and detail columns, then a totals line.
pub fn format_table(steps: &[Step]) -> String {
    let width = steps
        .iter()
        .map(|s| s.name.chars().count())
        .max()
        .unwrap_or(0)
        .max("step".len());
    let pad = |s: &str| format!("{s}{}", " ".repeat(width - s.chars().count()));
    let mut out = format!("{}  status  {:>6}  detail\n", pad("step"), "ms");
    out.push_str(&format!(
        "{}  ------  {:>6}  ------\n",
        "-".repeat(width),
        "------"
    ));
    for s in steps {
        let detail = s.detail.lines().next().unwrap_or_default();
        out.push_str(&format!(
            "{}  {:<6}  {:>6}  {detail}\n",
            pad(&s.name),
            s.status.word(),
            s.ms
        ));
    }
    let count = |st| steps.iter().filter(|s| s.status == st).count();
    let total: u128 = steps.iter().map(|s| s.ms).sum();
    out.push_str(&format!(
        "{} passed, {} failed, {} skipped in {:.1} s\n",
        count(Status::Pass),
        count(Status::Fail),
        count(Status::Skip),
        total as f64 / 1000.0
    ));
    out
}

/// Whether every step passed.
pub fn all_passed(steps: &[Step]) -> bool {
    !steps.is_empty() && steps.iter().all(|s| s.status == Status::Pass)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn step(name: &str, status: Status, ms: u128, detail: &str) -> Step {
        Step {
            name: name.into(),
            status,
            ms,
            detail: detail.into(),
        }
    }

    #[test]
    fn the_table_aligns_columns_and_counts_outcomes() {
        let steps = [
            step("boot", Status::Pass, 812, "port 50123"),
            step(
                "roster CSV",
                Status::Fail,
                1500,
                "expected 50 Students\nsecond line",
            ),
            step("ack", Status::Skip, 0, ""),
        ];
        let t = format_table(&steps);
        let lines: Vec<&str> = t.lines().collect();
        assert_eq!(lines[0], "step        status      ms  detail");
        assert_eq!(lines[1], "----------  ------  ------  ------");
        assert_eq!(lines[2], "boot        pass       812  port 50123");
        assert_eq!(lines[3], "roster CSV  FAIL      1500  expected 50 Students");
        assert_eq!(lines[4], "ack         skip         0  ");
        assert_eq!(lines[5], "1 passed, 1 failed, 1 skipped in 2.3 s");
        assert!(!all_passed(&steps));
        assert!(all_passed(&steps[..1]));
        assert!(!all_passed(&[]));
    }

    #[test]
    fn thai_names_pad_by_characters_not_bytes() {
        let t = format_table(&[step("รับทราบ", Status::Pass, 1, "")]);
        assert!(t.lines().nth(2).unwrap().starts_with("รับทราบ  pass"));
    }
}

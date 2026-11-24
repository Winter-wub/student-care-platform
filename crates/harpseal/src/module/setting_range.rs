//! The allowed values of a Number setting (#172): a Module declares them
//! with [`crate::ModuleRegistrar::number_setting`], `/admin/settings`
//! refuses a value outside them and shows them on the input.

/// The whole numbers a [`crate::SettingKind::Number`] setting takes: `min`
/// to `max`, both included, and with [`NumberRange::step`] only `min`,
/// `min + step`, `min + 2 * step`, … (as the HTML `step` attribute).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NumberRange {
    pub(crate) min: i64,
    pub(crate) max: i64,
    pub(crate) step: Option<i64>,
}

/// The catalog key of a value outside the range.
pub(crate) const ERR_RANGE: &str = "kernel.admin.module_settings.err_range";
/// The catalog key of a value in the range but off its step.
pub(crate) const ERR_STEP: &str = "kernel.admin.module_settings.err_step";

impl NumberRange {
    /// `min` to `max`, both included. `build()` refuses `min > max`.
    pub const fn new(min: i64, max: i64) -> Self {
        Self {
            min,
            max,
            step: None,
        }
    }

    /// Only every `step`-th number from `min`. `build()` refuses a `step`
    /// below 1.
    pub const fn step(self, step: i64) -> Self {
        Self {
            step: Some(step),
            ..self
        }
    }

    /// Why the range itself is wrong, if it is.
    pub(crate) fn invalid(&self) -> Option<String> {
        if self.min > self.max {
            return Some(format!(
                "the range is empty: min {} is above max {}",
                self.min, self.max
            ));
        }
        match self.step {
            Some(s) if s < 1 => Some(format!("the step must be at least 1, not {s}")),
            _ => None,
        }
    }

    /// `Ok` for a number the range takes, else the catalog key of why.
    pub(crate) fn check(&self, n: i64) -> core::result::Result<(), &'static str> {
        if n < self.min || n > self.max {
            return Err(ERR_RANGE);
        }
        // `n - min` fits: both are in `min..=max`, whose width an `i128` holds.
        let off = i128::from(n) - i128::from(self.min);
        match self.step {
            Some(s) if off % i128::from(s) != 0 => Err(ERR_STEP),
            _ => Ok(()),
        }
    }

    /// `{min}`, `{max}` and `{step}` for the range's texts.
    pub(crate) fn args(&self) -> [(&'static str, String); 3] {
        [
            ("min", self.min.to_string()),
            ("max", self.max.to_string()),
            ("step", self.step.unwrap_or(1).to_string()),
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_bounds_are_included() {
        let r = NumberRange::new(1, 30);
        for ok in [1, 2, 29, 30] {
            assert_eq!(r.check(ok), Ok(()), "{ok}");
        }
        for bad in [0, 31, -1, 45, i64::MIN, i64::MAX] {
            assert_eq!(r.check(bad), Err(ERR_RANGE), "{bad}");
        }
    }

    #[test]
    fn a_step_counts_from_min() {
        let r = NumberRange::new(5, 60).step(5);
        assert_eq!(r.check(5), Ok(()));
        assert_eq!(r.check(60), Ok(()));
        assert_eq!(r.check(7), Err(ERR_STEP));
        assert_eq!(r.check(0), Err(ERR_RANGE));
        let odd = NumberRange::new(1, 9).step(2);
        assert_eq!(odd.check(3), Ok(()));
        assert_eq!(odd.check(4), Err(ERR_STEP));
        let wide = NumberRange::new(i64::MIN, i64::MAX).step(2);
        assert_eq!(wide.check(i64::MAX), Err(ERR_STEP));
        assert_eq!(wide.check(i64::MAX - 1), Ok(()));
    }

    #[test]
    fn a_wrong_range_is_named() {
        assert!(NumberRange::new(1, 30).invalid().is_none());
        assert!(NumberRange::new(3, 3).invalid().is_none());
        assert!(NumberRange::new(4, 3).invalid().is_some());
        assert!(NumberRange::new(1, 3).step(0).invalid().is_some());
        assert!(NumberRange::new(1, 3).step(-2).invalid().is_some());
    }
}

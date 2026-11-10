//! Shared pieces of a confirmation in proportion to reach (#120, #135):
//! the typed count, which only the number satisfies, and a send that
//! cannot go yet, shown disabled with the reason beside it.

use maud::{Markup, html};

/// Whether `typed` is the number `n`: ASCII or Thai digits, with commas
/// and any whitespace ignored (spaces, tabs, no-break and narrow no-break
/// spaces: "1,234", "1 234", "๑๒๓๔"). A word such as
/// "ยืนยัน" or "confirm" never matches (#135): Staff confirm by reading
/// and typing the reach, not by typing a generic word.
///
/// ```
/// use harpseal::ui::typed_count_ok;
/// assert!(typed_count_ok(Some(" 1,234 "), 1234));
/// assert!(typed_count_ok(Some("๑๒๓๔"), 1234));
/// assert!(!typed_count_ok(Some("ยืนยัน"), 1234));
/// assert!(!typed_count_ok(None, 1234));
/// ```
pub fn typed_count_ok(typed: Option<&str>, n: u64) -> bool {
    let Some(t) = typed.map(str::trim).filter(|t| !t.is_empty()) else {
        return false;
    };
    let digits: Option<String> = t
        .chars()
        .filter(|c| *c != ',' && !c.is_whitespace())
        .map(|c| match c {
            '0'..='9' => Some(c),
            '๐'..='๙' => char::from_digit(c as u32 - '๐' as u32, 10),
            _ => None,
        })
        .collect();
    digits
        .filter(|d| !d.is_empty())
        .and_then(|d| d.parse::<u64>().ok())
        .is_some_and(|got| got == n)
}

/// A submit that cannot go yet (#135): `reason` in a caution note on its
/// own line ("ยังไม่มีนักศึกษาที่ผูกบัญชี"), then the button `label`,
/// `disabled` and tied to the reason by `aria-describedby`, so Staff read
/// why before they reach the button. `id` names the reason (unique on the
/// page). Put it in an `.actions` row; check the same condition on the
/// server, since a disabled button is not a guard.
pub fn blocked_button(label: &str, reason: &str, id: &str) -> Markup {
    html! {
        p class="blocked-reason" id=(id) { (reason) }
        button type="submit" disabled aria-describedby=(id) { (label) }
    }
}

/// A confirm step for an action that cannot be undone (#144, e.g. a
/// delete): `what` happens, then a form posting `confirm=yes` (plus
/// `csrf`, the Staff member's CSRF field) to `action` with the danger
/// button `label`, and the way `back` (`(label, href)`) that does nothing.
/// The handler acts only on `confirm=yes`.
pub fn confirm_step(
    csrf: Markup,
    action: &str,
    what: &str,
    label: &str,
    back: (&str, &str),
) -> Markup {
    html! {
        form method="post" action=(action) class="card confirm-step" {
            (csrf)
            input type="hidden" name="confirm" value="yes";
            p { (what) }
            div class="actions" {
                button type="submit" class="danger" { (label) }
                a class="btn secondary" href=(back.1) { (back.0) }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_number_confirms() {
        for ok in [
            "1234",
            " 1,234 ",
            "๑๒๓๔",
            "1 234",
            "1\u{a0}234",
            "1\u{202f}234",
            "1\t234",
        ] {
            assert!(typed_count_ok(Some(ok), 1234), "{ok}");
        }
        let bad = [
            "",
            "  ",
            ",",
            "1233",
            "12345",
            "-1234",
            "ยืนยัน",
            "Confirm",
            "confirm",
            "yes",
            "1234x",
            "1234.0",
        ];
        for b in bad {
            assert!(!typed_count_ok(Some(b), 1234), "{b}");
        }
        assert!(!typed_count_ok(None, 1234));
        assert!(typed_count_ok(Some("0"), 0));
        assert!(!typed_count_ok(Some(""), 0), "an empty field is never 0");
    }

    #[test]
    fn a_blocked_button_is_disabled_and_names_its_reason() {
        let h = blocked_button("ยืนยันส่ง", "ยังไม่มีนักศึกษาที่ผูกบัญชี", "why").into_string();
        assert!(h.contains("disabled"), "{h}");
        assert!(h.contains(r#"aria-describedby="why""#), "{h}");
        assert!(h.contains(r#"id="why""#), "{h}");
    }
}

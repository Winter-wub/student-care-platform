//! The parse stays linear whatever the input (#108; headings and colours
//! #113): a deterministic step count, not a timer.

use super::*;

/// Steps (characters the main loop reads plus bytes every helper scans,
/// counted by `scan::step` in tests) for `s`.
fn steps(s: &str) -> usize {
    scan::STEPS.with(|c| c.set(0));
    let doc = parse(s);
    assert!(doc.plain().len() <= s.len() + 64);
    scan::STEPS.with(std::cell::Cell::get)
}

/// Every hostile line costs at most 8 steps per byte, at a 2,000-byte size
/// and at 8 times that: deterministic, whatever the machine's load. A
/// quadratic parse (say, a `]` search cache that never hits) blows the
/// bound at once.
#[test]
fn hostile_lines_parse_in_linear_steps() {
    let patterns = [
        "**a ",
        "*a ",
        "[",
        "[x](",
        "[x](a(",
        "*",
        "**",
        "*** ",
        "https://a,",
        "https://seal-bay.invalid/)",
        "]([",
        "![",
        "![x](image:",
        "![x](image:1",
        "![](image:1)",
        "!![x](image:0)",
        // #113: headings and colours.
        "## ",
        "### **a ",
        "## [x](",
        "{red}",
        "{red}x",
        "{/red}",
        "{red}{/red}",
        "{red}**a ",
        "{re",
        "{red}{blue}x{/blue}",
        "**{red}a ",
    ];
    for p in patterns {
        for size in [2_000, 16_000] {
            let s = p.repeat(size / p.len() + 1);
            let n = steps(&s);
            assert!(n <= 8 * s.len(), "{p:?} x{}: {n} steps", s.len());
        }
    }
    // The 2,000-character body itself, mixed.
    let body = "**a [".repeat(400);
    assert!(steps(&body) <= 8 * body.len());
    // Every colour's opener, none closed, then one closer at the end.
    let mut body = String::new();
    for c in Colour::ALL {
        body.push_str(&c.open().repeat(80));
    }
    body.push_str("{/red}");
    assert!(steps(&body) <= 8 * body.len());
    // Many lines, each a heading holding the hostile mix.
    let body = "## **a {red}[x](\n".repeat(200);
    assert!(steps(&body) <= 8 * body.len());
}

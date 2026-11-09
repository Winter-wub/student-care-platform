//! The colour tokens derived from a Deployment's brand colour (#129), so
//! the focus ring, links and text on the brand stay readable whatever the
//! institution picks, including a `[branding] primary_color` in the TOML,
//! which is checked only to be hex.
//!
//! - `--brand`: the colour itself; it fills the bar, primary buttons and
//!   the current page in the navigation.
//! - `--brand-ink`: text on `--brand`: white, else Ledger Ink when white
//!   is under 4.5:1 (only a TOML colour can be: the branding page refuses
//!   one on save, the same [`contrast`]).
//! - `--brand-text`: links and brand-coloured text: the brand, darkened in
//!   steps until it is at least 4.5:1 on every light surface.
//! - `--focus`: the focus ring: the brand, darkened until at least 3:1 on
//!   every light surface (WCAG 1.4.11 and 2.4.13's contrast).

/// Ledger Ink, the body text colour.
pub(crate) const INK: [u8; 3] = [0x17, 0x21, 0x2b];
const WHITE: [u8; 3] = [0xff, 0xff, 0xff];

/// The light surfaces brand-coloured text and the ring may sit on: Paper,
/// Counter Mist, Chip Fog, and the approved, refused, caution and
/// attention washes. The darkest of them decides.
const SURFACES: [[u8; 3]; 7] = [
    [0xff, 0xff, 0xff],
    [0xf4, 0xf6, 0xf8],
    [0xe8, 0xee, 0xf3],
    [0xe5, 0xf5, 0xec],
    [0xfd, 0xec, 0xec],
    [0xff, 0xf3, 0xd6],
    [0xff, 0xf8, 0xe6],
];

/// Least contrast of text (WCAG AA, normal size).
pub(crate) const TEXT_MIN: f64 = 4.5;
/// Least contrast of a focus ring or another non-text mark.
pub(crate) const MARK_MIN: f64 = 3.0;

/// `#rgb` or `#rrggbb` (any case) as red, green, blue.
pub(crate) fn rgb(hex: &str) -> Option<[u8; 3]> {
    let h = hex.trim().strip_prefix('#')?;
    if !h.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let digit = |i: usize| u8::from_str_radix(&h[i..=i], 16).ok();
    match h.len() {
        3 => Some([digit(0)? * 17, digit(1)? * 17, digit(2)? * 17]),
        6 => {
            let pair = |i: usize| u8::from_str_radix(&h[i..i + 2], 16).ok();
            Some([pair(0)?, pair(2)?, pair(4)?])
        }
        _ => None,
    }
}

fn hex(c: [u8; 3]) -> String {
    format!("#{:02x}{:02x}{:02x}", c[0], c[1], c[2])
}

/// WCAG 2 relative luminance.
fn luminance(c: [u8; 3]) -> f64 {
    let lin = |v: u8| {
        let s = f64::from(v) / 255.0;
        if s <= 0.04045 {
            s / 12.92
        } else {
            ((s + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * lin(c[0]) + 0.7152 * lin(c[1]) + 0.0722 * lin(c[2])
}

/// WCAG 2 contrast ratio of `a` and `b` (1 to 21).
pub(crate) fn contrast(a: [u8; 3], b: [u8; 3]) -> f64 {
    let (la, lb) = (luminance(a), luminance(b));
    (la.max(lb) + 0.05) / (la.min(lb) + 0.05)
}

/// The least contrast of `c` on any of [`SURFACES`].
fn on_surfaces(c: [u8; 3]) -> f64 {
    SURFACES
        .iter()
        .map(|s| contrast(c, *s))
        .fold(f64::INFINITY, f64::min)
}

/// `c` mixed toward black in 5 % steps until it reaches `min` on every
/// surface; Ledger Ink if no step does (none fails: black is 21:1).
fn darkened(c: [u8; 3], min: f64) -> [u8; 3] {
    for step in 0..=20u16 {
        let keep = |v: u8| ((u16::from(v) * (20 - step) + 10) / 20) as u8;
        let d = [keep(c[0]), keep(c[1]), keep(c[2])];
        if on_surfaces(d) >= min {
            return d;
        }
    }
    INK
}

/// The derived tokens of one brand colour, as `#rrggbb`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Tokens {
    pub(crate) brand: String,
    pub(crate) brand_ink: String,
    pub(crate) brand_text: String,
    pub(crate) focus: String,
}

impl Tokens {
    /// The tokens of `color` (`#rgb` or `#rrggbb`); the default brand's
    /// for anything else.
    pub(crate) fn of(color: &str) -> Self {
        let c = rgb(color)
            .or_else(|| rgb(crate::config::DEFAULT_PRIMARY))
            .unwrap_or(INK);
        let ink = if contrast(WHITE, c) >= TEXT_MIN || contrast(WHITE, c) >= contrast(INK, c) {
            WHITE
        } else {
            INK
        };
        Tokens {
            brand: hex(c),
            brand_ink: hex(ink),
            brand_text: hex(darkened(c, TEXT_MIN)),
            focus: hex(darkened(c, MARK_MIN)),
        }
    }

    /// The custom properties, for a `style` element or attribute (every
    /// value is a `#rrggbb` written here, never the input).
    pub(crate) fn css(&self) -> String {
        format!(
            "--brand:{};--brand-ink:{};--brand-text:{};--focus:{}",
            self.brand, self.brand_ink, self.brand_text, self.focus
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn worst(c: &str) -> f64 {
        on_surfaces(rgb(c).unwrap())
    }

    #[test]
    fn a_dark_brand_is_used_as_it_is() {
        let t = Tokens::of("#0B5CAD");
        assert_eq!(t.brand, "#0b5cad");
        assert_eq!(
            (t.brand_ink.as_str(), t.focus.as_str()),
            ("#ffffff", "#0b5cad")
        );
        assert_eq!(t.brand_text, "#0b5cad");
        assert!(worst(&t.focus) >= MARK_MIN && worst(&t.brand_text) >= TEXT_MIN);
    }

    #[test]
    fn a_light_brand_gets_dark_ink_and_a_darkened_ring_and_text() {
        // A TOML colour the branding page would refuse: white is 1.4:1.
        let t = Tokens::of("#ffd400");
        assert_eq!(t.brand, "#ffd400");
        assert_eq!(t.brand_ink, "#17212b");
        assert!(contrast(rgb(&t.brand_ink).unwrap(), rgb(&t.brand).unwrap()) >= TEXT_MIN);
        assert_ne!(t.focus, t.brand);
        assert!(worst(&t.focus) >= MARK_MIN, "{}", worst(&t.focus));
        assert!(worst(&t.brand_text) >= TEXT_MIN, "{}", worst(&t.brand_text));
        // Still the institution's hue, not grey.
        let f = rgb(&t.focus).unwrap();
        assert!(f[0] > f[2] && f[1] > f[2], "{f:?}");
    }

    #[test]
    fn every_colour_the_branding_page_accepts_keeps_white_ink_and_a_ring() {
        // The save check (#114) refuses white under 4.5:1; every colour it
        // accepts keeps white ink, and its ring and text stay readable.
        for c in [
            "#767676", "#0f5c7a", "#8a1c1c", "#2e7d32", "#6a1b9a", "#000000",
        ] {
            assert!(crate::kernel::branding::check::color(c).is_ok(), "{c}");
            let t = Tokens::of(c);
            assert_eq!(t.brand_ink, "#ffffff", "{c}");
            assert!(worst(&t.focus) >= MARK_MIN, "{c}");
            assert!(worst(&t.brand_text) >= TEXT_MIN, "{c}");
        }
        // The AA edge is 4.2:1 on Counter Mist, so its text is darkened.
        assert_ne!(Tokens::of("#767676").brand_text, "#767676");
        assert_eq!(Tokens::of("#767676").focus, "#767676");
    }

    #[test]
    fn short_hex_and_junk() {
        assert_eq!(rgb("#fa0"), Some([0xff, 0xaa, 0x00]));
        assert_eq!(rgb("#0b5cag"), None);
        assert_eq!(Tokens::of("red").brand, crate::config::DEFAULT_PRIMARY);
        assert!(
            Tokens::of("#fff")
                .css()
                .starts_with("--brand:#ffffff;--brand-ink:#17212b")
        );
    }
}

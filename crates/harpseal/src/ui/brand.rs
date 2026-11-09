//! The colour tokens derived from a Deployment's brand colour (#129), so
//! the focus ring, links and text on the brand stay readable whatever the
//! institution picks, including a `[branding] primary_color` in the TOML,
//! which is checked only to be hex.
//!
//! - `--brand`: the colour itself; it fills the bar, primary buttons and
//!   the current page in the navigation.
//! - `--brand-ink`: text on `--brand`: white, else Ledger Ink, else black,
//!   the first at least 4.5:1 (only a TOML colour can be: the branding page refuses
//!   one on save, the same [`contrast`]).
//! - `--brand-text`: links and brand-coloured text: the brand, darkened in
//!   steps until it is at least 4.5:1 on every light surface.
//! - `--focus`: the focus ring: the brand, darkened until at least 3:1 on
//!   every light surface (WCAG 1.4.11 and 2.4.13's contrast).
//! - `--brand-press` (#131): a primary button's hover and active fill: the
//!   brand moved 15 % away from its ink (darker under white ink, lighter
//!   under dark ink), so the ink only gains contrast.

/// Ledger Ink, the body text colour.
pub(crate) const INK: [u8; 3] = [0x17, 0x21, 0x2b];
const WHITE: [u8; 3] = [0xff, 0xff, 0xff];
const BLACK: [u8; 3] = [0, 0, 0];

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

/// `c` mixed 15 % toward `to`.
fn mixed(c: [u8; 3], to: [u8; 3]) -> [u8; 3] {
    let m = |a: u8, b: u8| ((u16::from(a) * 85 + u16::from(b) * 15 + 50) / 100) as u8;
    [m(c[0], to[0]), m(c[1], to[1]), m(c[2], to[2])]
}

/// The press fill: `c` moved away from `ink`; at black or white, where
/// that leaves it unchanged, toward the ink instead (still far above
/// 4.5:1 there).
fn pressed(c: [u8; 3], ink: [u8; 3]) -> [u8; 3] {
    let away = mixed(c, if ink == WHITE { BLACK } else { WHITE });
    if away != c {
        away
    } else {
        mixed(c, if ink == WHITE { WHITE } else { BLACK })
    }
}

/// The derived tokens of one brand colour, as `#rrggbb`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Tokens {
    pub(crate) brand: String,
    pub(crate) brand_ink: String,
    pub(crate) brand_text: String,
    pub(crate) focus: String,
    pub(crate) brand_press: String,
}

impl Tokens {
    /// The tokens of `color` (`#rgb` or `#rrggbb`); the default brand's
    /// for anything else.
    pub(crate) fn of(color: &str) -> Self {
        let c = rgb(color)
            .or_else(|| rgb(crate::config::DEFAULT_PRIMARY))
            .unwrap_or(INK);
        // White, else Ledger Ink, else black: when white is under 4.5:1
        // the brand's luminance is above 0.183, so black is at least 4.66:1.
        let ink = [WHITE, INK]
            .into_iter()
            .find(|t| contrast(*t, c) >= TEXT_MIN)
            .unwrap_or(BLACK);
        Tokens {
            brand: hex(c),
            brand_ink: hex(ink),
            brand_text: hex(darkened(c, TEXT_MIN)),
            focus: hex(darkened(c, MARK_MIN)),
            // Away from the ink: white ink on a darker fill, dark ink on a
            // lighter one, so the label's contrast never drops.
            brand_press: hex(pressed(c, ink)),
        }
    }

    /// The custom properties, for a `style` element or attribute (every
    /// value is a `#rrggbb` written here, never the input).
    pub(crate) fn css(&self) -> String {
        format!(
            "--brand:{};--brand-ink:{};--brand-text:{};--focus:{};--brand-press:{}",
            self.brand, self.brand_ink, self.brand_text, self.focus, self.brand_press
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
    fn text_on_any_brand_reaches_4_5() {
        // Mid tones where white fails and Ledger Ink is close: #1e88e5 is
        // white 3.7, ink 4.4, so it takes black.
        for (c, ink) in [
            ("#1e88e5", "#000000"),
            ("#f44336", "#000000"),
            ("#e65100", "#000000"),
            ("#808080", "#000000"),
            ("#ffd400", "#17212b"),
            ("#0b5cad", "#ffffff"),
        ] {
            let t = Tokens::of(c);
            assert_eq!(t.brand_ink, ink, "{c}");
            let ratio = contrast(rgb(&t.brand_ink).unwrap(), rgb(c).unwrap());
            assert!(ratio >= TEXT_MIN, "{c}: {ratio}");
        }
        // Every grey: one of the three always reaches 4.5:1.
        for v in 0..=255u8 {
            let t = Tokens::of(&hex([v, v, v]));
            assert!(
                contrast(rgb(&t.brand_ink).unwrap(), [v, v, v]) >= TEXT_MIN,
                "{v}"
            );
        }
    }

    #[test]
    fn the_pressed_fill_keeps_the_ink_readable() {
        for c in ["#0f5c7a", "#767676", "#ffd400", "#1e88e5", "#000000"] {
            let t = Tokens::of(c);
            let (ink, fill, press) = (
                rgb(&t.brand_ink).unwrap(),
                rgb(c).unwrap(),
                rgb(&t.brand_press).unwrap(),
            );
            assert_ne!(t.brand_press, t.brand, "{c}");
            let keeps = contrast(ink, press) >= contrast(ink, fill);
            assert!(keeps || contrast(ink, press) >= 15.0, "{c}");
        }
        assert_eq!(Tokens::of("#0f5c7a").brand_press, "#0d4e68");
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

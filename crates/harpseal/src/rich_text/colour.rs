//! The text colours of a body (#113): a fixed palette, never a free hex.
//! Each colour passes 4.5:1 (WCAG AA, normal text) on Paper `#ffffff` and
//! on Counter Mist `#f4f6f8`, the only surfaces a body sits on (there is
//! no dark theme yet). Chat strips them ([`super::Doc::plain`]).
//!
//! Syntax: `{red}text{/red}`; the closer names its colour. A colour never
//! holds another one; it may hold bold, italic and links, and bold or
//! italic may hold it.

/// A colour of the palette.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Colour {
    /// `#b42318`: 6.57:1 on white, 6.07:1 on Counter Mist.
    Red,
    /// `#b54708`: 5.43:1 and 5.01:1.
    Orange,
    /// `#067647`: 5.69:1 and 5.25:1.
    Green,
    /// `#175cd3`: 5.99:1 and 5.53:1.
    Blue,
    /// `#475467`: 7.69:1 and 7.10:1.
    Grey,
}

impl Colour {
    /// Every colour, in toolbar order.
    pub const ALL: [Colour; 5] = [
        Colour::Red,
        Colour::Orange,
        Colour::Green,
        Colour::Blue,
        Colour::Grey,
    ];

    /// The name typed in a body: `red` in `{red}…{/red}`.
    pub fn name(self) -> &'static str {
        match self {
            Self::Red => "red",
            Self::Orange => "orange",
            Self::Green => "green",
            Self::Blue => "blue",
            Self::Grey => "grey",
        }
    }

    /// The colour itself (`#rrggbb`), as in `ui.css` and DESIGN.md.
    pub fn hex(self) -> &'static str {
        match self {
            Self::Red => "#b42318",
            Self::Orange => "#b54708",
            Self::Green => "#067647",
            Self::Blue => "#175cd3",
            Self::Grey => "#475467",
        }
    }

    /// The CSS class of the `<span>` ([`super::Doc::html`]): a class, not
    /// a `style`, so a body never writes CSS.
    pub fn class(self) -> &'static str {
        match self {
            Self::Red => "tc-red",
            Self::Orange => "tc-orange",
            Self::Green => "tc-green",
            Self::Blue => "tc-blue",
            Self::Grey => "tc-grey",
        }
    }

    /// `{red}`.
    pub(super) fn open(self) -> &'static str {
        match self {
            Self::Red => "{red}",
            Self::Orange => "{orange}",
            Self::Green => "{green}",
            Self::Blue => "{blue}",
            Self::Grey => "{grey}",
        }
    }

    /// `{/red}`.
    pub(super) fn close(self) -> &'static str {
        match self {
            Self::Red => "{/red}",
            Self::Orange => "{/orange}",
            Self::Green => "{/green}",
            Self::Blue => "{/blue}",
            Self::Grey => "{/grey}",
        }
    }

    /// Its index in [`Colour::ALL`].
    pub(super) fn index(self) -> usize {
        match self {
            Self::Red => 0,
            Self::Orange => 1,
            Self::Green => 2,
            Self::Blue => 3,
            Self::Grey => 4,
        }
    }
}

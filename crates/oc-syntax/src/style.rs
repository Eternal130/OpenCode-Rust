//! The oc-syntax style vocabulary: [`Color`], [`Modifier`] and
//! [`Style`] — self-contained (no oc-tui, no ratatui), the token style
//! shape the highlight engine materializes spans in (lane B2: the
//! vocabulary leaves ratatui; consumers convert at their boundary).
//!
//! The shape and the modifier bit layout match ratatui 0.29's `style`
//! surface byte-for-byte, so boundary conversions are field-for-field
//! rebuilds with zero semantic drift.

/// A token color: theme rules resolve to `Rgb`; `Reset` and `Indexed`
/// cover the terminal-default / palette forms.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Color {
    Reset,
    Indexed(u8),
    Rgb(u8, u8, u8),
}

/// Text attribute bits — ratatui 0.29's exact layout (`BOLD = 1<<0` …
/// `CROSSED_OUT = 1<<8`), so consumers rebuild their own words 1:1.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Modifier(u16);

impl Modifier {
    pub const EMPTY: Self = Self(0);
    pub const BOLD: Self = Self(1 << 0);
    pub const DIM: Self = Self(1 << 1);
    pub const ITALIC: Self = Self(1 << 2);
    pub const UNDERLINED: Self = Self(1 << 3);
    pub const SLOW_BLINK: Self = Self(1 << 4);
    pub const RAPID_BLINK: Self = Self(1 << 5);
    pub const REVERSED: Self = Self(1 << 6);
    pub const HIDDEN: Self = Self(1 << 7);
    pub const CROSSED_OUT: Self = Self(1 << 8);

    /// The empty word.
    #[must_use]
    pub const fn empty() -> Self {
        Self(0)
    }

    /// Whether every bit of `other` is set.
    #[must_use]
    pub const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }

    /// Whether any bit of `other` is set.
    #[must_use]
    pub const fn intersects(self, other: Self) -> bool {
        self.0 & other.0 != 0
    }

    /// Whether no bit is set.
    #[must_use]
    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }

    /// Set every bit of `other`.
    pub const fn insert(&mut self, other: Self) {
        self.0 |= other.0;
    }

    /// Clear every bit of `other`.
    pub const fn remove(&mut self, other: Self) {
        self.0 &= !other.0;
    }

    /// The raw bit word.
    #[must_use]
    pub const fn bits(self) -> u16 {
        self.0
    }

    /// Rebuild from a raw bit word produced by [`Modifier::bits`].
    #[must_use]
    pub const fn from_bits(bits: u16) -> Self {
        Self(bits)
    }
}

impl std::ops::BitOr for Modifier {
    type Output = Self;
    fn bitor(self, rhs: Self) -> Self {
        Self(self.0 | rhs.0)
    }
}

impl std::ops::BitOrAssign for Modifier {
    fn bitor_assign(&mut self, rhs: Self) {
        self.0 |= rhs.0;
    }
}

/// A token style: optional fg/bg overlaid on the base, plus an
/// attribute word to add — ratatui 0.29's `Style` shape (minus the
/// fields nothing here exercises).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Style {
    pub fg: Option<Color>,
    pub bg: Option<Color>,
    pub add_modifier: Modifier,
    pub sub_modifier: Modifier,
}

impl Style {
    /// The empty style.
    #[must_use]
    pub const fn new() -> Self {
        Self { fg: None, bg: None, add_modifier: Modifier::EMPTY, sub_modifier: Modifier::EMPTY }
    }

    /// Set the foreground (overlay semantics).
    #[must_use]
    pub const fn fg(mut self, color: Color) -> Self {
        self.fg = Some(color);
        self
    }

    /// Set the background (overlay semantics).
    #[must_use]
    pub const fn bg(mut self, color: Color) -> Self {
        self.bg = Some(color);
        self
    }

    /// Add attribute bits.
    #[must_use]
    pub const fn add_modifier(mut self, modifier: Modifier) -> Self {
        self.add_modifier.0 |= modifier.0;
        self
    }

    /// Remove attribute bits.
    #[must_use]
    pub const fn remove_modifier(mut self, modifier: Modifier) -> Self {
        self.sub_modifier.0 |= modifier.0;
        self
    }
}

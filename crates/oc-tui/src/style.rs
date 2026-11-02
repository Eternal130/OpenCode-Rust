//! The oc-tui style vocabulary: [`Color`], [`Modifier`] and [`Style`]
//! — a self-owned copy of the ratatui 0.29 `style` surface this crate
//! actually consumes (lane B2 of the opentui render migration: the
//! vocabulary leaves ratatui; only the terminal backends stay).
//!
//! Semantics are byte-for-byte the ratatui ones:
//!
//! * [`Modifier`] keeps ratatui's exact bit layout (`BOLD = 1<<0` …
//!   `CROSSED_OUT = 1<<8`) — the canvas merges attribute words with
//!   the same insert/remove arithmetic, and the canonical serializer's
//!   ECMA-ordered bits are derived from these.
//! * [`Style`] is the ratatui shape: `Option`al fg/bg overlaid on top
//!   of the cell's current colors, plus add/sub modifier words. The
//!   builder methods are `const fn` (the layout tests build `const`
//!   styles).
//!
//! No widgets, no backend — just the paint vocabulary.

/// A terminal color, in the ratatui 0.29 vocabulary this crate uses
/// (theme tokens resolve to `Rgb`, the terminal-palette paths carry
/// `Indexed`, and `Reset` is the terminal default).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Color {
    #[default]
    Reset,
    Indexed(u8),
    Rgb(u8, u8, u8),
}

/// Text attribute bits — ratatui 0.29's exact layout (`Modifier::BOLD`
/// is bit 0, … `Modifier::CROSSED_OUT` is bit 8). The bit values are
/// load-bearing: the paint canvas merges words bit-wise and the
/// canonical serializer's ECMA ordering is derived from them.
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

    /// The empty word (ratatui `Modifier::empty()`).
    #[must_use]
    pub const fn empty() -> Self {
        Self(0)
    }

    /// Whether every bit of `other` is set (bitflags `contains`).
    #[must_use]
    pub const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }

    /// Whether any bit of `other` is set (bitflags `intersects`).
    #[must_use]
    pub const fn intersects(self, other: Self) -> bool {
        self.0 & other.0 != 0
    }

    /// Whether no bit is set (bitflags `is_empty`).
    #[must_use]
    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }

    /// Set every bit of `other` (bitflags `insert`).
    pub const fn insert(&mut self, other: Self) {
        self.0 |= other.0;
    }

    /// Clear every bit of `other` (bitflags `remove`).
    pub const fn remove(&mut self, other: Self) {
        self.0 &= !other.0;
    }

    /// The raw bit word (same values ratatui's bitflags carry).
    #[must_use]
    pub const fn bits(self) -> u16 {
        self.0
    }

    /// Rebuild from a raw bit word produced by [`Modifier::bits`].
    #[must_use]
    pub const fn from_bits(bits: u16) -> Self {
        Self(bits)
    }

    /// Iterate the set known flags, lowest bit first (bitflags `iter`
    /// order — the canonical `Debug` chain lists modifiers in this
    /// order).
    pub fn iter(self) -> impl Iterator<Item = Modifier> {
        [
            Modifier::BOLD,
            Modifier::DIM,
            Modifier::ITALIC,
            Modifier::UNDERLINED,
            Modifier::SLOW_BLINK,
            Modifier::RAPID_BLINK,
            Modifier::REVERSED,
            Modifier::HIDDEN,
            Modifier::CROSSED_OUT,
        ]
        .into_iter()
        .filter(move |flag| self.contains(*flag))
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

/// A paint style: optional fg/bg overlaid on the target cell, plus
/// attribute words to add and remove — ratatui 0.29's `Style` shape
/// (minus `underline_color`, which nothing in this crate sets).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Style {
    pub fg: Option<Color>,
    pub bg: Option<Color>,
    pub add_modifier: Modifier,
    pub sub_modifier: Modifier,
}

impl Style {
    /// The empty style (ratatui `Style::new()` — a `const fn` there,
    /// and here: the layout tests build `const` styles).
    #[must_use]
    pub const fn new() -> Self {
        Self { fg: None, bg: None, add_modifier: Modifier::EMPTY, sub_modifier: Modifier::EMPTY }
    }

    /// The canonical builder-style `Debug` chain (ratatui 0.29's
    /// `Style: Debug` prints `Style::new().fg(Color::Rgb(..)).bold()`
    /// — the syntax goldens pin these bytes, so the vocabulary keeps
    /// the exact format).
    fn fmt_stylize(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if let Some(color) = self.fg {
            write!(f, ".fg(Color::{color:?})")?;
        }
        if let Some(color) = self.bg {
            write!(f, ".bg(Color::{color:?})")?;
        }
        for modifier in self.add_modifier.iter() {
            match modifier {
                Modifier::BOLD => f.write_str(".bold()")?,
                Modifier::DIM => f.write_str(".dim()")?,
                Modifier::ITALIC => f.write_str(".italic()")?,
                Modifier::UNDERLINED => f.write_str(".underlined()")?,
                Modifier::SLOW_BLINK => f.write_str(".slow_blink()")?,
                Modifier::RAPID_BLINK => f.write_str(".rapid_blink()")?,
                Modifier::REVERSED => f.write_str(".reversed()")?,
                Modifier::HIDDEN => f.write_str(".hidden()")?,
                Modifier::CROSSED_OUT => f.write_str(".crossed_out()")?,
                _ => write!(f, ".add_modifier(Modifier::{modifier:?})")?,
            }
        }
        for modifier in self.sub_modifier.iter() {
            match modifier {
                Modifier::BOLD => f.write_str(".not_bold()")?,
                Modifier::DIM => f.write_str(".not_dim()")?,
                Modifier::ITALIC => f.write_str(".not_italic()")?,
                Modifier::UNDERLINED => f.write_str(".not_underlined()")?,
                Modifier::SLOW_BLINK => f.write_str(".not_slow_blink()")?,
                Modifier::RAPID_BLINK => f.write_str(".not_rapid_blink()")?,
                Modifier::REVERSED => f.write_str(".not_reversed()")?,
                Modifier::HIDDEN => f.write_str(".not_hidden()")?,
                Modifier::CROSSED_OUT => f.write_str(".not_crossed_out()")?,
                _ => write!(f, ".remove_modifier(Modifier::{modifier:?})")?,
            }
        }
        Ok(())
    }

    /// Set the foreground (overlay semantics: `None` leaves the
    /// cell's current fg untouched).
    #[must_use]
    pub const fn fg(mut self, color: Color) -> Self {
        self.fg = Some(color);
        self
    }

    /// Set the background (same overlay semantics as [`Style::fg`]).
    #[must_use]
    pub const fn bg(mut self, color: Color) -> Self {
        self.bg = Some(color);
        self
    }

    /// Add attribute bits (ratatui: `add_modifier.insert(m)`).
    #[must_use]
    pub const fn add_modifier(mut self, modifier: Modifier) -> Self {
        self.add_modifier.0 |= modifier.0;
        self
    }

    /// Remove attribute bits (ratatui: `sub_modifier.insert(m)`).
    #[must_use]
    pub const fn remove_modifier(mut self, modifier: Modifier) -> Self {
        self.sub_modifier.0 |= modifier.0;
        self
    }
}

impl std::fmt::Debug for Style {
    /// ratatui 0.29's builder-chain format (`Style::new().fg(..)…`),
    /// byte-identical — the syntax goldens pin these bytes.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Style::new()")?;
        self.fmt_stylize(f)
    }
}

impl From<oc_syntax::style::Style> for Style {
    /// The oc-syntax boundary conversion (same shape, same bit
    /// layout — a field-for-field rebuild).
    fn from(style: oc_syntax::style::Style) -> Self {
        Self {
            fg: style.fg.map(Color::from),
            bg: style.bg.map(Color::from),
            add_modifier: Modifier::from_bits(style.add_modifier.bits()),
            sub_modifier: Modifier::from_bits(style.sub_modifier.bits()),
        }
    }
}

impl From<Style> for oc_syntax::style::Style {
    /// The oc-tui → oc-syntax direction (the highlight boundary's
    /// base style).
    fn from(style: Style) -> Self {
        Self {
            fg: style.fg.map(oc_syntax::style::Color::from),
            bg: style.bg.map(oc_syntax::style::Color::from),
            add_modifier: oc_syntax::style::Modifier::from_bits(style.add_modifier.bits()),
            sub_modifier: oc_syntax::style::Modifier::from_bits(style.sub_modifier.bits()),
        }
    }
}

impl From<oc_syntax::style::Color> for Color {
    fn from(color: oc_syntax::style::Color) -> Self {
        match color {
            oc_syntax::style::Color::Reset => Self::Reset,
            oc_syntax::style::Color::Indexed(index) => Self::Indexed(index),
            oc_syntax::style::Color::Rgb(r, g, b) => Self::Rgb(r, g, b),
        }
    }
}

impl From<Color> for oc_syntax::style::Color {
    fn from(color: Color) -> Self {
        match color {
            Color::Reset => Self::Reset,
            Color::Indexed(index) => Self::Indexed(index),
            Color::Rgb(r, g, b) => Self::Rgb(r, g, b),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The bit layout is ratatui 0.29's, exactly (the load-bearing
    /// contract: canvas merges and the canonical serializer's ECMA
    /// ordering derive from these values).
    #[test]
    fn modifier_bits_match_ratatui_layout() {
        assert_eq!(Modifier::BOLD.bits(), 1 << 0);
        assert_eq!(Modifier::DIM.bits(), 1 << 1);
        assert_eq!(Modifier::ITALIC.bits(), 1 << 2);
        assert_eq!(Modifier::UNDERLINED.bits(), 1 << 3);
        assert_eq!(Modifier::SLOW_BLINK.bits(), 1 << 4);
        assert_eq!(Modifier::RAPID_BLINK.bits(), 1 << 5);
        assert_eq!(Modifier::REVERSED.bits(), 1 << 6);
        assert_eq!(Modifier::HIDDEN.bits(), 1 << 7);
        assert_eq!(Modifier::CROSSED_OUT.bits(), 1 << 8);
    }

    /// The builder surface components use, const-context included
    /// (the layout tests build `const` styles).
    #[test]
    fn builder_is_const_and_chains() {
        const BOLD_BASE: Style = Style::new().fg(Color::Indexed(252)).add_modifier(Modifier::BOLD);
        assert_eq!(BOLD_BASE.fg, Some(Color::Indexed(252)));
        assert!(BOLD_BASE.add_modifier.contains(Modifier::BOLD));
        assert!(Modifier::SLOW_BLINK.intersects(Modifier::SLOW_BLINK | Modifier::RAPID_BLINK));
        let mut word = Modifier::empty();
        word |= Modifier::BOLD;
        word.insert(Modifier::ITALIC);
        assert!(word.contains(Modifier::BOLD));
        assert!(word.contains(Modifier::ITALIC));
        word.remove(Modifier::BOLD);
        assert!(!word.contains(Modifier::BOLD));
    }
}

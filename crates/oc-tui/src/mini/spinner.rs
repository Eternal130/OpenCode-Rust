//! The one-cell work-spinner tables (`ui/one-cell-motion.ts` +
//! `ui/subcell.ts`) — CONFIG-T4's `mini.work_spinner` consumer.
//!
//! v2 renders the busy indicator as ONE terminal cell whose glyph
//! advances through a frame table (`<Show when={busy()}>` →
//! `OneCellSpinner animation={WORK_SPINNERS[miniSettings().work_spinner]}`,
//! `footer.view.tsx:978-988`); `mini.mono` swaps in the ASCII
//! `SEED_MONO` animation. The frame index is `elapsed / interval`
//! (modulo the table); the >30 s `WORK_PACE` easing and the glow
//! `levels` only change animation speed/brightness, not the frame
//! set, so the mono port keeps the plain interval stepping.

use std::time::Duration;

/// One animation: the glyph table + the per-frame interval
/// (`OneCellMotion`'s `frames`/`interval` subset the mono writer
/// needs).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Motion {
    pub frames: Vec<char>,
    pub interval_ms: u64,
}

impl Motion {
    /// The glyph for an elapsed age (`oneCellFrame`'s modulo walk —
    /// intro/pace/once are splash-only paths the busy spinner never
    /// uses).
    #[must_use]
    pub fn frame(&self, elapsed: Duration) -> char {
        if self.frames.is_empty() || self.interval_ms == 0 {
            return ' ';
        }
        let ticks = elapsed.as_millis() / u128::from(self.interval_ms);
        self.frames[(ticks % self.frames.len() as u128) as usize]
    }
}

/// `octantGlyph` (`subcell.ts:32-36`): the row-major 2x4 octant masks
/// Unicode reuses older glyphs for, then the sequential octant block
/// (`0x1cd00 + mask − mapped-below`) for the rest.
#[must_use]
#[allow(clippy::cast_possible_truncation)] // the count is bounded by the 26-entry table
pub fn octant_glyph(mask: u8) -> char {
    match mask {
        0x00 => ' ',
        0x01 => '\u{1cea8}',
        0x02 => '\u{1ceab}',
        0x03 => '\u{1fb82}',
        0x05 => '\u{2598}',
        0x0a => '\u{259d}',
        0x0f => '\u{2580}',
        0x14 => '\u{1fbe6}',
        0x28 => '\u{1fbe7}',
        0x3f => '\u{1fb85}',
        0x40 => '\u{1cea3}',
        0x50 => '\u{2596}',
        0x55 => '\u{258c}',
        0x5a => '\u{259e}',
        0x5f => '\u{259b}',
        0x80 => '\u{1cea0}',
        0xa0 => '\u{2597}',
        0xa5 => '\u{259a}',
        0xaa => '\u{2590}',
        0xaf => '\u{259c}',
        0xc0 => '\u{2582}',
        0xf0 => '\u{2584}',
        0xf5 => '\u{2599}',
        0xfa => '\u{259f}',
        0xfc => '\u{2586}',
        0xff => '\u{2588}',
        other => {
            let below = OCTANT_MASKS.iter().filter(|&&key| key < other).count() as u32;
            char::from_u32(0x1cd00 + u32::from(other) - below).unwrap_or(' ')
        }
    }
}

/// The mapped octant masks, ascending (`OCTANTS`, `subcell.ts:3-31`).
const OCTANT_MASKS: [u8; 26] = [
    0x00, 0x01, 0x02, 0x03, 0x05, 0x0a, 0x0f, 0x14, 0x28, 0x3f, 0x40, 0x50, 0x55, 0x5a, 0x5f, 0x80,
    0xa0, 0xa5, 0xaa, 0xaf, 0xc0, 0xf0, 0xf5, 0xfa, 0xfc, 0xff,
];

/// `BLOCK_SOFT_SLIDE` (the default spinner): five soft-left blocks
/// then five soft-right blocks.
fn block_soft_slide() -> Motion {
    let mut frames = vec!['\u{1fbe6}'; 5];
    frames.extend(vec!['\u{1fbe7}'; 5]);
    Motion { frames, interval_ms: 100 }
}

/// `WORK_SPINNERS[id]` (`one-cell-motion.ts:77-106`): the twelve
/// `MiniWorkSpinner` literals. Unknown ids fall back to the default
/// (`block-soft-slide`) — the schema normally rejects them upstream.
#[must_use]
#[allow(clippy::match_same_arms)] // the default arm mirrors the reference fallback
pub fn work_spinner(id: &str) -> Motion {
    let octets = |frames: &[u8], interval_ms: u64| Motion {
        frames: frames.iter().map(|&mask| octant_glyph(mask)).collect(),
        interval_ms,
    };
    match id {
        "block-soft-slide" => block_soft_slide(),
        "block-soft-sweep" => {
            octets(&[0x14, 0x14, 0x14, 0x1c, 0x3c, 0x38, 0x28, 0x28, 0x28, 0x38, 0x3c, 0x1c], 100)
        }
        "block-low-comet" => Motion { frames: block_low_comet(), interval_ms: 100 },
        "block-low-duet" => Motion { frames: block_low_duet(), interval_ms: 60 },
        "block-shuttle" => octets(&[0x14, 0x28], 500),
        "block-bridge" => octets(&[0x14, 0x14, 0x14, 0x3c, 0x28, 0x28, 0x28, 0x3c], 120),
        "block-squeeze" => {
            octets(&[0x14, 0x14, 0x14, 0x10, 0x20, 0x28, 0x28, 0x28, 0x20, 0x10], 100)
        }
        "small-toggle" => Motion { frames: vec!['\u{25ab}', '\u{25aa}'], interval_ms: 320 },
        "square-toggle" => Motion { frames: vec!['\u{25a1}', '\u{25a0}'], interval_ms: 500 },
        "grow-shrink" => Motion {
            frames: vec!['\u{25ab}', '\u{25ab}', '\u{25aa}', '\u{25a0}', '\u{25aa}'],
            interval_ms: 180,
        },
        "quadrant-orbit" => Motion {
            frames: vec!['\u{2598}', '\u{259d}', '\u{2597}', '\u{2596}'],
            interval_ms: 160,
        },
        "crosshatch" => Motion {
            frames: vec!['\u{25a7}', '\u{25a9}', '\u{25a8}', '\u{25a9}'],
            interval_ms: 240,
        },
        "density-wave" => Motion {
            frames: vec![
                '\u{2591}', '\u{2591}', '\u{2591}', '\u{2592}', '\u{2593}', '\u{2588}', '\u{2593}',
                '\u{2592}',
            ],
            interval_ms: 160,
        },
        // `SEED_WORK`: forty identical ▫ frames (the glow levels are
        // the only variation — one glyph for the mono writer).
        "seed" => Motion { frames: vec!['\u{25aa}'], interval_ms: 40 },
        _ => block_soft_slide(),
    }
}

/// `SEED_MONO` (`one-cell-motion.ts:107`): the ASCII spinner `mono`
/// selects (`animation={props.mono ? SEED_MONO : …}`).
#[must_use]
pub fn seed_mono() -> Motion {
    Motion { frames: vec!['-', '\\', '|', '/'], interval_ms: 120 }
}

/// `BLOCK_LOW_COMET` (`one-cell-motion.ts:48-55`): a comet walking
/// the lower octants — per point, three `full` frames then two
/// `leading` frames. (The octant masks fit `u8`: the walk's highest
/// bit is point 7.)
fn block_low_comet() -> Vec<char> {
    let path = [2u8, 3, 5, 7, 6, 4];
    let mut frames = Vec::new();
    for (index, &point) in path.iter().enumerate() {
        let leading = (1u8 << point) | (1u8 << path[(index + 5) % 6]);
        let full = leading | (1u8 << path[(index + 4) % 6]);
        for mask in [full, full, full, leading, leading] {
            frames.push(octant_glyph(mask));
        }
    }
    frames
}

/// `block-low-duet` (`one-cell-motion.ts:84-92`): two lower cells
/// trading places — per point, three `full` frames then one `heads`.
fn block_low_duet() -> Vec<char> {
    let path = [2u8, 3, 5, 7, 6, 4];
    let mut frames = Vec::new();
    for (index, &point) in path.iter().enumerate() {
        let heads = (1u8 << point) | (1u8 << path[(index + 3) % 6]);
        let full = heads | (1u8 << path[(index + 2) % 6]) | (1u8 << path[(index + 5) % 6]);
        for mask in [full, full, full, heads] {
            frames.push(octant_glyph(mask));
        }
    }
    frames
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_slide_frames_match_the_reference() {
        let slide = work_spinner("block-soft-slide");
        assert_eq!(slide.frames.len(), 10);
        assert_eq!(slide.interval_ms, 100);
        assert!(slide.frames.iter().take(5).all(|&glyph| glyph == '\u{1fbe6}'));
        assert!(slide.frames.iter().skip(5).all(|&glyph| glyph == '\u{1fbe7}'));
    }

    /// Every `MiniWorkSpinner` literal resolves to a non-empty table
    /// with a positive interval (the schema's literal set,
    /// `config/index.ts` `MiniWorkSpinner`).
    #[test]
    fn every_literal_resolves() {
        for id in [
            "block-soft-slide",
            "block-soft-sweep",
            "block-low-comet",
            "block-low-duet",
            "block-shuttle",
            "block-bridge",
            "block-squeeze",
            "small-toggle",
            "square-toggle",
            "grow-shrink",
            "quadrant-orbit",
            "crosshatch",
            "density-wave",
            "seed",
        ] {
            let motion = work_spinner(id);
            assert!(!motion.frames.is_empty(), "{id} needs frames");
            assert!(motion.interval_ms > 0, "{id} needs an interval");
            assert!(motion
                .frames
                .iter()
                .all(|glyph| !glyph.is_whitespace() || *glyph == ' ' || id == "seed"));
        }
        // Unknown ids degrade to the default, never panic.
        assert_eq!(work_spinner("nonsense"), work_spinner("block-soft-slide"));
    }

    #[test]
    fn frame_advances_with_the_interval() {
        let motion = Motion { frames: vec!['a', 'b', 'c'], interval_ms: 100 };
        assert_eq!(motion.frame(Duration::from_millis(0)), 'a');
        assert_eq!(motion.frame(Duration::from_millis(99)), 'a');
        assert_eq!(motion.frame(Duration::from_millis(100)), 'b');
        assert_eq!(motion.frame(Duration::from_millis(250)), 'c');
        assert_eq!(motion.frame(Duration::from_millis(300)), 'a', "wraps");
    }

    /// `octantGlyph`'s sequential fallback (`subcell.ts:35`): masks
    /// outside the reused-glyph table land in the octant block.
    #[test]
    fn octant_fallback_is_sequential() {
        assert_eq!(octant_glyph(0x1c), char::from_u32(0x1cd00 + 0x1c - 8).unwrap_or(' '));
        assert_eq!(octant_glyph(0x10), char::from_u32(0x1cd00 + 0x10 - 7).unwrap_or(' '));
        assert_eq!(octant_glyph(0x54), char::from_u32(0x1cd00 + 0x54 - 12).unwrap_or(' '));
    }

    #[test]
    fn seed_mono_is_the_ascii_quartet() {
        let mono = seed_mono();
        assert_eq!(mono.frames, vec!['-', '\\', '|', '/']);
        assert_eq!(mono.interval_ms, 120);
    }

    /// The computed comet/duet tables carry the reference's frame
    /// COUNTS (30/24 — the lower-octant walks).
    #[test]
    fn computed_walks_match_the_reference_counts() {
        assert_eq!(block_low_comet().len(), 30);
        assert_eq!(block_low_duet().len(), 24);
    }
}

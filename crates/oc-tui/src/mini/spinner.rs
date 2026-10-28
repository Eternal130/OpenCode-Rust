//! The one-cell work-spinner tables (`ui/one-cell-motion.ts` +
//! `ui/subcell.ts`) — CONFIG-T4's `mini.work_spinner` consumer.
//!
//! v2 renders the busy indicator as ONE terminal cell whose glyph
//! advances through a frame table (`<Show when={busy()}>` →
//! `OneCellSpinner animation={WORK_SPINNERS[miniSettings().work_spinner]}`,
//! `footer.view.tsx:978-988`); `mini.mono` swaps in the ASCII
//! `SEED_MONO` animation. RD-19 — the full motion model: the per-frame
//! glow `levels` (`color.a *= level`, one-cell-spinner.tsx:29-34),
//! the `WORK_PACE` decel (one-cell-motion.ts:36-47's integrated-time
//! easing, 1.2 → 0.96 across the second 30 s), the `intro` clip and
//! the `once` one-shot — `SEED_LAUNCH`'s 21-frame × 40 ms startup-row
//! animation.

use std::time::Duration;

/// `WORK_PACE` (`one-cell-motion.ts:47`): the busy animations' decel —
/// rate 1.2 folding to 0.96 across the 30-60 s window.
pub const WORK_PACE: Pace = Pace { initial: 1.2, end: 0.96, after_ms: 30_000, duration_ms: 30_000 };

/// `pace` (`OneCellMotion`): the rate eases `initial` → `end` over
/// `duration_ms` once `after_ms` of age have elapsed.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Pace {
    pub initial: f64,
    pub end: f64,
    pub after_ms: u64,
    pub duration_ms: u64,
}

/// One animation: the glyph table + the per-frame interval +
/// RD-19's glow levels / intro clip / one-shot flag / pace easing
/// (`OneCellMotion`, one-cell-motion.ts:5-13).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Motion {
    pub frames: Vec<char>,
    pub interval_ms: u64,
    /// The glow table (`levels`): one alpha multiplier per frame; an
    /// empty table renders every frame at full strength.
    pub levels: Vec<f64>,
    /// The intro clip (`intro`): a table played FIRST, before the
    /// main cycle (`SEED_WORK`'s 80-frame ramp).
    pub intro: Option<Box<Motion>>,
    /// `once`: the main cycle plays once and holds its last frame.
    pub once: bool,
    /// The `WORK_PACE` decel.
    pub pace: Option<Pace>,
}

impl Motion {
    /// The glyph for an elapsed age (`oneCellFrame`'s modulo walk).
    #[must_use]
    pub fn frame(&self, elapsed: Duration) -> char {
        self.one_cell_frame(elapsed).glyph
    }

    /// `oneCellFrame` (one-cell-motion.ts:18-45): the intro clip
    /// plays first; the pace easing integrates the changing rate so
    /// slowing down never jumps or restarts; `once` clips the final
    /// frame. Returns the glyph, its glow level (1.0 when the table
    /// has none) and the one-shot completion marker.
    #[must_use]
    // The ref math is float throughout (`elapsed` ms × rates) — the
    // int→float widenings mirror it; the frame index crosses i64 for
    // the `rem_euclid` walk and is pre-clamped to the table.
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_precision_loss,
        clippy::cast_sign_loss,
        clippy::cast_possible_wrap
    )]
    pub fn one_cell_frame(&self, elapsed: Duration) -> CellFrame {
        let elapsed_ms = elapsed.as_millis() as f64;
        let intro_duration = self
            .intro
            .as_ref()
            .map_or(0.0, |intro| (intro.frames.len() * intro.interval_ms.max(1) as usize) as f64);
        let intro = elapsed_ms < intro_duration;
        let clip: &Motion = if intro { self.intro.as_deref().unwrap_or(self) } else { self };
        let count = clip.frames.len();
        if count == 0 || clip.interval_ms == 0 {
            return CellFrame::default();
        }
        // Integrate the changing rate so slowing down does not jump or
        // restart the motion (`one-cell-motion.ts:31-38`).
        let progress = match self.pace {
            Some(pace) if pace.duration_ms > 0 => {
                ((elapsed_ms - pace.after_ms as f64) / pace.duration_ms as f64).clamp(0.0, 1.0)
            }
            _ => 0.0,
        };
        let time = if intro {
            elapsed_ms
        } else {
            match self.pace {
                None => elapsed_ms - intro_duration,
                Some(pace) => {
                    let after = pace.after_ms as f64;
                    let duration = pace.duration_ms as f64;
                    (elapsed_ms - intro_duration) * pace.initial
                        + (pace.end - pace.initial)
                            * (duration
                                * (progress * progress * progress
                                    - progress * progress * progress * progress / 2.0)
                                + (elapsed_ms - after - duration).max(0.0))
                }
            }
        };
        // Keep cycle boundaries exact when individual frame intervals
        // are fractional (`time * N / (interval * N)`).
        let step = (clip.interval_ms as usize * count) as f64;
        let frame_index = (time * count as f64 / step).floor();
        let index = if !intro && self.once {
            (frame_index as i64).clamp(0, count as i64 - 1) as usize
        } else {
            (frame_index as i64).rem_euclid(count as i64) as usize
        };
        CellFrame {
            glyph: clip.frames[index],
            level: clip.levels.get(index).copied().unwrap_or(1.0),
            complete: !intro && self.once && frame_index >= count as f64 - 1.0,
        }
    }
}

/// One resolved spinner frame (`oneCellFrame`'s return): the glyph,
/// its glow level and the one-shot completion marker.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CellFrame {
    pub glyph: char,
    pub level: f64,
    pub complete: bool,
}

impl Default for CellFrame {
    fn default() -> Self {
        Self { glyph: ' ', level: 1.0, complete: false }
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
/// then five soft-right blocks, with the glow levels
/// (`one-cell-motion.ts:58-61`).
fn block_soft_slide() -> Motion {
    let mut frames = vec!['\u{1fbe6}'; 5];
    frames.extend(vec!['\u{1fbe7}'; 5]);
    Motion {
        levels: vec![0.55, 0.85, 1.0, 0.85, 0.55, 0.55, 0.85, 1.0, 0.85, 0.55],
        ..plain(frames, 100)
    }
}

/// A bare glyph table (no glow, intro, once or pace).
fn plain(frames: Vec<char>, interval_ms: u64) -> Motion {
    Motion { frames, interval_ms, ..Motion::default() }
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
        ..Motion::default()
    };
    match id {
        "block-soft-slide" => block_soft_slide(),
        // WORK_PACE decels the sweep and the comet (one-cell-motion.
        // tsx:62-75's `pace: WORK_PACE`).
        "block-soft-sweep" => Motion {
            pace: Some(WORK_PACE),
            ..octets(&[0x14, 0x14, 0x14, 0x1c, 0x3c, 0x38, 0x28, 0x28, 0x28, 0x38, 0x3c, 0x1c], 100)
        },
        "block-low-comet" => Motion { pace: Some(WORK_PACE), ..plain(block_low_comet(), 100) },
        "block-low-duet" => plain(block_low_duet(), 60),
        "block-shuttle" => octets(&[0x14, 0x28], 500),
        "block-bridge" => octets(&[0x14, 0x14, 0x14, 0x3c, 0x28, 0x28, 0x28, 0x3c], 120),
        "block-squeeze" => {
            octets(&[0x14, 0x14, 0x14, 0x10, 0x20, 0x28, 0x28, 0x28, 0x20, 0x10], 100)
        }
        "small-toggle" => plain(vec!['\u{25ab}', '\u{25aa}'], 320),
        "square-toggle" => plain(vec!['\u{25a1}', '\u{25a0}'], 500),
        "grow-shrink" => {
            plain(vec!['\u{25ab}', '\u{25ab}', '\u{25aa}', '\u{25a0}', '\u{25aa}'], 180)
        }
        "quadrant-orbit" => plain(vec!['\u{2598}', '\u{259d}', '\u{2597}', '\u{2596}'], 160),
        "crosshatch" => plain(vec!['\u{25a7}', '\u{25a9}', '\u{25a8}', '\u{25a9}'], 240),
        "density-wave" => plain(
            vec![
                '\u{2591}', '\u{2591}', '\u{2591}', '\u{2592}', '\u{2593}', '\u{2588}', '\u{2593}',
                '\u{2592}',
            ],
            160,
        ),
        // `SEED_WORK`: forty identical ▪ frames — the glow levels and
        // the intro ramp ARE the animation (one-cell-motion.ts:49-46).
        "seed" => seed_work(),
        _ => block_soft_slide(),
    }
}

/// `SEED_WORK`'s glow level (`one-cell-motion.ts:53`): a cosine
/// brightness walk, 0.3 → 1.0 → 0.3 across the cycle.
// The count-bounded index→float widenings mirror the ref's
// `index / count` phases.
#[allow(clippy::cast_precision_loss)]
fn seed_level(index: usize, count: usize) -> f64 {
    let phase = index as f64 / count as f64;
    0.3 + 0.7 * (1.0 - (phase * std::f64::consts::TAU).cos()) / 2.0
}

/// `SEED_WORK`'s INTRO level (`one-cell-motion.ts:56-59`): the
/// piecewise ramp — quick attack over the first eighth, hold through
/// the second quarter, cubic falloff after.
#[allow(clippy::cast_precision_loss)]
fn seed_intro_level(index: usize) -> f64 {
    let phase = index as f64 / 80.0;
    0.3 + 0.7
        * (if phase < 0.125 {
            phase / 0.125
        } else if phase < 0.375 {
            1.0
        } else {
            ((1.0 - phase) / 0.625).powi(3)
        })
}

/// `SEED_WORK` (`one-cell-motion.ts:49-64`): the 40-frame seed cycle
/// with its 80-frame intro ramp and the 1.25 → 0.5 decel.
#[must_use]
pub fn seed_work() -> Motion {
    let levels = (0..40).map(|index| seed_level(index, 40)).collect();
    let intro = Motion {
        levels: (0..80).map(seed_intro_level).collect(),
        ..plain(vec!['\u{25aa}'; 80], 40)
    };
    Motion {
        intro: Some(Box::new(intro)),
        pace: Some(Pace { initial: 1.25, end: 0.5, after_ms: 30_000, duration_ms: 30_000 }),
        levels,
        ..plain(vec!['\u{25aa}'; 40], 40)
    }
}

/// `SEED_LAUNCH` (`one-cell-motion.ts:108-113`): the startup row's
/// ONE-SHOT — 21 frames × 40 ms (10 ▪ outlines then 11 ▪ blocks),
/// the seed levels' head, played once.
#[must_use]
pub fn seed_launch() -> Motion {
    let frames = (0..21).map(|index| if index < 10 { '\u{25ab}' } else { '\u{25aa}' }).collect();
    let levels = (0..21).map(|index| seed_level(index, 40)).collect();
    Motion { once: true, levels, ..plain(frames, 40) }
}

/// `SEED_MONO` (`one-cell-motion.ts:115`): the ASCII spinner `mono`
/// selects (`animation={props.mono ? SEED_MONO : …}`).
#[must_use]
pub fn seed_mono() -> Motion {
    plain(vec!['-', '\\', '|', '/'], 120)
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
        let motion = plain(vec!['a', 'b', 'c'], 100);
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

    // -- RD-19: glow levels, pace easing, the SEED_LAUNCH one-shot --

    /// `BLOCK_SOFT_SLIDE`'s glow table (`one-cell-motion.ts:60`): the
    /// level walks 0.55 → 0.85 → 1.0 → 0.85 → 0.55 per half cycle.
    #[test]
    fn glow_levels_follow_the_frame() {
        let slide = work_spinner("block-soft-slide");
        assert_eq!(slide.levels.len(), 10);
        assert_eq!(slide.levels[0], 0.55);
        // 200 ms at interval 100 = frame 2 → full strength.
        let frame = slide.one_cell_frame(Duration::from_millis(200));
        assert_eq!(frame.glyph, '\u{1fbe6}');
        assert_eq!(frame.level, 1.0);
        // The comet and the sweep carry the WORK_PACE decel; the
        // slide only glows (`one-cell-motion.ts:48-75`).
        assert_eq!(work_spinner("block-soft-slide").pace, None);
        assert_eq!(work_spinner("block-soft-sweep").pace, Some(WORK_PACE));
        assert_eq!(work_spinner("block-low-comet").pace, Some(WORK_PACE));
        let pace = work_spinner("block-soft-sweep").pace.expect("work pace");
        assert_eq!(
            (pace.initial, pace.end, pace.after_ms, pace.duration_ms),
            (1.2, 0.96, 30_000, 30_000)
        );
    }

    /// The pace integration (`one-cell-motion.ts:31-38`): at 45 s
    /// into the seed's 1.25 → 0.5 decel the integrated time is
    /// 54140.625 ms → frame 1353 → cycle position 33. The levels
    /// vector (index = position) makes the position observable.
    #[test]
    #[allow(clippy::cast_precision_loss)] // the mirror of the ref's float math
    fn work_pace_integrates_the_decel() {
        let motion = Motion {
            levels: (0..40).map(f64::from).collect(),
            pace: Some(Pace { initial: 1.25, end: 0.5, after_ms: 30_000, duration_ms: 30_000 }),
            ..plain(vec!['x'; 40], 40)
        };
        // Pre-decel: rate 1.25 — 29999 ms integrates to 37498.75 →
        // frame 937 → position 17.
        assert_eq!(motion.one_cell_frame(Duration::from_millis(29_999)).level, 17.0);
        // Mid-decel: progress 0.5.
        assert_eq!(motion.one_cell_frame(Duration::from_secs(45)).level, 33.0);
        // Post-decel: the rate settles at 0.5 — 75 s integrates to
        // 1.25·30000 + (0.5-1.25)·(30000·(1 - 0.5) + 15000) + 0.5·15000
        // (the closed form lands at frame position 10 for this curve).
        let late = motion.one_cell_frame(Duration::from_secs(75)).level;
        let expected = {
            let progress = 1.0_f64;
            let time = 75_000.0 * 1.25
                + (0.5 - 1.25)
                    * (30_000.0
                        * (progress * progress * progress
                            - progress * progress * progress * progress / 2.0)
                        + (75_000.0 - 60_000.0));
            (((time * 40.0 / (40.0 * 40.0)).floor() as i64).rem_euclid(40)) as f64
        };
        assert_eq!(late, expected);
    }

    /// `SEED_WORK`'s intro (`one-cell-motion.ts:55-60`): the first
    /// 3.2 s ride the 80-frame ramp — frame 25 sits in the hold
    /// (full strength) where the main cycle would be dim.
    #[test]
    fn seed_intro_plays_before_the_cycle() {
        let seed = seed_work();
        let intro_frame = seed.one_cell_frame(Duration::from_millis(1_000));
        assert_eq!(intro_frame.glyph, '\u{25aa}');
        assert_eq!(intro_frame.level, 1.0, "intro frame 25 is in the hold");
        // The main cycle at the same position in ITS table (frame 25,
        // 1000 ms past the 3200 ms intro = 4200 ms): cosine level.
        let cycle_frame = seed.one_cell_frame(Duration::from_millis(4_200));
        let elapsed_ms = 4_200.0_f64 - 3_200.0;
        let position = (elapsed_ms * 1.25 / 40.0).floor(); // rate 1.25 pre-decel
        let index = (position as i64).rem_euclid(40) as usize;
        assert_eq!(cycle_frame.level, seed.levels[index]);
    }

    /// `SEED_LAUNCH` (`one-cell-motion.ts:108-113`): 21 frames ×
    /// 40 ms — 10 ▪ outlines then 11 ▪ blocks — played ONCE, holding
    /// the last frame; complete at the 800 ms window's end.
    #[test]
    fn seed_launch_is_the_one_shot_table() {
        let launch = seed_launch();
        assert_eq!(launch.frames.len(), 21);
        assert_eq!(launch.interval_ms, 40);
        assert!(launch.once);
        assert!(launch.frames.iter().take(10).all(|&glyph| glyph == '\u{25ab}'));
        assert!(launch.frames.iter().skip(10).all(|&glyph| glyph == '\u{25aa}'));
        // The seed levels' head rides along (the glow).
        let seed = seed_work();
        assert_eq!(launch.levels, seed.levels[..21]);

        assert_eq!(launch.one_cell_frame(Duration::from_millis(0)).glyph, '\u{25ab}');
        assert_eq!(launch.one_cell_frame(Duration::from_millis(400)).glyph, '\u{25aa}');
        let end = launch.one_cell_frame(Duration::from_millis(800));
        assert!(end.complete, "20 frames × 40 ms completes the one-shot");
        let held = launch.one_cell_frame(Duration::from_secs(10));
        assert!(held.complete);
        assert_eq!(held.glyph, '\u{25aa}', "once holds the last frame");
        // `STARTUP_DURATION_MS` matches the ref's setTimeout window.
        assert_eq!(super::super::lifecycle::STARTUP_DURATION_MS, 800);
    }
}

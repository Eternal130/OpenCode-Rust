//! The running-state knight-rider gradient spinner — port of v2
//! `ui/spinner.ts` (`createFrames` / `createColors`, the
//! `opentui-spinner` trail model) wired at `component/prompt/
//! index.tsx:1892` as `<spinner frames={createFrames({color, style:
//! "blocks", inactiveFactor: 0.6, minAlpha: 0.3})} interval={40}>`.
//!
//! The scanner sweeps a bidirectional lead across `width` cells
//! (forward, hold at the end, backward, hold at the start) with an
//! alpha-decayed trail behind the movement direction; inactive cells
//! dim toward the base color at `inactive_factor`, and hold/movement
//! frames globally fade between `min_alpha` and full strength. Colors
//! are RGBA — the ref blends over the terminal background — so the
//! painters blend each cell over the page fill for the opaque cell
//! grid.

use oc_theme::Rgba;

/// The spinner's interval (`<spinner interval={40}>`, index.tsx:1892).
pub const SPINNER_INTERVAL_MS: u64 = 40;

/// `KnightRiderStyle` (`"blocks" | "diamonds"`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum KnightStyle {
    #[default]
    Blocks,
    Diamonds,
}

/// `KnightRiderOptions` (spinner.ts:316-355) — the composer's shape:
/// a single base `color` derives the trail + inactive colors.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct KnightOptions {
    /// `width` (default 8).
    pub width: usize,
    pub style: KnightStyle,
    /// `holdStart` (default 30).
    pub hold_start: usize,
    /// `holdEnd` (default 9).
    pub hold_end: usize,
    /// `color` — the single bright color the trail derives from.
    pub color: Rgba,
    /// `trailSteps` (default 6).
    pub trail_steps: usize,
    /// `inactiveFactor` (default 0.2; the composer passes 0.6).
    pub inactive_factor: f64,
    /// `enableFading` (default true).
    pub enable_fading: bool,
    /// `minAlpha` (default 0; the composer passes 0.3).
    pub min_alpha: f64,
}

impl Default for KnightOptions {
    fn default() -> Self {
        Self {
            width: 8,
            style: KnightStyle::Blocks,
            hold_start: 30,
            hold_end: 9,
            color: Rgba::from_ints(0xff, 0, 0, 0xff),
            trail_steps: 6,
            inactive_factor: 0.2,
            enable_fading: true,
            min_alpha: 0.0,
        }
    }
}

/// One RGBA color as f64 units (the ref's `RGBA` floats).
#[derive(Debug, Clone, Copy, PartialEq)]
struct Frgba {
    r: f64,
    g: f64,
    b: f64,
    a: f64,
}

impl Frgba {
    fn from_rgba(color: Rgba) -> Self {
        Self {
            r: f64::from(color.r) / 255.0,
            g: f64::from(color.g) / 255.0,
            b: f64::from(color.b) / 255.0,
            a: f64::from(color.a) / 255.0,
        }
    }

    fn to_rgba(self) -> Rgba {
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        fn byte(value: f64) -> u8 {
            (value.clamp(0.0, 1.0) * 255.0).round() as u8
        }
        Rgba::from_ints(byte(self.r), byte(self.g), byte(self.b), byte(self.a))
    }
}

/// `deriveTrailColors` (spinner.ts:236-263): the lead at full
/// brightness/opacity, position 1 a slight bloom (×1.15, α 0.9),
/// then exponential α decay (`0.65^(i-1)`).
fn derive_trail_colors(bright: Rgba, steps: usize) -> Vec<Frgba> {
    let base = Frgba::from_rgba(bright);
    (0..steps)
        .map(|index| {
            let (alpha, brightness) = if index == 0 {
                (1.0, 1.0)
            } else if index == 1 {
                (0.9, 1.15)
            } else {
                (0.65_f64.powi((index - 1) as i32), 1.0)
            };
            Frgba {
                r: (base.r * brightness).min(1.0),
                g: (base.g * brightness).min(1.0),
                b: (base.b * brightness).min(1.0),
                a: alpha,
            }
        })
        .collect()
}

/// `deriveInactiveColor` (spinner.ts:272-278): the base color at
/// `factor` alpha.
fn derive_inactive_color(bright: Rgba, factor: f64) -> Frgba {
    let base = Frgba::from_rgba(bright);
    Frgba { a: factor, ..base }
}

/// `ScannerState` (spinner.ts:17-28) — the bidirectional arm only
/// (the port's only direction).
struct ScannerState {
    active_position: usize,
    is_holding: bool,
    hold_progress: usize,
    hold_total: usize,
    movement_progress: usize,
    movement_total: usize,
    is_moving_forward: bool,
}

/// `getScannerState`'s bidirectional arm (spinner.ts:31-88): forward
/// (`width` frames) → hold at the end (`hold_end`) → backward
/// (`width-1`) → hold at the start (`hold_start`).
fn scanner_state(
    frame_index: usize,
    total_chars: usize,
    hold_start: usize,
    hold_end: usize,
) -> ScannerState {
    let forward_frames = total_chars;
    let hold_end_frames = hold_end;
    let backward_frames = total_chars - 1;
    if frame_index < forward_frames {
        ScannerState {
            active_position: frame_index,
            is_holding: false,
            hold_progress: 0,
            hold_total: 0,
            movement_progress: frame_index,
            movement_total: forward_frames,
            is_moving_forward: true,
        }
    } else if frame_index < forward_frames + hold_end_frames {
        ScannerState {
            active_position: total_chars - 1,
            is_holding: true,
            hold_progress: frame_index - forward_frames,
            hold_total: hold_end_frames,
            movement_progress: 0,
            movement_total: 0,
            is_moving_forward: true,
        }
    } else if frame_index < forward_frames + hold_end_frames + backward_frames {
        let backward_index = frame_index - forward_frames - hold_end_frames;
        ScannerState {
            active_position: total_chars - 2 - backward_index,
            is_holding: false,
            hold_progress: 0,
            hold_total: 0,
            movement_progress: backward_index,
            movement_total: backward_frames,
            is_moving_forward: false,
        }
    } else {
        ScannerState {
            active_position: 0,
            is_holding: true,
            hold_progress: frame_index - forward_frames - hold_end_frames - backward_frames,
            hold_total: hold_start,
            movement_progress: 0,
            movement_total: 0,
            is_moving_forward: false,
        }
    }
}

/// `calculateColorIndex` (spinner.ts:91-127): the trail's color index
/// behind the movement direction — `directional_distance` while
/// moving, `distance + hold_progress` while holding (the trail fades
/// by shifting), `-1` outside the trail.
fn calculate_color_index(state: &ScannerState, char_index: usize, trail_length: usize) -> i64 {
    let directional_distance: i64 = if state.is_moving_forward {
        state.active_position as i64 - char_index as i64
    } else {
        char_index as i64 - state.active_position as i64
    };
    if state.is_holding {
        return directional_distance + state.hold_progress as i64;
    }
    if directional_distance > 0 && directional_distance < trail_length as i64 {
        return directional_distance;
    }
    if directional_distance == 0 {
        return 0;
    }
    -1
}

/// The total bidirectional cycle: forward + hold-end + backward +
/// hold-start (spinner.ts:313's `totalFrames`).
#[must_use]
pub fn total_frames(options: &KnightOptions) -> usize {
    options.width + options.hold_end + (options.width - 1) + options.hold_start
}

/// `createFrames` (spinner.ts:270-315): one string per frame, `width`
/// cells each — active cells carry the style's glyph, inactive cells
/// the dim marker.
#[must_use]
pub fn create_frames(options: &KnightOptions) -> Vec<String> {
    let trail = derive_trail_colors(options.color, options.trail_steps);
    let trail_length = trail.len();
    (0..total_frames(options))
        .map(|frame_index| {
            let state =
                scanner_state(frame_index, options.width, options.hold_start, options.hold_end);
            (0..options.width)
                .map(|char_index| {
                    let index = calculate_color_index(&state, char_index, trail_length);
                    match options.style {
                        KnightStyle::Diamonds => {
                            let shapes = ['⬥', '◆', '⬩', '⬪'];
                            if index >= 0 && index < trail_length as i64 {
                                shapes[(index as usize).min(shapes.len() - 1)]
                            } else {
                                '·'
                            }
                        }
                        KnightStyle::Blocks => {
                            if index >= 0 && index < trail_length as i64 {
                                '■'
                            } else {
                                '⬝'
                            }
                        }
                    }
                })
                .collect()
        })
        .collect()
}

/// `createColors` (spinner.ts:335-368) as a per-cell query: the trail
/// color at `index`, or the inactive default — with the global
/// hold/movement fade folded into the default's alpha
/// (`createKnightRiderTrail`, spinner.ts:129-176).
#[must_use]
pub fn create_colors(options: &KnightOptions, frame_index: usize, char_index: usize) -> Rgba {
    let trail = derive_trail_colors(options.color, options.trail_steps);
    let default_base = derive_inactive_color(options.color, options.inactive_factor);
    let state = scanner_state(frame_index, options.width, options.hold_start, options.hold_end);
    let index = calculate_color_index(&state, char_index, trail.len());

    // The global fade (spinner.ts:150-164): linear fade-out to
    // min_alpha across the hold, linear fade-in from min_alpha during
    // movement.
    let mut fade_factor = 1.0;
    if options.enable_fading {
        if state.is_holding && state.hold_total > 0 {
            let progress = (state.hold_progress as f64 / state.hold_total as f64).min(1.0);
            fade_factor = (1.0 - progress * (1.0 - options.min_alpha)).max(options.min_alpha);
        } else if !state.is_holding && state.movement_total > 0 {
            let progress = (state.movement_progress as f64
                / (state.movement_total.saturating_sub(1)).max(1) as f64)
                .min(1.0);
            fade_factor = options.min_alpha + progress * (1.0 - options.min_alpha);
        }
    }
    let inactive = Frgba { a: default_base.a * fade_factor, ..default_base };
    if index == -1 {
        return inactive.to_rgba();
    }
    trail.get(index as usize).map_or(inactive.to_rgba(), |color| color.to_rgba())
}

/// The composer's running-state cell at an elapsed age
/// (prompt/index.tsx:1632-1649 + 1892): the frame string for the
/// elapsed clock (interval 40 ms) and the per-cell color.
#[must_use]
pub fn composer_spinner_cell(options: &KnightOptions, elapsed_ms: u64) -> Vec<(char, Rgba)> {
    let frames = create_frames(options);
    if frames.is_empty() {
        return Vec::new();
    }
    let index = (elapsed_ms / SPINNER_INTERVAL_MS) % frames.len() as u64;
    let frame_index = usize::try_from(index).unwrap_or(0);
    frames[frame_index]
        .chars()
        .enumerate()
        .map(|(char_index, glyph)| (glyph, create_colors(options, frame_index, char_index)))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const RED: Rgba = Rgba::from_ints(0xff, 0, 0, 0xff);

    /// The bidirectional cycle length: 8 + 9 + 7 + 30 = 54 frames,
    /// each 8 cells (spinner.ts:313).
    #[test]
    fn frame_table_shape_matches_the_reference() {
        let options = KnightOptions::default();
        let frames = create_frames(&options);
        assert_eq!(frames.len(), 54, "width + holdEnd + (width-1) + holdStart");
        assert!(frames.iter().all(|frame| frame.chars().count() == 8));
        assert_eq!(total_frames(&options), 54);
    }

    /// The scanner sweeps forward then backward: frame 0's lead is
    /// cell 0, frame 7 holds at cell 7 (blocks style → the LAST
    /// block cell is the lead), and the backward pass (frame 16)
    /// leads at cell 6.
    #[test]
    fn blocks_frames_track_the_scanner() {
        let options = KnightOptions { color: RED, ..KnightOptions::default() };
        let frames = create_frames(&options);
        // Frame 0: lead at 0 → "■⬝⬝⬝⬝⬝⬝⬝" (trail is BEHIND forward
        // movement, so nothing precedes the lead).
        assert_eq!(frames[0], "■⬝⬝⬝⬝⬝⬝⬝");
        // Frame 3: lead at 3 with a 3-cell trail behind it (forward
        // movement → the trail is at LOWER indices).
        assert_eq!(frames[3], "■■■■⬝⬝⬝⬝");
        // Backward pass (frame 8+9=17 is backwardIndex 0 → active 6):
        // the lead at 6 with the trail to its RIGHT (higher indices).
        assert_eq!(frames[17], "⬝⬝⬝⬝⬝⬝■■");
    }

    /// Diamonds style uses the shape ladder for the trail and `·` for
    /// inactive cells (spinner.ts:302-308).
    #[test]
    fn diamonds_style_uses_the_shape_ladder() {
        let options =
            KnightOptions { color: RED, style: KnightStyle::Diamonds, ..KnightOptions::default() };
        let frames = create_frames(&options);
        assert!(frames[0].starts_with('⬥'), "lead is the brightest shape: {}", frames[0]);
        assert!(frames[0].ends_with('·'), "inactive cells are dots: {}", frames[0]);
    }

    /// `deriveTrailColors`: lead full, position 1 blooms (×1.15,
    /// α 0.9), then exponential α decay — checked through
    /// `create_colors` on the trail.
    #[test]
    fn trail_colors_follow_the_reference_derivation() {
        let options = KnightOptions {
            color: Rgba::from_ints(0x80, 0x80, 0x80, 0xff),
            enable_fading: false,
            ..KnightOptions::default()
        };
        // Frame 3, char 3 = the lead (index 0): full brightness.
        let lead = create_colors(&options, 3, 3);
        assert_eq!(lead, Rgba::from_ints(0x80, 0x80, 0x80, 0xff));
        // Char 2 = trail index 1: bloom 0.5·1.15 = 0.575 → 147,
        // α 0.9 → 229/230.
        let bloom = create_colors(&options, 3, 2);
        assert_eq!(bloom, Rgba::from_ints(147, 147, 147, 230));
        // Char 1 = trail index 2: α 0.65 → 166.
        let trail = create_colors(&options, 3, 1);
        assert_eq!(trail, Rgba::from_ints(0x80, 0x80, 0x80, 166));
        // Char 0 = trail index 3: α 0.65² → 108.
        let deep = create_colors(&options, 3, 0);
        assert_eq!(deep, Rgba::from_ints(0x80, 0x80, 0x80, 108));
    }

    /// The inactive default carries `inactive_factor` alpha and the
    /// hold fade walks it toward `min_alpha`
    /// (`createKnightRiderTrail`, spinner.ts:148-164).
    #[test]
    fn inactive_cells_carry_the_factor_and_the_hold_fade() {
        let options = KnightOptions {
            color: RED,
            inactive_factor: 0.6,
            min_alpha: 0.3,
            ..KnightOptions::default()
        };
        // Frame 3 (moving, forward progress 3/7): the fade applies to
        // the INACTIVE cells only through the default's alpha.
        let inactive = create_colors(&options, 3, 7);
        let alpha = f64::from(inactive.a) / 255.0;
        let expected = 0.6 * (0.3 + (3.0 / 7.0) * 0.7);
        assert!(
            (alpha - expected).abs() < 0.01,
            "inactive alpha {alpha} ≈ factor × movement fade {expected}"
        );
        // Deep hold (frame 15, holdProgress 7/9): the fade walks
        // linearly — 1 − (7/9)·0.7 ≈ 0.456.
        let held = create_colors(&options, 15, 0);
        let held_alpha = f64::from(held.a) / 255.0;
        let expected = 0.6 * (1.0 - (7.0 / 9.0) * 0.7);
        assert!((held_alpha - expected).abs() < 0.02, "held alpha {held_alpha} ≈ {expected}");
    }

    /// The composer's clock-driven cell advances every 40 ms and
    /// wraps at the cycle end (index.tsx:1892 `interval={40}`).
    #[test]
    fn composer_cell_advances_at_the_40ms_interval() {
        let options = KnightOptions {
            color: RED,
            style: KnightStyle::Blocks,
            inactive_factor: 0.6,
            min_alpha: 0.3,
            ..KnightOptions::default()
        };
        let first = composer_spinner_cell(&options, 0);
        assert_eq!(first.len(), 8);
        assert_eq!(first[0], ('■', create_colors(&options, 0, 0)));
        let later = composer_spinner_cell(&options, 40 * 3);
        assert_eq!(later, composer_spinner_cell(&options, 3 * SPINNER_INTERVAL_MS));
        // The cycle wraps: 54 frames × 40 ms.
        let wrapped = composer_spinner_cell(&options, 40 * 54);
        assert_eq!(wrapped, first, "the cycle wraps modulo 54 frames");
    }

    /// G-49a — the frame index is derived from ELAPSED TIME at
    /// render (`elapsed_ms/40 % frames.len()`), so the ref's 40 ms
    /// interval MATH holds at any repaint rate: consecutive 40 ms
    /// elapsed steps walk consecutive frames, and even a 100 ms
    /// pump-tick clock (mini/runtime.rs TICK / the app loop's plain
    /// tick) lands on exactly `floor(elapsed/40)` — never off the
    /// 40 ms grid. (A faster whole-frame repaint was evaluated and
    /// rejected: the run loop has no composer-area-only repaint, and
    /// a 40 ms global cadence disturbs the pump — the shared PTY
    /// harness closes bursts only on >80 ms quiet gaps and the
    /// registered temporal tests pin the 100 ms cadence.)
    #[test]
    fn frame_progression_follows_the_40ms_cadence_math() {
        let options = KnightOptions {
            color: RED,
            style: KnightStyle::Blocks,
            inactive_factor: 0.6,
            min_alpha: 0.3,
            ..KnightOptions::default()
        };
        let frames = create_frames(&options);
        let frame_at = |elapsed: u64| -> String {
            composer_spinner_cell(&options, elapsed)
                .iter()
                .map(|(glyph, _)| *glyph)
                .collect()
        };
        // Consecutive 40 ms elapsed steps → consecutive frame indices
        // (the ref's timer advances one frame per step).
        for index in 0..12u64 {
            let elapsed = index * SPINNER_INTERVAL_MS;
            let expected = frames[(elapsed / SPINNER_INTERVAL_MS % frames.len() as u64) as usize]
                .clone();
            assert_eq!(frame_at(elapsed), expected, "elapsed {elapsed} ms → frame {index}");
        }
        // Sub-interval elapsed stays on the SAME frame (39 ms is
        // still frame 0 — the index floors onto the 40 ms grid).
        assert_eq!(frame_at(39), frames[0]);
        assert_eq!(frame_at(41), frames[1]);
        // A 100 ms pump-tick clock samples the same math exactly:
        // index = floor(elapsed/40) % 54 — precise, just visually
        // skipping the in-between frames (0, 2, 5, 7, 10, …).
        let sampled: Vec<u64> = (0..5u64).map(|tick| tick * 100 / SPINNER_INTERVAL_MS).collect();
        assert_eq!(sampled, vec![0, 2, 5, 7, 10]);
        for (tick, index) in sampled.iter().enumerate() {
            let elapsed = tick as u64 * 100;
            assert_eq!(
                frame_at(elapsed),
                frames[(*index % frames.len() as u64) as usize],
                "tick {tick} lands on grid index {index}"
            );
        }
    }
}

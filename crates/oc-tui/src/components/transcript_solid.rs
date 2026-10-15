//! Lane M4: the transcript (the message list) on opentui-solid —
//! the session route's transcript region re-homed the M1/M2/M3 way
//! (see `docs/migration-m1-tabstrip.md`).
//!
//! **What this module is.** The message list the store's message
//! snapshots reduce to — user-message boxes (`┃` agent border +
//! raised fill), exploration/tool rows (running spinner / completed
//! `→` / the subagent flow's card rides the same tool rows), thought
//! blocks (collapsed `+ Thought · Nms` headers, the expanded `-`
//! header + `┃`-guttered children), assistant markdown text parts,
//! stats rows (`Build · model · Nms · X.X tok/s`), `Error: …` rows,
//! and the R-07 image strip's cells — painted as ONE opentui-solid
//! builder tree.
//!
//! **The bridge shape** (the M1/M2/M3 template, verbatim — M3's
//! "share the solver walk" rule at the largest scale):
//!
//! * *inputs*: the transcript geometry comes from the SAME
//!   [`render_transcript`] walk the old painter consumes (row
//!   reduction, markdown, wrap, the disclosure expansion — ONE
//!   solver, both painters). [`shape`] collects the VISIBLE window's
//!   placed runs + fills as pre-shaped [`SolidItem`]s; the store's
//!   hit rows (E1 #29/#34), the scroll metrics, and the
//!   `Transcript.images` overlay set are computed from that same
//!   [`Transcript`] upstream of the flag — UNTOUCHED by this lane
//!   (E1's machinery stays authoritative at the store level; the
//!   solid path only paints).
//! * *reactive shape*: one flat `For` over the window's items keyed
//!   by `{row.key}#{unit}` — the row keys the old walk assigns
//!   (`message:{id}` / `part:{mid}:{pid}` / `footer:{mid}` /
//!   `group:{kind}:{first}` / `thought:{mid}:{pid}`). Streaming
//!   appends patch the tail row's chunk content in place; a scroll
//!   re-windows the `For` (the entering/leaving items mount/unmount
//!   — the Wave-B visible-rows shape, bounded by the viewport, not
//!   the transcript length); a disclosure expansion mounts the
//!   thought children. The mount keys on GEOMETRY ONLY (width,
//!   region height, page bg) — the content height and the scroll
//!   offset are NOT keys (sticky streaming changes the offset every
//!   chunk; that must patch, never rebuild).
//! * *frame out*: the tree paints through a headless
//!   [`SolidRenderer`] into an `OptimizedBuffer`;
//!   [`session_tabs_solid::frame_from_buffer`] copies the cells into
//!   the oc-tui [`Frame`] the route pastes at `(0, content_top)` —
//!   the same paste target the old path's walk painted into.
//!
//! **Window semantics** (the old session walk's pinned drop rules —
//! the parity gate is cell for cell): units (placed items / fills)
//! whose ABSOLUTE transcript y starts above the scroll offset drop
//! WHOLE; the region frame bounds clip the bottom. [`shape`]
//! filters identically, so a partially-scrolled user box loses its
//! top border/fill on BOTH paths.
//!
//! **Markdown.** The old path's markdown bytes come from
//! `message/markdown.rs` (the C1 port of `@opentui/core`
//! `MarkdownRenderable`'s text behavior) INSIDE the shared
//! `render_transcript` walk; this bridge consumes its output as
//! chunks. opentui-core's own `MarkdownRenderable` block pipeline
//! (`renderables/markdown.rs`) is a SEPARATE port of the same
//! upstream — wiring the transcript to IT would fork the two paths'
//! bytes (the fallback contract's parity bar) and is deferred to the
//! old-path's retirement lane; see the lane doc's decision record.
//!
//! **Scroll.** E1's driver (`oc_tui_core::scroll::ScrollDriver`)
//! implements the ported `ScrollBar.ts` setter math
//! (`round(clamp(v, 0, scrollSize - viewportSize))` — the js_round
//! halves-toward-+∞ behavior the long-reply-scroll corpus pins) over
//! the store's `FollowState`. The ported `ScrollBox` renderable
//! moves that state INTO the tree — the store-level seam (metrics,
//! offset, hit rows) would have to be re-derived from the rendered
//! tree, for the same math. The lane keeps the store driver (see
//! `scroll_driver_matches_the_ported_scrollbar_math` for the
//! equivalence evidence); the solid tree is a pure paint of the
//! driver's window.
//!
//! **Selection flag (retired).** The lane-M4 dual-path flag
//! (`OC_TRANSCRIPT_OPENTUI`) is gone: this module's opentui-solid
//! tree is the transcript region's ONLY renderer — the hand-rolled
//! region walk was deleted after the one-release probation (the
//! retirement lane).

// Terminal dimensions/colors are bounded usize→f32 casts (taffy
// points), the same allowance the other solid surfaces carry.
#![allow(clippy::cast_possible_truncation)]
#![allow(clippy::cast_precision_loss)]
#![allow(clippy::cast_sign_loss)]

use std::cell::RefCell;
use std::rc::Rc;

use opentui_core::styled_text::{StyledText, TextChunk};
use opentui_core::Rgba;
use opentui_solid::builder::ForItemSource;
use opentui_solid::element::{ForItem, PropValue};
use opentui_solid::reactive::Reactive;
use opentui_solid::{box_, for_list, map, text, Element, Signal, SolidRenderer};

use crate::components::message::list::Transcript;
use crate::components::session_tabs_solid::frame_from_buffer;
use crate::layout::StyledSpan;
use crate::render::Frame;
use crate::theme::ThemeStyles;

// ---------------------------------------------------------------------------
// colors + chunk helpers (the M1/M2/M3 shape)
// ---------------------------------------------------------------------------

/// An RGB triple (the port's cell currency).
type Rgb = (u8, u8, u8);

fn rgb_of(color: ratatui::style::Color) -> Rgb {
    match color {
        ratatui::style::Color::Rgb(r, g, b) => (r, g, b),
        // The transcript's theme tokens are all truecolor; the
        // fallback only guards a future non-RGB token.
        _ => (128, 128, 128),
    }
}

fn rgba(rgb: Rgb) -> Rgba {
    Rgba::rgb(rgb.0, rgb.1, rgb.2)
}

fn hex(rgb: Rgb) -> String {
    format!("#{:02x}{:02x}{:02x}", rgb.0, rgb.1, rgb.2)
}

/// ratatui modifier bits → opentui's ECMA-ordered attribute word
/// (bold 1<<0 … hidden 1<<6, strikethrough 1<<7 = SGR 9) — the exact
/// inverse of the bits `render::paint::modifier_bits` writes, so the
/// two painters' cells carry the same attribute set (strikethrough's
/// bit-8 remap below is the one documented asymmetry).
fn ecma_attributes(modifier: ratatui::style::Modifier) -> u32 {
    use ratatui::style::Modifier;
    let mut bits = 0u32;
    if modifier.contains(Modifier::BOLD) {
        bits |= 1 << 0;
    }
    if modifier.contains(Modifier::DIM) {
        bits |= 1 << 1;
    }
    if modifier.contains(Modifier::ITALIC) {
        bits |= 1 << 2;
    }
    if modifier.contains(Modifier::UNDERLINED) {
        bits |= 1 << 3;
    }
    if modifier.intersects(Modifier::SLOW_BLINK | Modifier::RAPID_BLINK) {
        bits |= 1 << 4;
    }
    if modifier.contains(Modifier::REVERSED) {
        bits |= 1 << 5;
    }
    if modifier.contains(Modifier::HIDDEN) {
        bits |= 1 << 6;
    }
    if modifier.contains(Modifier::CROSSED_OUT) {
        bits |= 1 << 7;
    }
    bits
}

/// One placed run line → styled chunks. `fg`/`bg` `None` where the
/// run carries none (fg-less ⇒ the renderer default; bg-less ⇒ the
/// chunk BLENDS onto the destination — the fill under it — exactly
/// the old painter's `set_style`-keeps-None semantics).
fn line_chunks(runs: &[StyledSpan]) -> StyledText {
    let chunks = runs
        .iter()
        .filter(|run| !run.text.is_empty())
        .map(|run| TextChunk {
            text: run.text.clone(),
            fg: run.style.fg.map(|color| rgba(rgb_of(color))),
            bg: run.style.bg.map(|color| rgba(rgb_of(color))),
            attributes: ecma_attributes(run.style.add_modifier),
            link: None,
        })
        .collect();
    StyledText::new(chunks)
}

// ---------------------------------------------------------------------------
// inputs (the shared walk → view bridge)
// ---------------------------------------------------------------------------

/// One visible paint unit — a pre-split placed LINE (text) or a
/// background fill rect — at ABSOLUTE transcript coordinates.
#[derive(Debug, Clone)]
struct SolidItem {
    /// `{row.key}#{unit}` — stable across frames (streaming patches
    /// content under the same key; the For reconciles on it).
    key: String,
    left: usize,
    /// Absolute transcript y (the scroll offset is applied in the
    /// reactive `top` maps, never baked here — sticky streaming
    /// changes it every chunk).
    top: usize,
    body: ItemBody,
}

#[derive(Debug, Clone)]
enum ItemBody {
    Text(StyledText),
    Fill { width: usize, height: usize, color: Rgb },
}

/// One frame's transcript snapshot — the ONLY input the tree reads.
/// Geometry (width/height/bg) is MOUNT-STATIC; the item window and
/// the offset patch in place.
#[derive(Clone)]
struct TranscriptInputs {
    items: Vec<SolidItem>,
    scroll_offset: usize,
}

/// Collect the visible window's paint units from the SHARED
/// [`Transcript`] walk — the same drop rules the old region painter
/// applies (units starting above the offset drop whole; the bottom
/// clips at the region bounds, which also keeps the `For` bounded by
/// the viewport — the Wave-B visible-rows window).
fn shape(
    transcript: &Transcript,
    scroll_offset: usize,
    width: usize,
    height: usize,
) -> Vec<SolidItem> {
    let mut items = Vec::new();
    for row in &transcript.rows {
        if row.y + row.height <= scroll_offset {
            continue;
        }
        if row.y.saturating_sub(scroll_offset) >= height {
            break;
        }
        // The row's FILL rects first, then its text units — the For's
        // DOM order is the paint order (later siblings overwrite):
        // the old painter stamps every fill before any text, and a
        // row's fill always covers that row's own glyphs (rows never
        // share rows), so per-row fills-first mirrors it exactly.
        for (index, fill) in row.fills.iter().enumerate() {
            let top = usize::from(fill.y);
            if top < scroll_offset {
                continue;
            }
            if top.saturating_sub(scroll_offset) >= height {
                continue;
            }
            items.push(SolidItem {
                key: format!("{}#f{index}", row.key),
                left: usize::from(fill.x),
                top,
                body: ItemBody::Fill {
                    width: usize::from(fill.width),
                    height: usize::from(fill.height),
                    color: rgb_of(fill.style.bg.unwrap_or(ratatui::style::Color::Reset)),
                },
            });
        }
        for (index, placed) in row.placed.iter().enumerate() {
            if placed.y < scroll_offset {
                continue; // the unit starts above the window
            }
            for (line_index, line) in placed.lines.iter().enumerate() {
                let top = placed.y + line_index;
                if top.saturating_sub(scroll_offset) >= height {
                    continue; // below the region (the For window bound)
                }
                items.push(SolidItem {
                    key: format!("{}#t{index}.{line_index}", row.key),
                    left: placed.x,
                    top,
                    body: ItemBody::Text(line_chunks(&line.runs)),
                });
            }
        }
    }
    let _ = width; // the wrap pre-shaped the content; the raster clips
    items
}

// ---------------------------------------------------------------------------
// the element tree
// ---------------------------------------------------------------------------

/// `dyn_(src, to PropValue::Num)` — a usize-valued dynamic prop.
fn dyn_num<S>(source: S) -> Rc<dyn opentui_solid::DynPropSource>
where
    S: Reactive<usize> + 'static,
{
    opentui_solid::dyn_(source, |value: &usize| PropValue::Num(*value as f32))
}

/// `dyn_(src, to PropValue::Color)` — an Rgb-valued dynamic prop.
fn dyn_color<S>(source: S) -> Rc<dyn opentui_solid::DynPropSource>
where
    S: Reactive<Rgb> + 'static,
{
    opentui_solid::dyn_(source, |value: &Rgb| PropValue::Color(hex(*value)))
}

/// `dyn_(src, to PropValue::Styled)` — styled-chunk content.
fn dyn_styled<S>(source: S) -> Rc<dyn opentui_solid::DynPropSource>
where
    S: Reactive<StyledText> + 'static,
{
    opentui_solid::dyn_(source, |value: &StyledText| PropValue::Styled(value.clone()))
}

/// One `For` item element: a text unit (absolute, natural width,
/// pre-wrapped content — `wrapMode none` + `flexShrink 0` so nothing
/// re-wraps or shrinks) or a fill rect (absolute box, explicit
/// points). `top` is the DYNAMIC scroll shift; `left` patches too
/// (a re-shape can move a unit under its key). The unit's KIND is
/// baked into its key (`#t` text / `#f` fill) — stable per identity,
/// so the element type is chosen once at mount.
fn item_element(inputs: &Signal<TranscriptInputs>, key: &str) -> Element {
    let key = key.to_owned();
    let is_fill = inputs
        .get()
        .items
        .iter()
        .any(|item| item.key == key && matches!(item.body, ItemBody::Fill { .. }));
    let left_of = {
        let key = key.clone();
        move |inputs: &TranscriptInputs| {
            inputs.items.iter().find(|item| item.key == key).map_or(0, |item| item.left)
        }
    };
    let top_of = {
        let key = key.clone();
        move |inputs: &TranscriptInputs| {
            inputs
                .items
                .iter()
                .find(|item| item.key == key)
                .map_or(0, |item| item.top.saturating_sub(inputs.scroll_offset))
        }
    };
    if is_fill {
        box_()
            .position("absolute")
            .dyn_prop("left", dyn_num(map(inputs.clone(), left_of)))
            .dyn_prop("top", dyn_num(map(inputs.clone(), top_of)))
            .dyn_prop(
                "width",
                dyn_num(map(inputs.clone(), {
                    let key = key.clone();
                    move |inputs: &TranscriptInputs| {
                        inputs.items.iter().find(|item| item.key == key).map_or(0, |item| {
                            match &item.body {
                                ItemBody::Fill { width, .. } => *width,
                                ItemBody::Text(_) => 0,
                            }
                        })
                    }
                })),
            )
            .dyn_prop(
                "height",
                dyn_num(map(inputs.clone(), {
                    let key = key.clone();
                    move |inputs: &TranscriptInputs| {
                        inputs.items.iter().find(|item| item.key == key).map_or(0, |item| {
                            match &item.body {
                                ItemBody::Fill { height, .. } => *height,
                                ItemBody::Text(_) => 0,
                            }
                        })
                    }
                })),
            )
            .dyn_prop(
                "backgroundColor",
                dyn_color(map(inputs.clone(), {
                    let key = key.clone();
                    move |inputs: &TranscriptInputs| {
                        inputs.items.iter().find(|item| item.key == key).map_or((0, 0, 0), |item| {
                            match &item.body {
                                ItemBody::Fill { color, .. } => *color,
                                ItemBody::Text(_) => (0, 0, 0),
                            }
                        })
                    }
                })),
            )
            .into_element()
    } else {
        text()
            .position("absolute")
            .wrap_mode("none")
            .flex_shrink(0.0)
            .dyn_prop("left", dyn_num(map(inputs.clone(), left_of)))
            .dyn_prop("top", dyn_num(map(inputs.clone(), top_of)))
            .dyn_prop(
                "content",
                dyn_styled(map(inputs.clone(), {
                    let key = key.clone();
                    move |inputs: &TranscriptInputs| {
                        inputs.items.iter().find(|item| item.key == key).map_or_else(
                            || StyledText::new(Vec::new()),
                            |item| match &item.body {
                                ItemBody::Text(chunks) => chunks.clone(),
                                ItemBody::Fill { .. } => StyledText::new(Vec::new()),
                            },
                        )
                    }
                })),
            )
            .into_element()
    }
}

/// The transcript region mount: a positioned root carrying the page
/// background, one flat keyed `For` of the window's absolute units.
/// Geometry bakes at build time (the mount key); every content slot
/// is a dyn prop over the inputs signal.
fn transcript_element(
    inputs: &Signal<TranscriptInputs>,
    width: usize,
    height: usize,
    bg: Rgb,
) -> Element {
    let source = ForItemSource {
        source: map(inputs.clone(), |inputs: &TranscriptInputs| {
            inputs
                .items
                .iter()
                .map(|item| ForItem {
                    key: item.key.clone(),
                    value: PropValue::Str(item.key.clone()),
                })
                .collect::<Vec<ForItem>>()
        }),
    };
    let item_inputs = inputs.clone();
    let units = for_list(Rc::new(source), move |item: &ForItem, _index: usize| {
        item_element(&item_inputs, &item.key)
    });
    box_()
        .width(width as f32)
        .height(height as f32)
        .position("relative")
        .background_color(hex(bg))
        .children_dyn(Rc::new(units))
        .into_element()
}

// ---------------------------------------------------------------------------
// the mount (one per thread, keyed on geometry)
// ---------------------------------------------------------------------------

/// The mounted transcript: the inputs signal + a real headless
/// [`SolidRenderer`]. Rebuilt when the region geometry changes
/// (resize, the composer's mount-height ladder moving the region
/// height, a theme swap) — the M2 height-keying rule. The content
/// height and the scroll offset are deliberately NOT keys: sticky
/// streaming changes the offset every chunk and must patch.
struct SolidTranscript {
    inputs: Signal<TranscriptInputs>,
    renderer: SolidRenderer,
    key: (usize, usize, Rgb),
}

impl SolidTranscript {
    fn new(width: usize, height: usize, bg: Rgb, inputs: &TranscriptInputs) -> Self {
        let signal = Signal::new(inputs.clone());
        let renderer = opentui_solid::render(
            opentui_core::renderer::CliRendererConfig {
                width: width.max(1) as u32,
                height: height.max(1) as u32,
                background_color: rgba(bg),
                ..opentui_core::renderer::CliRendererConfig::default()
            },
            transcript_element(&signal, width, height, bg),
        );
        Self { inputs: signal, renderer, key: (width, height, bg) }
    }

    /// Sync one frame's snapshot, flush the reconciler patches, and
    /// render — the per-frame bridge cost.
    fn paint(&mut self, inputs: TranscriptInputs) {
        self.inputs.set(inputs);
        self.renderer.flush();
    }
}

thread_local! {
    /// One transcript mount per thread-process — the app paints
    /// exactly one per frame; the harnesses reuse it across
    /// scenarios.
    static TRANSCRIPT: RefCell<Option<SolidTranscript>> = const { RefCell::new(None) };
}

/// Paint the transcript region through the opentui-solid tree — the
/// ONLY renderer since the retirement lane: shape the visible
/// window from the SHARED walk, sync + flush the mount, copy the
/// cells, then apply the ONE bridge-level fixup (the strikethrough
/// bit: opentui packs SGR 9 at bit 7, the cell grid at bit 8 —
/// `~~del~~` markdown must land on the grid's bit).
#[must_use]
pub fn paint_transcript_region(
    transcript: &Transcript,
    theme: &ThemeStyles,
    size: (u16, u16),
    scroll_offset: usize,
) -> Frame {
    let (width, height) = (usize::from(size.0), usize::from(size.1));
    if width == 0 || height == 0 {
        // The old region painter's degenerate shape.
        return Frame { cells: vec![Vec::new(); height], width, height };
    }
    let items = shape(transcript, scroll_offset, width, height);
    let bg = rgb_of(theme.background);
    TRANSCRIPT.with_borrow_mut(|slot| {
        let mount = slot.get_or_insert_with(|| {
            SolidTranscript::new(
                width,
                height,
                bg,
                &TranscriptInputs { items: items.clone(), scroll_offset },
            )
        });
        if mount.key != (width, height, bg) {
            *mount = SolidTranscript::new(
                width,
                height,
                bg,
                &TranscriptInputs { items: items.clone(), scroll_offset },
            );
        }
        mount.paint(TranscriptInputs { items, scroll_offset });
        let buffer = &mount.renderer.renderer.current_render_buffer;
        let mut frame = frame_from_buffer(buffer, &[]);
        // Strikethrough bit remap (see the doc comment above).
        for row in &mut frame.cells {
            for cell in row {
                if cell.attrs & (1 << 8) != 0 {
                    cell.attrs = (cell.attrs & !(1 << 8)) | (1 << 7);
                }
            }
        }
        frame
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::components::message::list::{
        message_groups_fixture, render_transcript, vscroll_messages_fixture, TranscriptOptions,
    };
    use crate::components::message::model::{Message, Part};
    use crate::theme::ThemeState;

    fn theme() -> ThemeStyles {
        ThemeState::v2_dark().styles()
    }

    /// Streaming churn + scroll re-windowing patch the mounted tree
    /// WITHOUT a rebuild (the mount key is geometry-only): append
    /// text to the tail turn, scroll away and back, expand the
    /// disclosure — every frame paints its region and the reconciler
    /// records its ops (absolute since the retirement).
    #[test]
    fn streaming_and_scroll_patch_the_mounted_transcript() {
        let theme = theme();
        let mut messages = vscroll_messages_fixture();
        let group = {
            let groups = message_groups_fixture();
            let transcript = render_transcript(&groups, &TranscriptOptions::default(), &theme, 80);
            transcript
                .rows
                .iter()
                .find(|row| row.key.starts_with("group:Reasoning:"))
                .map(|row| row.key.clone())
                .expect("reasoning group")
        };
        messages.extend(message_groups_fixture());
        let (width, height) = (80usize, 16usize);
        let mut options = TranscriptOptions::default();
        for step in 0..6 {
            // Grow the tail text (streaming), then move the offset
            // (scroll), then toggle the disclosure (expand).
            match step {
                3 => options.expanded_groups = vec![group.clone()],
                4 => options.expanded_groups = Vec::new(),
                _ => {}
            }
            let transcript = render_transcript(&messages, &options, &theme, width);
            let maximum = transcript.content_height.saturating_sub(height);
            let offset = match step {
                0 | 5 => maximum,
                1 => 0,
                _ => maximum / 2,
            };
            let frame =
                paint_transcript_region(&transcript, &theme, (width as u16, height as u16), offset);
            assert_eq!(frame.height, height, "region height at step {step}");
            if step == 2 {
                // Stream one more chunk into the tail assistant.
                if let Some(Message::Assistant { parts, .. }) = messages
                    .iter_mut()
                    .rev()
                    .find(|message| matches!(message, Message::Assistant { .. }))
                {
                    for part in parts {
                        if let Part::Text { text, .. } = part {
                            text.push_str(" …and the stream continues past the wrap.");
                        }
                    }
                }
            }
        }
        TRANSCRIPT.with_borrow(|slot| {
            let mount = slot.as_ref().expect("transcript mounted");
            assert!(!mount.renderer.ops_log().is_empty(), "the reconciler recorded its ops");
        });
    }

    /// A resize (the composer's height ladder shrinking the region)
    /// rebuilds cleanly — the mount-key rule (absolute self-check
    /// since the retirement).
    #[test]
    fn resize_rebuilds_cleanly() {
        let theme = theme();
        let messages = vscroll_messages_fixture();
        for (width, height) in [(80usize, 16usize), (40, 16), (120, 23), (80, 14)] {
            let transcript =
                render_transcript(&messages, &TranscriptOptions::default(), &theme, width);
            let offset = transcript.content_height.saturating_sub(height);
            let frame =
                paint_transcript_region(&transcript, &theme, (width as u16, height as u16), offset);
            assert_eq!(frame.width, width, "full-width region at {width}x{height}");
        }
    }

    /// The scroll decision's evidence pin: E1's store driver lands on
    /// EXACTLY the ported `ScrollBar` math (`round(clamp(value, 0,
    /// scrollSize - viewportSize))` — `scrollbar.rs`'s
    /// `clamp_scroll_position`) for every command over a
    /// viewport/content/start matrix — moving the viewport onto
    /// opentui-core's `ScrollBox` renderable would re-derive the same
    /// integers from inside the tree, so the store driver stays (its
    /// `FollowState` is what the hit rows, the metrics and the jump
    /// indicator read; the long-reply-scroll corpus is
    /// flag-symmetric either way).
    #[test]
    fn scroll_driver_matches_the_ported_scrollbar_math() {
        use oc_tui_core::scroll::{FollowState, ScrollCommand, ScrollDriver};
        for viewport in [10usize, 15, 16, 23, 40] {
            for content in [viewport, viewport + 1, viewport * 3 / 2, viewport * 5] {
                let maximum = content.saturating_sub(viewport);
                for start in [0usize, maximum / 3, maximum / 2, maximum] {
                    for command in ScrollCommand::all() {
                        let mut driver = ScrollDriver::new();
                        driver.scrolled_to(start.min(maximum), maximum);
                        driver.scroll(command, viewport, content);
                        let current = f64::from(u32::try_from(start.min(maximum)).unwrap_or(0));
                        let delta = match command {
                            ScrollCommand::PageUp => -(f64::from(viewport as u32) / 2.0),
                            ScrollCommand::PageDown => f64::from(viewport as u32) / 2.0,
                            ScrollCommand::HalfPageUp => -(f64::from(viewport as u32) / 4.0),
                            ScrollCommand::HalfPageDown => f64::from(viewport as u32) / 4.0,
                            ScrollCommand::LineUp => -1.0,
                            ScrollCommand::LineDown => 1.0,
                            // The jumps are sticky-state transitions,
                            // not setter math.
                            ScrollCommand::First | ScrollCommand::Last => continue,
                        };
                        // The ported setter: round(clamp(v, 0, max)).
                        let ported = opentui_core::renderables::scrollbar::clamp_scroll_position(
                            ((current + delta) as f32).max(0.0),
                            content as f32,
                            viewport as f32,
                        ) as usize;
                        let landed = match driver.follow() {
                            FollowState::Sticky => maximum,
                            FollowState::Free { offset, .. } => offset,
                        };
                        assert_eq!(
                            landed, ported,
                            "viewport {viewport} content {content} start {start} {command:?}"
                        );
                    }
                }
            }
        }
    }
}

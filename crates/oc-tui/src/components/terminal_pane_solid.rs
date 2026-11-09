//! Lane D4.1b — the embedded terminal pane's shared-tree mount.
//!
//! The pane used to materialize a paint [`Frame`]
//! (`render_terminal_pane`) that `render_session` pasted into the
//! session frame (the Frame detour). The VT pixels now ride the
//! shared tree like every other family: this module composes the
//! opentui engine into an `OptimizedBuffer`
//! (`crate::terminal_pane::pane_cells` — the SAME cell walk the
//! retired Frame conversion ran), shapes the rows into styled text
//! runs, and syncs the `surface-terminal` family wrapper into the
//! shared-tree hub (`chrome_solid::mount_family_wrapper`). No Frame
//! comes back; the pixels land at `render_app`'s ONE composite seam
//! (`composite_chrome_tree`), which walks the shared tree buffer
//! over the pane's rect with the seam's cell semantics.
//!
//! The family seats at the BASE layer (skeleton index 0, the
//! tabs-strip/home seat): the old paste ran inside `render_session`
//! — BEFORE the composite seam — so the pane sat under every tree
//! family; the base seat keeps that exact z (bytes arbitrate — the
//! sidebar never co-mounts with the pane, `rightPane()` precedence).
//!
//! The headless twin (`crate::terminal_pane::render_terminal_pane`)
//! keeps building a standalone Frame from the SAME cell walk for the
//! unit harnesses (golden parity by construction).

// Terminal dimensions are bounded usize→f32 casts (taffy points),
// the same allowance the other solid surfaces carry.
#![allow(clippy::cast_possible_truncation)]
#![allow(clippy::cast_precision_loss)]
#![allow(clippy::cast_sign_loss)]

use std::cell::RefCell;
use std::rc::Rc;

use opentui_core::Rgba;
use opentui_core::styled_text::{StyledText, TextChunk};
use opentui_solid::builder::ForItemSource;
use opentui_solid::element::{ForItem, PropValue};
use opentui_solid::{Element, Signal, box_, for_list, map, text};

use crate::components::chrome_solid::{family_wrapper_live, mount_family_wrapper};
use crate::render::CellColor;
use crate::terminal_pane::{self, TerminalPaneState};

/// The terminal family's skeleton index under the shared tree's root
/// (`surface-terminal`): 0, the BASE seat — the old Frame paste ran
/// inside `render_session`, under every tree family, so the pane
/// seats below the chrome container exactly like the tabs strip and
/// the home route base.
pub(crate) const TERMINAL_SKELETON_INDEX: usize = 0;

/// One shaped row: styled text runs over the row's cells (the
/// [`PaneCell`] walk's run-length form — equal-style neighbors merge,
/// the twin's byte output is per-cell so the merge is loss-free).
#[derive(Clone, PartialEq, Eq, Debug, Default)]
struct TermRow {
    chunks: Vec<(String, CellColor, CellColor, u16)>,
}

/// The family's reactive content: one keyed entry per grid row.
#[derive(Clone, Default)]
struct TermInputs {
    rows: Vec<TermRow>,
}

/// The mounted family's state, kept across frames so content churn
/// patches the rows' dyn-props in place; a geometry-key change (the
/// pane rect) rebuilds the wrapper's content subtree.
struct TerminalEntry {
    signal: Signal<TermInputs>,
    key: (usize, usize),
}

thread_local! {
    /// The terminal family's content state (one mount per
    /// thread-process — the wrapper lives in the shared-tree hub).
    static TERMINAL_TREE: RefCell<Option<TerminalEntry>> =
        const { RefCell::new(None) };
}

/// `CellColor` → the styled-text color channel (the chrome seam's
/// `chunk_rgba` rule: `Default` has no opaque channel — the cell
/// falls to the transparent slot and the seam's hole rule keeps the
/// frame's own pixel, exactly the twin's `CellColor::Default`).
fn chunk_color(color: CellColor) -> Option<Rgba> {
    match color {
        CellColor::Rgb(red, green, blue) => Some(Rgba::rgb(red, green, blue)),
        CellColor::Default | CellColor::Indexed(_) => None,
    }
}

fn row_styled(row: &TermRow) -> StyledText {
    StyledText::new(
        row.chunks
            .iter()
            .map(|(text, fg, bg, attrs)| TextChunk {
                text: text.clone(),
                fg: chunk_color(*fg),
                bg: chunk_color(*bg),
                attributes: u32::from(*attrs),
                link: None,
            })
            .collect::<Vec<TextChunk>>(),
    )
}

fn row_element(inputs: &Signal<TermInputs>, index: usize) -> Element {
    let inputs = inputs.clone();
    text()
        .position("absolute")
        .left(0.0)
        .top(index as f32)
        .wrap_mode("none")
        .flex_shrink(0.0)
        .dyn_prop(
            "content",
            opentui_solid::dyn_(
                map(inputs, move |inputs: &TermInputs| {
                    inputs
                        .rows
                        .get(index)
                        .map_or_else(|| StyledText::new(Vec::new()), |row| row_styled(row))
                }),
                |value: &StyledText| PropValue::Styled(value.clone()),
            ),
        )
        .into_element()
}

/// The wrapper's content: one flat keyed `For` of the pane's absolute
/// row texts (surface-local coordinates — the wrapper is their
/// containing block).
fn terminal_content(inputs: &Signal<TermInputs>, width: usize, height: usize) -> Element {
    let source = ForItemSource {
        source: map(inputs.clone(), |inputs: &TermInputs| {
            inputs
                .rows
                .iter()
                .enumerate()
                .map(|(y, _)| ForItem {
                    key: format!("y{y}"),
                    value: PropValue::Str(format!("y{y}")),
                })
                .collect::<Vec<ForItem>>()
        }),
    };
    let item_inputs = inputs.clone();
    let rows = for_list(Rc::new(source), move |item: &ForItem, _index: usize| {
        let index: usize = item.key.strip_prefix('y').and_then(|s| s.parse().ok()).unwrap_or(0);
        row_element(&item_inputs, index)
    });
    box_()
        .width(width as f32)
        .height(height as f32)
        .position("absolute")
        .left(0.0)
        .top(0.0)
        .children_dyn(Rc::new(rows))
        .into_element()
}

/// Sync the terminal family into the shared tree (the family's ONLY
/// live mount entry — no Frame comes back; pixels reach the emitted
/// frame through `composite_chrome_tree`, the single compositor seam
/// inside `render_app`). `left`/`top` are the OLD pane paste site (the
/// imperative source); the engine composes through
/// [`terminal_pane::pane_cells`] — the retired Frame conversion's
/// exact cell walk — and the rows shape into styled text runs.
pub fn terminal_pane_mount(
    state: &TerminalPaneState,
    width: usize,
    height: usize,
    left: usize,
    top: usize,
) {
    if width == 0 || height == 0 {
        // The degenerate shape: nothing mounts — the stale wrapper
        // unmounts at the seam.
        return;
    }
    let rows: Vec<TermRow> = terminal_pane::pane_cells(state, width, height)
        .into_iter()
        .map(|cells| TermRow { chunks: shape_row(cells) })
        .collect();
    let key = (width, height);
    // Rebuild iff the geometry key changed OR the wrapper left the
    // tree (a hub reset between harness frames, a seam prune).
    let rebuild = !family_wrapper_live("terminal")
        || TERMINAL_TREE.with_borrow(|slot| !matches!(slot, Some(entry) if entry.key == key));
    let (content, content_key) = if rebuild {
        let signal = Signal::new(TermInputs { rows: rows.clone() });
        let element = terminal_content(&signal, width, height);
        TERMINAL_TREE.with_borrow_mut(|slot| {
            *slot = Some(TerminalEntry { signal, key });
        });
        (Some(element), Some(format!("terminal-content@{width}x{height}")))
    } else {
        TERMINAL_TREE.with_borrow(|slot| {
            slot.as_ref().expect("checked above").signal.set(TermInputs { rows });
        });
        (None, None)
    };
    mount_family_wrapper(
        "terminal",
        TERMINAL_SKELETON_INDEX,
        left,
        top,
        width,
        height,
        content_key,
        content,
        &[],
        &[],
        false,
    );
}

/// Run-length the twin's per-cell row into styled chunks: equal
/// (fg, bg, attrs) neighbors merge (the text raster lays each char at
/// its own cell, so the merge is loss-free).
fn shape_row(cells: Vec<terminal_pane::PaneCell>) -> Vec<(String, CellColor, CellColor, u16)> {
    let mut chunks: Vec<(String, CellColor, CellColor, u16)> = Vec::new();
    for cell in cells {
        match chunks.last_mut() {
            Some((text, fg, bg, attrs)) if *fg == cell.fg && *bg == cell.bg && *attrs == cell.attrs => {
                text.push(cell.ch);
            }
            _ => chunks.push((cell.ch.to_string(), cell.fg, cell.bg, cell.attrs)),
        }
    }
    chunks
}

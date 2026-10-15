//! Lane M6: the exit farewell (`sessionEpilogue`,
//! util/presentation.ts:26-34) on the opentui-solid builder tree —
//! the wordmark glyph rows + the weak labels + the resume hint
//! (see `docs/migration-m1-tabstrip.md`, the M6 section).
//!
//! **What this module is.** v2 writes the epilogue to STDOUT after
//! the renderer stops (app.tsx:443-450 — the `EpilogueProvider`
//! value the session route stages while mounted). The byte stream is
//! the rig's compared artifact (SGR included), so the solid path
//! must reproduce [`crate::presentation::session_epilogue`]
//! byte-for-byte. The port: the epilogue's grid (4 wordmark art
//! rows, a blank, the `Session`/`Continue` label rows, a blank)
//! mounts as a STATIC builder tree — absolute text rows carrying
//! chunk-merged runs at the exact presentation palette — renders
//! through a real [`SolidRenderer`], and the rasterized buffer is
//! SERIALIZED back to ANSI with presentation.ts's emission rules:
//!
//! * wordmark rows (0-3) emit PER GLYPH (`draw`, presentation.ts:
//!   8-17): `_` → the shadow-bg space, `^` → fg+bg `▀`, `~` → the
//!   shadow-fg `▀`, plain spaces stay bare, glyphs carry their
//!   half's fg (the LEFT half `dim` = SGR 90, the RIGHT half the
//!   literal reset-as-fg `\x1b[0m` prefix — presentation.ts's own
//!   quirk, reproduced from the cell's default-fg class);
//! * label rows (5-6) emit PER RUN (`weak()` + the bold values):
//!   `\x1b[90m{label:<10}\x1b[0m\x1b[1m{value}\x1b[0m`.
//!
//! **Selection flag.** `OC_SUBAGENT_OPENTUI` (shared with the
//! picker panel — one flag per lane surface family; default ON after
//! the proof): `=0` selects [`crate::presentation`]'s string builder
//! (the byte-exact E2 port, one release); every other value —
//! including unset — renders through this module's tree. The unit
//! gate pins BOTH the emitted bytes and the rasterized grid against
//! the string builder's parse.

// Terminal dimensions/colors are bounded usize→f32 casts (taffy
// points), the same allowance the other solid surfaces carry.
#![allow(clippy::cast_possible_truncation)]
#![allow(clippy::cast_precision_loss)]
#![allow(clippy::match_same_arms)] // the cell-class table reads better explicit

use opentui_core::renderer::CliRendererConfig;
use opentui_core::styled_text::{StyledText, TextChunk};
use opentui_core::Rgba;
use opentui_solid::element::PropValue;
use opentui_solid::{box_, text, Element, SolidRenderer};

/// The presentation palette (presentation.ts:3-24) — the EXACT
/// xterm-256 RGB values the SGR indices resolve to, so the raster →
/// ANSI walk round-trips closed: SGR 90 (bright black) is xterm's
/// `#767676`, index 235 `#262626`, index 238 `#444444`.
fn dim_fg() -> Rgba {
    Rgba::rgb(0x76, 0x76, 0x76)
}
/// `38;5;235` / `48;5;235` (the LEFT half's shadow).
fn left_shadow() -> Rgba {
    Rgba::rgb(0x26, 0x26, 0x26)
}
/// `38;5;238` / `48;5;238` (the RIGHT half's shadow).
fn right_shadow() -> Rgba {
    Rgba::rgb(0x44, 0x44, 0x44)
}

/// `logo.left` / `logo.right` (logo.ts:1-4) — presentation.rs's own
/// tables (the ONE source for the art; this module never forks it).
use crate::presentation::{LOGO_LEFT, LOGO_RIGHT};

/// The epilogue's fixed shape: 4 art rows, blank, 2 label rows,
/// blank (8 lines; the app's write appends the final newline).
const EPILOGUE_ROWS: usize = 8;
/// One art row's painted width: 2 pad + 19 left + 1 gap + 19 right.
const WORDMARK_WIDTH: usize = 41;

// ---------------------------------------------------------------------------
// the cell model (draw()'s classes, the tree's own currency)
// ---------------------------------------------------------------------------

/// One wordmark cell's style class — the exact `draw()` arms.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CellClass {
    /// `' '` inside an art line — bare, no escapes.
    PlainSpace,
    /// `_` → a space over the shadow bg.
    ShadowSpace { right: bool },
    /// `^` → `▀` with the half's fg over the shadow bg.
    UpperOver { right: bool },
    /// `~` → `▀` IN the shadow (shadow fg, no bg).
    UpperIn { right: bool },
    /// A block glyph (`█`/`▄`/…) with the half's fg.
    Glyph { right: bool },
}

impl CellClass {
    /// The cell's fg (None = the default/reserved class — the RIGHT
    /// half's literal `\x1b[0m` prefix, or a plain space).
    fn fg_color(self) -> Option<Rgba> {
        match self {
            Self::PlainSpace | Self::ShadowSpace { .. } => None,
            Self::UpperOver { right: false } | Self::Glyph { right: false } => Some(dim_fg()),
            Self::UpperOver { right: true } | Self::Glyph { right: true } => None,
            Self::UpperIn { right: false } => Some(left_shadow()),
            Self::UpperIn { right: true } => Some(right_shadow()),
        }
    }

    /// The cell's bg (None = default).
    fn bg_color(self) -> Option<Rgba> {
        match self {
            Self::ShadowSpace { right: false } | Self::UpperOver { right: false } => {
                Some(left_shadow())
            }
            Self::ShadowSpace { right: true } | Self::UpperOver { right: true } => {
                Some(right_shadow())
            }
            Self::PlainSpace | Self::UpperIn { .. } | Self::Glyph { .. } => None,
        }
    }
}

/// One art row's cells — `draw(line, fg, shadow, bg)` per glyph
/// (presentation.ts:8-17).
fn art_cells(line: &str, right: bool) -> Vec<(char, CellClass)> {
    line.chars()
        .map(|ch| {
            let class = match ch {
                '_' => CellClass::ShadowSpace { right },
                '^' => CellClass::UpperOver { right },
                '~' => CellClass::UpperIn { right },
                ' ' => CellClass::PlainSpace,
                _ => CellClass::Glyph { right },
            };
            let glyph = match ch {
                '^' | '~' => '▀',
                '_' => ' ',
                other => other,
            };
            (glyph, class)
        })
        .collect()
}

/// One row's cells as chunk-MERGED runs (the tree's text content —
/// adjacent same-style cells merge, exactly like
/// `home_solid::logo_row_chunks`).
fn cells_chunks(cells: &[(char, CellClass)], bold: bool) -> StyledText {
    let attributes = u32::from(bold);
    let mut chunks: Vec<TextChunk> = Vec::new();
    let mut run: Option<(String, CellClass)> = None;
    for (glyph, class) in cells {
        if matches!(run, Some((_, class0)) if class0 == *class) {
            if let Some((text, _)) = &mut run {
                text.push(*glyph);
            }
        } else {
            if let Some((text, class)) = run.take() {
                chunks.push(chunk(text, class, attributes));
            }
            run = Some((glyph.to_string(), *class));
        }
    }
    if let Some((text, class)) = run {
        chunks.push(chunk(text, class, attributes));
    }
    StyledText::new(chunks)
}

fn chunk(text: String, class: CellClass, attributes: u32) -> TextChunk {
    TextChunk { text, fg: class.fg_color(), bg: class.bg_color(), attributes, link: None }
}

// ---------------------------------------------------------------------------
// the tree (static rows over the cell model)
// ---------------------------------------------------------------------------

/// The epilogue's grid width: the wordmark's 41 or the widest label
/// row (the values never wrap — `wrapMode none`).
fn epilogue_width(title: &str, session_id: &str) -> usize {
    let title = crate::presentation::truncate_utf16(title, 50);
    let label_rows =
        [12 + title.chars().count(), 12 + "opencode -s ".len() + session_id.chars().count()];
    label_rows.into_iter().fold(WORDMARK_WIDTH, usize::max)
}

/// The farewell tree: one absolute text row per epilogue line over a
/// transparent root (the serializer owns the byte stream — the tree
/// proves the GRID).
fn farewell_element(title: &str, session_id: &str) -> Element {
    let title_cell = crate::presentation::truncate_utf16(title, 50);
    let mut children: Vec<Element> = Vec::with_capacity(EPILOGUE_ROWS);
    // Rows 0-3 — the wordmark: one text element per half (the pad
    // cols 0-1 and the gap col 21 stay OUTSIDE both, like the home
    // wordmark's gap cell).
    for row in 0..4 {
        for (right, line, x) in [(false, LOGO_LEFT[row], 2usize), (true, LOGO_RIGHT[row], 22)] {
            let cells = art_cells(line, right);
            children.push(
                text()
                    .position("absolute")
                    .left(x as f32)
                    .top(row as f32)
                    .wrap_mode("none")
                    .flex_shrink(0.0)
                    .content(PropValue::Styled(cells_chunks(&cells, false)))
                    .into_element(),
            );
        }
    }
    // Rows 5-6 — the weak label + the bold value (two chunks, one
    // text element each so the classes never merge).
    for (row, label, value) in
        [(5usize, "Session", title_cell), (6, "Continue", format!("opencode -s {session_id}"))]
    {
        let mut chunks = vec![TextChunk {
            text: format!("{label:<10}"),
            fg: Some(dim_fg()),
            bg: None,
            attributes: 0,
            link: None,
        }];
        chunks.push(TextChunk {
            text: value,
            fg: None,
            bg: None,
            attributes: 1, // BOLD
            link: None,
        });
        children.push(
            text()
                .position("absolute")
                .left(2.0)
                .top(row as f32)
                .wrap_mode("none")
                .flex_shrink(0.0)
                .content(PropValue::Styled(StyledText::new(chunks)))
                .into_element(),
        );
    }
    box_()
        .width(epilogue_width(title, session_id).max(1) as f32)
        .height(EPILOGUE_ROWS as f32)
        .position("relative")
        .children(opentui_solid::element::Children::Static(children))
        .into_element()
}

// ---------------------------------------------------------------------------
// the raster → ANSI serializer (presentation.ts's emission rules)
// ---------------------------------------------------------------------------

/// The fg SGR prefix for a raster cell color (None = default).
fn fg_prefix(color: Rgba) -> Option<&'static str> {
    if color == dim_fg() {
        Some("\x1b[90m")
    } else if color == left_shadow() {
        Some("\x1b[38;5;235m")
    } else if color == right_shadow() {
        Some("\x1b[38;5;238m")
    } else {
        None
    }
}

/// The bg SGR prefix for a raster cell color (None = default).
fn bg_prefix(color: Rgba) -> Option<&'static str> {
    if color == left_shadow() {
        Some("\x1b[48;5;235m")
    } else if color == right_shadow() {
        Some("\x1b[48;5;238m")
    } else {
        None
    }
}

/// Serialize the rasterized tree back to presentation.ts's byte
/// stream. Wordmark rows (0-3) emit per glyph (`draw`); the label
/// rows (5-6) merge same-style runs (`weak()` + the bold values);
/// the blank rows (4, 7) emit empty lines. A default-fg glyph on an
/// art row carries the RIGHT half's literal reset-as-fg prefix.
#[must_use]
pub fn serialize_farewell(buffer: &opentui_core::buffer::OptimizedBuffer) -> String {
    let reset = "\x1b[0m";
    let mut lines: Vec<String> = Vec::with_capacity(EPILOGUE_ROWS);
    for y in 0..EPILOGUE_ROWS {
        if y == 4 || y == 7 {
            lines.push(String::new());
            continue;
        }
        let mut cells: Vec<(char, Option<&'static str>, Option<&'static str>, bool)> = Vec::new();
        for x in 0..buffer.width {
            let Some(cell) = buffer.get(x, y as u32) else { break };
            let ch = if opentui_core::buffer::is_continuation_char(cell.char) {
                continue; // a wide glyph's continuation cell — taken
            } else if opentui_core::buffer::is_grapheme_char(cell.char) {
                buffer
                    .grapheme_bytes(opentui_core::buffer::grapheme_id_from_char(cell.char))
                    .chars()
                    .next()
                    .unwrap_or(' ')
            } else {
                char::from_u32(cell.char).unwrap_or(' ')
            };
            let bold = cell.attributes & 0x1 != 0;
            cells.push((ch, fg_prefix(cell.fg), bg_prefix(cell.bg), bold));
        }
        // Label rows drop the buffer's trailing plain padding
        // (presentation.ts lines end at their content); wordmark rows
        // keep every cell of their fixed-width art (trailing spaces
        // are content) — the walk below caps them at the art width.
        let wordmark_row = y < 4;
        if !wordmark_row {
            while cells.last().is_some_and(|(ch, fg, bg, bold)| {
                *ch == ' ' && fg.is_none() && bg.is_none() && !*bold
            }) {
                cells.pop();
            }
        }
        let mut line = String::new();
        if wordmark_row {
            // The art is exactly WORDMARK_WIDTH wide — trailing
            // spaces INSIDE it are content (draw(' ') → ' '); the
            // buffer's padding beyond it never emits.
            let cells: Vec<_> = cells.into_iter().take(WORDMARK_WIDTH).collect();
            for (ch, fg, bg, _) in cells {
                match (ch == ' ', fg, bg) {
                    (true, None, None) => line.push(' '),
                    (true, None, Some(bg)) => {
                        line.push_str(bg);
                        line.push(' ');
                        line.push_str(reset);
                    }
                    (_, fg, bg) => {
                        // A glyph cell: the RIGHT half's default-fg is
                        // presentation.ts's literal `${reset}` prefix.
                        let fg_seq = fg.unwrap_or(reset);
                        line.push_str(fg_seq);
                        if let Some(bg) = bg {
                            line.push_str(bg);
                        }
                        line.push(ch);
                        line.push_str(reset);
                    }
                }
            }
        } else {
            // The label rows: same-style cells merge into one run
            // (`weak()` + the bold values). A run is EITHER the dim
            // label (fg 90, not bold) or the bold value (no fg); an
            // UNSTYLED run (the leading pad) emits bare text, no
            // escapes — presentation.ts wraps only styled runs.
            let mut run: Option<(String, Option<&'static str>, bool)> = None;
            let mut saw_bold_run = false;
            let flush =
                |line: &mut String, run: &mut Option<(String, Option<&'static str>, bool)>| {
                    if let Some((text, seq, bold)) = run.take() {
                        match (seq, bold) {
                            (Some(seq), false) => {
                                line.push_str(seq);
                                line.push_str(&text);
                                line.push_str(reset);
                            }
                            (None, true) => {
                                line.push_str("\x1b[1m");
                                line.push_str(&text);
                                line.push_str(reset);
                            }
                            (None, false) => line.push_str(&text),
                            (Some(_), true) => unreachable!("the weak label is never bold"),
                        }
                    }
                };
            for (ch, fg, _bg, bold) in cells {
                let seq = if bold { None } else { fg };
                saw_bold_run |= bold;
                let continues =
                    matches!(&run, Some((_, seq0, bold0)) if *seq0 == seq && *bold0 == bold);
                if continues {
                    if let Some((text, _, _)) = &mut run {
                        text.push(ch);
                    }
                } else {
                    flush(&mut line, &mut run);
                    run = Some((ch.to_string(), seq, bold));
                }
            }
            flush(&mut line, &mut run);
            if !saw_bold_run {
                // presentation.ts always emits the bold value run —
                // `${BOLD}${value}${RESET}` even when the value is
                // the empty string (the empty-title row).
                line.push_str("\x1b[1m");
                line.push_str(reset);
            }
        }
        lines.push(line);
    }
    lines.join("\n")
}

// ---------------------------------------------------------------------------
// the dispatch seam
// ---------------------------------------------------------------------------

/// Render the farewell through the builder tree and serialize the
/// raster back to presentation.ts's bytes — the solid-path twin of
/// [`crate::presentation::session_epilogue`].
#[must_use]
pub fn solid_session_epilogue(title: &str, session_id: &str) -> String {
    let width =
        u32::try_from(epilogue_width(title, session_id).max(1)).unwrap_or(u32::from(u16::MAX));
    let renderer = SolidRenderer::new(CliRendererConfig {
        width,
        height: EPILOGUE_ROWS as u32,
        background_color: Rgba::rgb(0, 0, 0),
        ..CliRendererConfig::default()
    });
    let mut renderer = renderer;
    renderer.render_root(farewell_element(title, session_id));
    renderer.flush();
    serialize_farewell(&renderer.renderer.current_render_buffer)
}

/// The epilogue's ONLY renderer (lane M6, retired): the
/// opentui-solid tree, raster → ANSI bytes — presentation.ts's byte
/// stream reproduced by the tree (the string builder was deleted
/// with the flag; the rig's subagent exit steps pin the live bytes).
#[must_use]
pub fn session_epilogue_dispatch(title: &str, session_id: &str) -> String {
    solid_session_epilogue(title, session_id)
}

#[cfg(test)]
mod tests {}

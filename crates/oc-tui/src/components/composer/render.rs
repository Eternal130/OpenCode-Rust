//! The composer's visible surface — byte-exact against the
//! `composer-*` goldens (text grid + canonical ANSI at the manifest
//! depth). Structure derived from `component/prompt/index.tsx`'s mount:
//!
//! ```text
//! ┃  ← left border (`border.base`) over the page background
//! ┃  Ask anything… "Fix a TODO in the codebase"   ← promptBg fill
//! ┃                                              (decrease(raised.base))
//! ╹▀▀▀…  ← bottom-left corner + upper-half blocks in promptBg
//! /tmp/opencode/packages/tui:main   [ agents  commands]
//! ```
//!
//! * padding: left/right 2 (1 below 44 cols), top 1 — the textarea
//!   sits at `top + 1`, the metadata pad row (its labels are absent in
//!   every pinned state) closes the box;
//! * the textarea's visible rows = `min(maxHeight, wrapped + 1)` with
//!   `maxHeight = max(6, height/3)`;
//! * the placeholder truncates to `inner − 2` (the caret column
//!   reserve) while typed text wraps with word semantics that KEEP the
//!   break space on the previous line — CJK clusters break anywhere,
//!   extmark chips (`[Image 1]`, bold `feedback.warning.base`) are
//!   atomic;
//! * the bottom border paints `▀` in `promptBg` over the page bg;
//! * the footer row: the `{directory}:{branch}` label muted at the
//!   left, the plugin hints (` agents` + 2 gap + ` commands`, keys in
//!   `text.base`, labels muted) right-aligned from col `width − 1`,
//!   gated at ≥ 44 cols;
//! * the autocomplete popup overlays the rows above the box: `┃`
//!   borders, the selected row filled `action.primary.focused` with
//!   `text.action.primary.focused` glyphs, the title column padded to
//!   the registry's longest display + 2, description prefixed by one
//!   space.
//!
//! Caret policy (lane contract): caret/selection internals are C2 —
//! the caret renders as v2's visible resting glyph, which in a golden
//! frame is nothing.

use crate::components::composer::autocomplete::{popup_height, AutocompleteOption};
use crate::components::composer::PromptDraft;
use crate::render::{Cell, CellColor, Frame};
use crate::theme::ThemeState;
use crate::wrap::display_width;
use oc_theme::Rgba;

/// The autocomplete popup's render inputs.
#[derive(Debug, Clone, Default)]
pub struct AutocompleteView<'a> {
    pub options: &'a [AutocompleteOption],
    pub selected: usize,
    /// The padded title-column width (`registry.display_width()`).
    pub title_width: usize,
}

/// The footer row's inputs: the location label, the two hint prefixes
/// ("" in the golden harness — unbound shortcuts render as nothing,
/// leaving ` agents` / ` commands`), the optional agent · model
/// statusline (v2's `footerStatuslinePolicy` hint row — empty in every
/// pinned composer golden, populated on the live routes), and the
/// wave-3 plugin hint segments (rendered left of the shortcut hints,
/// muted — empty by default so every golden stays byte-exact).
#[derive(Debug, Clone, Default)]
pub struct FooterHints {
    pub location: String,
    pub agent_shortcut: String,
    pub command_shortcut: String,
    /// The context-usage segment (`18 (0%)`, [`formatContextUsage`]):
    /// when present the right block shows usage + the COMMAND hint
    /// only — the agents hint drops (`PromptFooter`'s first `Match`,
    /// footer.tsx:65-94). Empty = the idle shape.
    pub usage: String,
    /// Plugin-contributed hint segments (wave 3; ` · `-joined).
    pub plugin_hints: Vec<String>,
    /// Wave A: the interrupt hint (`PromptInterruptStatus`,
    /// prompt/index.tsx:137-144) — `esc interrupt` while a turn runs,
    /// `esc again to interrupt` once armed; EMPTY renders nothing
    /// (every golden stays byte-exact while no turn is running).
    pub interrupt: String,
    /// #33 — the location label's hover lift
    /// (`useWorkingDirectoryActions.hovered`: `text.base` under the
    /// pointer, `text.muted` at rest, prompt/index.tsx:1927).
    pub location_hovered: bool,
}

/// The composer's metadata row (`PromptMetadataRow`,
/// `component/prompt/metadata.tsx`): `Build · Fake Model parityfake`
/// inside the prompt box under the textarea — agent in the rail's
/// agent color, `auto`/`·`/provider muted, model in `text.base`,
/// variant in `text.feedback.warning` bold.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MetadataLine {
    /// The staged agent label ("" = none — the row renders blank).
    pub agent: String,
    /// `local.permission.mode === "autoaccept"`.
    pub auto: bool,
    /// The model's display name.
    pub model: String,
    /// The provider's display name.
    pub provider: String,
    /// The selected variant id ("" = none).
    pub variant: String,
}

/// One resolved metadata layout (the surviving fields).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MetadataLayout {
    pub agent: String,
    pub auto: bool,
    pub model: String,
    pub provider: String,
    pub variant: String,
}

impl MetadataLayout {
    /// The joined single-line text (`text()`, metadata.tsx:138-146).
    #[must_use]
    pub fn text(&self) -> String {
        let mut parts: Vec<String> = Vec::new();
        if !self.agent.is_empty() {
            parts.push(self.agent.clone());
        }
        if self.auto {
            parts.push("auto".to_owned());
        }
        if !self.model.is_empty() {
            if !self.agent.is_empty() {
                parts.push("·".to_owned());
            }
            parts.push(self.model.clone());
        }
        if !self.provider.is_empty() {
            parts.push(self.provider.clone());
        }
        if !self.variant.is_empty() {
            parts.push("·".to_owned());
            parts.push(self.variant.clone());
        }
        parts.join(" ")
    }
}

impl MetadataLine {
    /// `promptMetadataLayout` (metadata.tsx:101-136): ordered drop
    /// candidates, then a width-truncated model. `terminal_width` is
    /// the TERMINAL width (the budget `width - (width < 44 ? 9 : 13)`
    /// reads it off `dimensions()`), not the prompt box's.
    #[must_use]
    pub fn layout(&self, terminal_width: usize) -> MetadataLayout {
        let budget = terminal_width.saturating_sub(if terminal_width < 44 { 9 } else { 13 });
        let narrow = terminal_width < 44;
        let agent = if narrow { String::new() } else { self.agent.clone() };
        let provider = if narrow { String::new() } else { self.provider.clone() };
        let candidates = [
            MetadataLayout {
                agent: agent.clone(),
                auto: self.auto,
                model: self.model.clone(),
                provider: provider.clone(),
                variant: self.variant.clone(),
            },
            MetadataLayout {
                agent: agent.clone(),
                auto: false,
                model: self.model.clone(),
                provider: provider.clone(),
                variant: self.variant.clone(),
            },
            MetadataLayout {
                agent: agent.clone(),
                auto: false,
                model: self.model.clone(),
                provider: provider.rsplit(" / ").next().unwrap_or_default().to_owned(),
                variant: self.variant.clone(),
            },
            MetadataLayout {
                agent: agent.clone(),
                auto: false,
                model: self.model.clone(),
                provider: String::new(),
                variant: self.variant.clone(),
            },
        ];
        for candidate in &candidates {
            if display_width(&candidate.text()) <= budget {
                return candidate.clone();
            }
        }
        // The truncate fallback: model clipped to what remains after
        // the agent prefix and variant suffix (min 9).
        let prefix = if agent.is_empty() { String::new() } else { format!("{agent} · ") };
        let suffix =
            if self.variant.is_empty() { String::new() } else { format!(" · {}", self.variant) };
        let model_budget =
            budget.saturating_sub(display_width(&prefix) + display_width(&suffix)).max(9);
        let model = truncate_ellipsis(&self.model, model_budget);
        MetadataLayout {
            agent,
            auto: false,
            model,
            provider: String::new(),
            variant: self.variant.clone(),
        }
    }
}

/// `Locale.truncateWidth` (util/locale.ts:76-81).
fn truncate_ellipsis(value: &str, width: usize) -> String {
    if width == 0 {
        return String::new();
    }
    if display_width(value) <= width {
        return value.to_owned();
    }
    if width == 1 {
        return "…".to_owned();
    }
    let kept = crate::wrap::take_width(value, width - 1);
    format!("{kept}…")
}

/// Everything [`render_prompt_frame`] paints.
#[derive(Debug, Clone)]
pub struct PromptView<'a> {
    pub draft: &'a PromptDraft,
    /// The placeholder list entry (`Ask anything… "{entry}"`).
    pub placeholder: String,
    pub autocomplete: Option<AutocompleteView<'a>>,
    pub footer: FooterHints,
    /// The composer's metadata row (#5, `PromptMetadataRow`): the
    /// staged `agent · model provider` line inside the prompt box.
    pub metadata: MetadataLine,
    /// The TERMINAL width (the metadata row's drop budget reads
    /// `dimensions().width`, not the prompt box's).
    pub terminal_width: usize,
    /// The mount's y (the golden harness pads 2 rows above the box).
    pub top: usize,
    /// The composer rail's agent color (`borderHighlight()` at fade
    /// alpha 1): v2 paints the left border + `╹` corner in the ACTIVE
    /// AGENT's categorical color whenever an agent resolved — the
    /// golden corpus (no session/agent in the state fixture) captures
    /// the no-agent fallback `border.base`, so `None` keeps that byte
    /// shape and the live routes pass the resolved color.
    pub rail: Option<CellColor>,
    /// The caret as a blinking block cursor (`input.cursorColor =
    /// theme.text.base`): `Some(char offset)` paints it at its visual
    /// cell; `None` paints nothing (the golden frames pin the resting
    /// glyph-less state — the caller gates this on the blink phase).
    pub caret: Option<usize>,
}

/// The composer cursor's blink period (`input.cursor.blinking`'s
/// toggle): 500 ms visible / 500 ms hidden, driven by the store clock
/// (the run loop's 100 ms tick re-renders, so no extra wake source).
pub const CURSOR_BLINK_INTERVAL_MS: u64 = 500;

/// Whether the cursor is in its visible phase at `clock_ms`
/// (deterministic: golden harnesses inject the clock).
#[must_use]
pub fn cursor_blink_on(clock_ms: u64) -> bool {
    (clock_ms / CURSOR_BLINK_INTERVAL_MS).is_multiple_of(2)
}

/// The caret's frame-relative screen cell (NEW-S1a): the TERMINAL
/// cursor's position — `(1 + pad + col, top + 1 + row)` under the
/// SAME wrap model the text paints (`caret_cell`). `None` when the
/// caret row sits outside the textarea's visible rows.
#[must_use]
pub fn prompt_caret_screen_cell(
    draft: &PromptDraft,
    width: usize,
    top: usize,
    height: usize,
    caret: usize,
) -> Option<(usize, usize)> {
    let pad = if width >= 44 { 2 } else { 1 };
    let inner = width.saturating_sub(1 + pad * 2).max(1);
    let lines = layout_draft_lines(draft, inner);
    let max_height = (height / 3).max(6);
    let textarea_rows = (lines.len().max(1) + 1).min(max_height);
    let (row, col) = caret_cell(draft, inner, caret);
    if row >= textarea_rows {
        return None;
    }
    Some((1 + pad + col, top + 1 + row))
}

/// One styled fragment of a laid-out line (`chip` marks a file
/// mention rendered as an extmark).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Span {
    pub(crate) text: String,
    pub(crate) chip: bool,
}

/// The textarea's wrap model. Units: spaces (kept on the line they fit
/// on, consumed at a break that lands on them), words (moved whole),
/// wide CJK clusters (break anywhere), chips (atomic).
enum UnitKind {
    Space,
    Word,
    BreakAnywhere,
}

impl UnitKind {
    fn is_space(&self) -> bool {
        matches!(self, Self::Space)
    }
}

struct Unit {
    text: String,
    width: usize,
    chip: bool,
    kind: UnitKind,
}

fn unit_kind(ch: char) -> UnitKind {
    if ch == ' ' {
        UnitKind::Space
    } else if display_width(&ch.to_string()) > 1 {
        UnitKind::BreakAnywhere
    } else {
        UnitKind::Word
    }
}

/// Split the draft into wrap units, honoring file-mention chips as
/// atomic spans (their chars join the unit stream but never split).
fn draft_units(draft: &PromptDraft) -> Vec<Unit> {
    let chars: Vec<char> = draft.text.chars().collect();
    let chip_at =
        |index: usize| draft.files.iter().any(|file| index >= file.start && index < file.end);
    let mut units: Vec<Unit> = Vec::new();
    let mut index = 0usize;
    while index < chars.len() {
        let ch = chars[index];
        if ch == '\n' {
            units.push(Unit {
                text: "\n".into(),
                width: 0,
                chip: false,
                kind: UnitKind::BreakAnywhere,
            });
            index += 1;
            continue;
        }
        if chip_at(index) {
            // Chips are atomic: one unit for the whole mention.
            let mut end = index;
            let mut text = String::new();
            while end < chars.len() && chip_at(end) {
                text.push(chars[end]);
                end += 1;
            }
            units.push(Unit {
                width: display_width(&text),
                text,
                chip: true,
                kind: UnitKind::Word,
            });
            index = end;
            continue;
        }
        match unit_kind(ch) {
            UnitKind::Space => {
                units.push(Unit { text: " ".into(), width: 1, chip: false, kind: UnitKind::Space });
                index += 1;
            }
            UnitKind::BreakAnywhere => {
                units.push(Unit {
                    text: ch.to_string(),
                    width: display_width(&ch.to_string()),
                    chip: false,
                    kind: UnitKind::BreakAnywhere,
                });
                index += 1;
            }
            UnitKind::Word => {
                let mut text = String::new();
                while index < chars.len()
                    && !chip_at(index)
                    && chars[index] != '\n'
                    && matches!(unit_kind(chars[index]), UnitKind::Word)
                {
                    text.push(chars[index]);
                    index += 1;
                }
                units.push(Unit {
                    width: display_width(&text),
                    text,
                    chip: false,
                    kind: UnitKind::Word,
                });
            }
        }
    }
    units
}

/// The caret's visual cell `(row, col)` for a char offset in the
/// draft, walking the SAME unit/wrap model [`layout_draft_lines`]
/// paints (consumed wrap spaces advance the char count without
/// painting — the mapping re-derives the break decisions, so the
/// cursor lands exactly where the text around it renders).
///
/// The wrap model here is intentionally aligned with (but simpler
/// than) [`layout_draft_lines`]; the golden harness only pins
/// caret-less frames, so this mapping's contract is the live cursor
/// position (unit tests below pin the exact cells).
#[must_use]
pub fn caret_cell(draft: &PromptDraft, inner: usize, caret: usize) -> (usize, usize) {
    let mut row = 0usize;
    let mut col = 0usize;
    let mut consumed = 0usize; // chars consumed so far (incl. newlines)
    let mut line_width = 0usize;
    for unit in draft_units(draft) {
        if unit.text == "\n" {
            if consumed >= caret {
                break;
            }
            consumed += 1;
            row += 1;
            col = 0;
            line_width = 0;
            continue;
        }
        // The same break decision as `layout_draft_lines`: any unit
        // that does not fit closes the current row (overwide units
        // close it unconditionally, then hard-fill).
        if line_width + unit.width > inner {
            row += 1;
            col = 0;
            line_width = 0;
            // A boundary SPACE is consumed by the break (not painted).
            if unit.kind.is_space() {
                if consumed >= caret {
                    break;
                }
                consumed += 1;
                continue;
            }
        }
        // Walk the unit's chars: overwide units hard-fill row by row.
        let mut unit_width_used = 0usize;
        for ch in unit.text.chars() {
            if consumed >= caret {
                break;
            }
            let ch_width = display_width(&ch.to_string()).max(1);
            if unit_width_used + ch_width > inner {
                row += 1;
                col = 0;
                unit_width_used = 0;
                line_width = 0;
            }
            consumed += 1;
            col += ch_width;
            unit_width_used += ch_width;
            line_width += ch_width;
        }
    }
    (row, col)
}

/// Lay the draft out at `inner` columns: source lines wrap
/// independently; a break keeps the trailing space on the previous
/// line when it fits there, and consumes it when it does not.
#[must_use]
pub fn layout_draft_lines(draft: &PromptDraft, inner: usize) -> Vec<Vec<Span>> {
    // Split into source lines first (newlines are hard breaks), then
    // wrap each line's units.
    let mut source_lines: Vec<Vec<Unit>> = vec![Vec::new()];
    for unit in draft_units(draft) {
        if unit.text == "\n" {
            source_lines.push(Vec::new());
        } else {
            source_lines.last_mut().expect("seeded").push(unit);
        }
    }
    let mut lines: Vec<Vec<Span>> = Vec::new();
    for source in source_lines {
        let mut pending: Vec<Unit> = Vec::new();
        let mut width = 0usize;
        for unit in source {
            let unit_width = unit.width;
            match unit.kind {
                UnitKind::Space => {
                    if width + unit_width <= inner {
                        pending.push(unit);
                        width += unit_width;
                    } else {
                        // The space wraps: consumed, line breaks.
                        lines.push(merge_spans(&pending));
                        pending.clear();
                        width = 0;
                    }
                }
                UnitKind::Word | UnitKind::BreakAnywhere => {
                    if width + unit_width <= inner {
                        pending.push(unit);
                        width += unit_width;
                    } else if unit_width > inner {
                        // Overwide unit: hard-fill char by char.
                        lines.push(merge_spans(&pending));
                        pending.clear();
                        let mut chunk = String::new();
                        let mut chunk_width = 0usize;
                        for ch in unit.text.chars() {
                            let char_width = display_width(&ch.to_string());
                            if chunk_width + char_width > inner {
                                lines.push(vec![Span {
                                    text: std::mem::take(&mut chunk),
                                    chip: unit.chip,
                                }]);
                                chunk_width = 0;
                            }
                            chunk.push(ch);
                            chunk_width += char_width;
                        }
                        pending.push(Unit {
                            text: chunk,
                            width: chunk_width,
                            chip: unit.chip,
                            kind: UnitKind::Word,
                        });
                        width = chunk_width;
                    } else {
                        lines.push(merge_spans(&pending));
                        pending.clear();
                        pending.push(unit);
                        width = unit_width;
                    }
                }
            }
        }
        lines.push(merge_spans(&pending));
    }
    if lines.is_empty() {
        lines.push(Vec::new());
    }
    lines
}

fn merge_spans(units: &[Unit]) -> Vec<Span> {
    let mut spans: Vec<Span> = Vec::new();
    for unit in units {
        if let Some(last) = spans.last_mut() {
            if last.chip == unit.chip {
                last.text.push_str(&unit.text);
                continue;
            }
        }
        spans.push(Span { text: unit.text.clone(), chip: unit.chip });
    }
    spans
}

#[must_use]
fn cell_color(color: Rgba) -> CellColor {
    CellColor::Rgb(color.r, color.g, color.b)
}

/// The composer mount's intrinsic height: the box (1 pad + textarea
/// rows + 1 metadata pad) + the ╹▀ transition + the footer row.
#[must_use]
pub fn mount_height(draft: &PromptDraft, width: usize, height: usize) -> usize {
    let pad = if width >= 44 { 2 } else { 1 };
    let inner = width.saturating_sub(1 + pad * 2).max(1);
    let lines = layout_draft_lines(draft, inner);
    let max_height = (height / 3).max(6);
    let textarea_rows = (lines.len().max(1) + 1).min(max_height);
    1 + textarea_rows + 1 + 2
}

/// Render the composer mount (golden harness shape: the box at
/// [`PromptView::top`], the popup above it, the footer row below).
#[must_use]
/// The theme's resolved painter palette (one place per color the
/// composer surfaces consume).
pub(crate) struct Palette {
    pub(crate) base: CellColor,
    pub(crate) prompt_bg: CellColor,
    pub(crate) border: CellColor,
    pub(crate) rail: CellColor,
    pub(crate) text_base: CellColor,
    pub(crate) muted: CellColor,
    pub(crate) default_fg: CellColor,
    pub(crate) raised_high: CellColor,
    pub(crate) popup_selected_bg: CellColor,
    pub(crate) popup_selected_text: CellColor,
    pub(crate) chip_color: CellColor,
}

pub(crate) fn palette(theme: &ThemeState) -> Palette {
    let tokens = &theme.resolved.tokens;
    let border = cell_color(tokens.border.base);
    Palette {
        base: cell_color(tokens.background.base),
        prompt_bg: cell_color(tokens.decrease(tokens.background.raised.base, 1.0)),
        rail: border,
        border,
        text_base: cell_color(tokens.text.base),
        muted: cell_color(tokens.text.muted),
        default_fg: cell_color(Rgba { r: 255, g: 255, b: 255, a: 255 }),
        raised_high: cell_color(tokens.background.raised.high),
        popup_selected_bg: cell_color(tokens.background.action.primary.focused),
        popup_selected_text: cell_color(tokens.text.action.primary.focused),
        chip_color: cell_color(tokens.text.feedback.warning.base),
    }
}

#[must_use]
pub fn render_prompt_frame(
    view: &PromptView,
    width: usize,
    height: usize,
    theme: &ThemeState,
) -> Frame {
    let colors = palette(theme);
    let colors = Palette { rail: view.rail.unwrap_or(colors.border), ..colors };
    let mut frame = Frame { cells: vec![vec![Cell::default(); width]; height], width, height };
    // Page fill: the renderer's default foreground over background.base.
    for row in &mut frame.cells {
        for cell in row.iter_mut() {
            cell.fg = colors.default_fg;
            cell.bg = colors.base;
        }
    }

    let pad = if width >= 44 { 2 } else { 1 };
    let inner = width.saturating_sub(1 + pad * 2).max(1);
    let lines = layout_draft_lines(view.draft, inner);
    let max_height = (height / 3).max(6);
    let textarea_rows = (lines.len().max(1) + 1).min(max_height);
    let content_h = 1 + textarea_rows + 1;
    let top = view.top;

    paint_composer_box(
        &mut frame,
        view,
        &lines,
        &colors,
        pad,
        textarea_rows,
        content_h,
        inner,
        width,
        height,
    );
    // The blinking block cursor (`cursorColor = theme.text.base`):
    // painted only when the caller resolved BOTH a caret offset and
    // the visible blink phase.
    if let Some(offset) = view.caret {
        let (row, col) = caret_cell(view.draft, inner, offset);
        if row < textarea_rows {
            let (x, y) = (1 + pad + col, top + 1 + row);
            if let Some(cell) = frame.cells.get_mut(y).and_then(|line| line.get_mut(x)) {
                cell.bg = colors.text_base;
                cell.fg = colors.prompt_bg;
            }
        }
    }
    paint_footer_row(&mut frame, view, &colors, top + content_h + 1, width, height);
    if let Some(popup) = &view.autocomplete {
        paint_autocomplete_popup(&mut frame, popup, &colors, top, width, height);
    }
    frame
}

/// The composer body: left border, prompt-bg fill, the textarea's
/// placeholder/lines, and the ╹▀ transition row.
#[allow(clippy::too_many_arguments)]
fn paint_composer_box(
    frame: &mut Frame,
    view: &PromptView,
    lines: &[Vec<Span>],
    colors: &Palette,
    pad: usize,
    textarea_rows: usize,
    content_h: usize,
    inner: usize,
    width: usize,
    height: usize,
) {
    let top = view.top;
    for row in top..(top + content_h).min(height) {
        let line = &mut frame.cells[row];
        // The left border in `borderHighlight()` (the agent color at
        // fade alpha 1; `border.base` while no agent resolved).
        line[0] = Cell { ch: '┃', attrs: 0, fg: colors.rail, bg: colors.base };
        for cell in &mut line[1..] {
            cell.fg = colors.default_fg;
            cell.bg = colors.prompt_bg;
        }
    }
    // Textarea row 1: the placeholder (truncated to inner − 2, muted)
    // or the wrapped lines with their extmark chips.
    let text_x = 1 + pad;
    let text_y = top + 1;
    if view.draft.is_empty() {
        // The placeholder truncates to inner − 2 (the caret column
        // reserve) — `composer-placeholder-empty.w40` pins 35 of 37.
        let placeholder = truncate_width(&view.placeholder, inner.saturating_sub(2));
        paint_text(frame, text_x, text_y, &placeholder, colors.muted, colors.prompt_bg, 0);
    } else {
        for (index, spans) in lines.iter().enumerate().take(textarea_rows) {
            let mut x = text_x;
            for span in spans {
                let (fg, attrs) =
                    if span.chip { (colors.chip_color, 1u16) } else { (colors.text_base, 0) };
                paint_text(frame, x, text_y + index, &span.text, fg, colors.prompt_bg, attrs);
                x += display_width(&span.text);
            }
        }
    }
    // Bottom transition: ╹ + ▀ row (the corner in the rail color —
    // the transition box's left border is `borderHighlight()` — and
    // upper-half blocks in promptBg).
    if top + content_h < height {
        let row = &mut frame.cells[top + content_h];
        row[0] = Cell { ch: '╹', attrs: 0, fg: colors.rail, bg: colors.base };
        for cell in &mut row[1..] {
            *cell = Cell { ch: '▀', attrs: 0, fg: colors.prompt_bg, bg: colors.base };
        }
    }
    // The metadata row (#5, `PromptMetadataRow`): the box's last
    // content row — `agent · model provider · variant` with the agent
    // in the rail color, separators/provider muted, model in
    // text.base, variant in warning bold (metadata.tsx:36-87). Blank
    // (promptBg fill) while no agent/model resolved — the golden
    // shape's empty row.
    let meta_y = top + content_h - 1;
    if meta_y < height {
        let layout = view.metadata.layout(view.terminal_width);
        // `Show when={(mode === "shell" || props.agent) && (layout().agent || layout().model)}`.
        let shell_or_agent = !view.metadata.agent.is_empty();
        if (shell_or_agent || !layout.model.is_empty())
            && (!layout.agent.is_empty() || !layout.model.is_empty())
        {
            let mut x = 1 + pad;
            let mut paint = |x: &mut usize, text: &str, fg: CellColor, attrs: u16| {
                paint_text(frame, *x, meta_y, text, fg, colors.prompt_bg, attrs);
                *x += display_width(text) + 1; // gap 1 between spans
            };
            if !layout.agent.is_empty() {
                paint(&mut x, &layout.agent, colors.rail, 0);
            }
            if layout.auto {
                paint(&mut x, "auto", colors.muted, 0);
            }
            if !layout.model.is_empty() {
                if !layout.agent.is_empty() {
                    paint(&mut x, "·", colors.muted, 0);
                }
                paint(&mut x, &layout.model, colors.text_base, 0);
            }
            if !layout.provider.is_empty() {
                paint(&mut x, &layout.provider, colors.muted, 0);
            }
            if !layout.variant.is_empty() {
                paint(&mut x, "·", colors.muted, 0);
                paint(&mut x, &layout.variant, colors.chip_color, 1);
            }
        }
    }
    let _ = width;
}

/// The footer row's resolved paint plan — ONE source of truth for
/// the #4/#20/#33 geometry, shared by [`paint_footer_row`] (the old
/// renderer), [`footer_location_extent`] (the hover hit rect), and
/// the opentui-solid bridge (`composer_solid.rs`): the truncated
/// location label, the interrupt hint's slot, the right-aligned
/// shortcut block (`{key} agents`/usage + `{key} commands` with the
/// key/label split `paint_hint` performs), and the plugin segment.
#[derive(Debug, Clone, Default)]
pub(crate) struct FooterPlan {
    /// The truncated location label ("" when nothing fits).
    pub location: String,
    /// The interrupt run's x (`esc ` first) — `None` when absent.
    pub interrupt_x: Option<usize>,
    /// The interrupt label after `esc `.
    pub interrupt_label: String,
    /// The right block's first segment x — `None` when width-gated
    /// off (usage arm or `{key} agents`).
    pub block_x: Option<usize>,
    /// The block's second segment x (`{key} commands`).
    pub command_x: Option<usize>,
    /// The block starts with the usage arm (the agents hint drops).
    pub usage_first: bool,
    /// The usage segment (only painted when `usage_first`).
    pub usage: String,
    /// The agents hint's `{key}` run (before the fixed `agents` label).
    pub agent_key: String,
    /// The commands hint's `{key}` run.
    pub command_key: String,
    /// The plugin segment's x + truncated text — `None` when absent.
    pub plugin: Option<(usize, String)>,
}

/// Resolve the footer row's plan at `width` (see [`FooterPlan`]).
#[must_use]
pub(crate) fn footer_plan(view: &PromptView, width: usize) -> FooterPlan {
    let agent_hint = format!("{} agents", view.footer.agent_shortcut);
    let command_hint = format!("{} commands", view.footer.command_shortcut);
    let usage = &view.footer.usage;
    let usage_width = display_width(usage);
    // `promptFooterLayout` (footer.tsx:50-60 + 131-143): with usage
    // present the packed `usage · commands` must fit
    // `(terminal-8) - min(28, (terminal-8)/2)` — else the commands
    // hint drops (usage alone), else nothing. Without usage the hints
    // gate at the raw terminal width ≥ 44 (footer.tsx:56).
    let hints_shown = if usage_width > 0 {
        let budget_width = view.terminal_width.saturating_sub(8);
        let available = budget_width.saturating_sub(28.min(budget_width / 2));
        display_width(&format!("{usage} · {command_hint}")) <= available
    } else {
        view.terminal_width >= 44
    };
    let command_width = display_width(&command_hint);
    let block_width = if usage_width > 0 {
        usage_width + 2 + command_width
    } else {
        display_width(&agent_hint) + 2 + command_width
    };
    let plugin_text = view.footer.plugin_hints.join(" · ");
    let plugin_width = display_width(&plugin_text);
    let right_block = if hints_shown {
        block_width + if plugin_width > 0 { plugin_width + 2 } else { 0 }
    } else {
        plugin_width
    };
    // Wave A: the interrupt hint (`PromptInterruptStatus`,
    // prompt/index.tsx:137-144) sits at the row's left — `esc` in
    // text.base with the label muted — while a turn runs; the
    // location label follows it (empty hint ⇒ the golden geometry).
    let interrupt_width = display_width(&view.footer.interrupt);
    // The location truncates into whatever the right block leaves
    // (`flexShrink 1 minWidth 0` + the row's gap 2) — the RIGHT side
    // never overflows the row, so a long label can never collide with
    // the hints (#20). Nothing fits ⇒ the label vanishes.
    let location_budget = width
        .saturating_sub(right_block + 2)
        .saturating_sub(if interrupt_width > 0 { interrupt_width + 2 } else { 0 });
    let location = if location_budget == 0 {
        String::new()
    } else {
        truncate_file_path(&view.footer.location, location_budget)
    };
    // `paint_hint`'s key/label split: the fixed label off the tail.
    let key_of = |hint: &str, label: &str| {
        let key_len = hint.len().saturating_sub(label.len());
        hint[..key_len].to_owned()
    };
    let interrupt_x = (interrupt_width > 0)
        .then(|| display_width(&location) + usize::from(!location.is_empty()) * 2);
    let block_x = hints_shown.then(|| width.saturating_sub(block_width));
    let command_x = hints_shown.then(|| {
        if usage_width > 0 {
            width.saturating_sub(block_width) + usage_width + 2
        } else {
            width.saturating_sub(block_width) + display_width(&agent_hint) + 2
        }
    });
    FooterPlan {
        location,
        interrupt_x,
        interrupt_label: view.footer.interrupt.strip_prefix("esc ").unwrap_or("").to_owned(),
        block_x,
        command_x,
        usage_first: usage_width > 0,
        usage: usage.clone(),
        agent_key: key_of(&agent_hint, "agents"),
        command_key: key_of(&command_hint, "commands"),
        plugin: (plugin_width > 0).then(|| {
            let x = width.saturating_sub(right_block);
            let budget = width.saturating_sub(x);
            (x, truncate_width(&plugin_text, budget))
        }),
    }
}

/// The location label's column extent on the footer row
/// (`prompt.footer.status`, prompt/index.tsx:1925-1937): the label
/// truncates into the space left of the right-aligned hints — the
/// hover/click surface (`useWorkingDirectoryActions`). None when the
/// label is empty. `(x, width)` in frame columns. Derived from the
/// SAME [`footer_plan`] math the row paints with (one source of
/// truth; the hover-sweep rig pins them together).
#[must_use]
pub fn footer_location_extent(view: &PromptView, width: usize) -> Option<(usize, usize)> {
    if view.footer.location.is_empty() {
        return None;
    }
    let painted = display_width(&footer_plan(view, width).location);
    (painted > 0).then_some((0, painted))
}

/// One `{key} label` hint pair at `x`; returns the end column.
fn paint_hint(
    frame: &mut Frame,
    x: usize,
    y: usize,
    key: &str,
    label: &str,
    colors: &Palette,
) -> usize {
    paint_text(frame, x, y, key, colors.text_base, colors.base, 0);
    let after = x + display_width(key);
    paint_text(frame, after, y, label, colors.muted, colors.base, 0);
    after + display_width(label)
}

/// The footer row (#4/#20, prompt/index.tsx:1875-1959): the
/// `{directory}:{branch}` location label muted in the flexible LEFT
/// slot (`truncateFilePath`, minWidth 0 — never collides with the
/// hints), the interrupt hint left while a turn runs, and the plugin
/// hints + `{key} agents` / `{key} commands` shortcut block
/// right-aligned (keys `text.base`, labels muted, gated at ≥ 44 cols).
/// The context-usage arm swaps [agents] for [usage] once a turn
/// completed (footer.tsx:65-94). No model statusline — that lives in
/// the composer's metadata row. All geometry comes from
/// [`footer_plan`] (shared with the solid bridge).
fn paint_footer_row(
    frame: &mut Frame,
    view: &PromptView,
    colors: &Palette,
    row_index: usize,
    width: usize,
    height: usize,
) {
    if row_index >= height {
        return;
    }
    let plan = footer_plan(view, width);
    // The hover lift (`locationActions.hovered()` → text.base,
    // prompt/index.tsx:1927) — #33's machinery rides on #4's paint.
    let location_fg = if view.footer.location_hovered { colors.text_base } else { colors.muted };
    paint_text(frame, 0, row_index, &plan.location, location_fg, colors.base, 0);
    if let Some(x) = plan.interrupt_x {
        // v2's shape: `esc ` in text.base, the label muted
        // (prompt/index.tsx:138-143).
        paint_text(frame, x, row_index, "esc ", colors.text_base, colors.base, 0);
        paint_text(frame, x + 4, row_index, &plan.interrupt_label, colors.muted, colors.base, 0);
    }
    if let Some(x) = plan.block_x {
        if plan.usage_first {
            paint_text(frame, x, row_index, &plan.usage, colors.muted, colors.base, 0);
        } else {
            paint_hint(frame, x, row_index, &plan.agent_key, "agents", colors);
        }
    }
    if let Some(x) = plan.command_x {
        paint_hint(frame, x, row_index, &plan.command_key, "commands", colors);
    }
    // The plugin hint segment sits left of the shortcut hints (or at
    // the right edge when the hints are width-gated off).
    if let Some((x, segment)) = &plan.plugin {
        if !segment.is_empty() {
            paint_text(frame, *x, row_index, segment, colors.muted, colors.base, 0);
        }
    }
}

/// `truncateFilePath` (ui/file-path.tsx:23-77) — the footer location's
/// middle-ellipsis path truncation: keep the basename and as many
/// trailing parent segments as fit, prefixing `/…/`.
#[must_use]
pub fn truncate_file_path(value: &str, max_width: usize) -> String {
    if max_width == 0 {
        return String::new();
    }
    if display_width(value) <= max_width {
        return value.to_owned();
    }
    let separator = '/';
    let root = if value.starts_with('/') { "/" } else { "" };
    let source = &value[root.len()..];
    let segments: Vec<&str> =
        source.split(separator).filter(|segment| !segment.is_empty()).collect();
    let Some(basename) = segments.last().copied() else {
        return truncate_width(value, max_width);
    };
    if segments.len() < 2 {
        let root_width = display_width(root);
        if root_width >= max_width {
            return truncate_width(root, max_width);
        }
        return format!("{root}{}", truncate_basename(basename, max_width - root_width));
    }
    let prefix = format!("{root}…{separator}");
    let basename_width = max_width.saturating_sub(display_width(&prefix));
    if basename_width == 0 {
        return truncate_width(&prefix, max_width);
    }
    let compact = truncate_basename(basename, basename_width);
    if compact != basename {
        return format!("{prefix}{compact}");
    }
    let mut selected: Vec<String> = vec![basename.to_owned()];
    let mut used = display_width(&format!("{prefix}{basename}"));
    for segment in segments[..segments.len() - 1].iter().rev() {
        let next = display_width(segment) + 1;
        if used + next > max_width {
            let available = max_width.saturating_sub(used + 1);
            if available > 1 {
                let kept = truncate_width(segment, available - 1);
                selected.insert(0, format!("{kept}…"));
            }
            break;
        }
        selected.insert(0, (*segment).to_owned());
        used += next;
    }
    format!("{prefix}{}", selected.join("/"))
}

/// `truncateBasename` (ui/file-path.tsx:78-87): extension-preserving
/// clip — `name.tar.gz` at 9 becomes `n….gz`.
fn truncate_basename(value: &str, max_width: usize) -> String {
    if display_width(value) <= max_width {
        return value.to_owned();
    }
    if max_width <= 1 {
        return "…".to_owned();
    }
    let dot = value.rfind('.').filter(|index| *index > 0);
    let extension = dot.map(|index| &value[index..]).unwrap_or_default();
    let extension_width = display_width(extension);
    if extension_width >= max_width {
        let reversed: String = extension.chars().rev().collect();
        let kept = crate::wrap::take_width(&reversed, max_width.saturating_sub(1));
        let tail: String = kept.chars().rev().collect();
        return format!("…{tail}");
    }
    let stem = dot.map_or(value, |index| &value[..index]);
    let kept = crate::wrap::take_width(stem, max_width - extension_width - 1);
    format!("{kept}…{extension}")
}

/// The autocomplete popup above the box: bordered rows, the selected
/// row filled `action.primary.focused`, title column padded, the
/// description one space ahead.
fn paint_autocomplete_popup(
    frame: &mut Frame,
    popup: &AutocompleteView,
    colors: &Palette,
    top: usize,
    width: usize,
    height: usize,
) {
    let count = popup.options.len();
    let popup_h = popup_height(count, top);
    for (row_index, option) in popup.options.iter().take(popup_h).enumerate() {
        let y = top - popup_h + row_index;
        if y >= height {
            continue;
        }
        let selected = row_index == popup.selected;
        let row = &mut frame.cells[y];
        row[0] = Cell { ch: '┃', attrs: 0, fg: colors.border, bg: colors.base };
        let last = width - 1;
        row[last] = Cell { ch: '┃', attrs: 0, fg: colors.border, bg: colors.base };
        let row_bg = if selected { colors.popup_selected_bg } else { colors.raised_high };
        for cell in &mut row[1..last] {
            cell.fg = colors.default_fg;
            cell.bg = row_bg;
        }
        let title_color = if selected { colors.popup_selected_text } else { colors.text_base };
        let desc_color = if selected { colors.popup_selected_text } else { colors.muted };
        let title = format!("{:width$}", option.display, width = popup.title_width);
        paint_text(frame, 2, y, &title, title_color, row_bg, 0);
        let mut x = 2 + popup.title_width;
        if let Some(description) = &option.description {
            let desc = format!(" {description}");
            paint_text(frame, x, y, &desc, desc_color, row_bg, 0);
            x += display_width(&desc);
        }
        let _ = x;
    }
}

/// Paint `text` starting at (`x`, `y`) with one style; wide clusters
/// mark their continuation cells with `'\0'` (skipped by the text grid
/// and the canonical serializer, matching the exporter's cell walk).
fn paint_text(
    frame: &mut Frame,
    x: usize,
    y: usize,
    text: &str,
    fg: CellColor,
    bg: CellColor,
    attrs: u16,
) {
    let Some(row) = frame.cells.get_mut(y) else { return };
    let mut x = x;
    for atom in crate::wrap::flatten(text, ratatui::style::Style::new()) {
        let width = atom.width.max(1);
        if x >= row.len() {
            break;
        }
        row[x] = Cell { ch: atom.text.chars().next().unwrap_or(' '), attrs, fg, bg };
        let span_end = (x + width).min(row.len());
        for continuation in &mut row[(x + 1)..span_end] {
            *continuation = Cell { ch: '\u{0}', attrs, fg, bg };
        }
        x += width;
    }
}

/// Truncate by display width (chars kept while the width budget lasts).
#[must_use]
pub(crate) fn truncate_width(text: &str, limit: usize) -> String {
    let mut budget = limit;
    let mut out = String::new();
    for ch in text.chars() {
        let width = display_width(&ch.to_string());
        if width > budget {
            break;
        }
        budget -= width;
        out.push(ch);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::components::composer::FileMention;

    fn draft(text: &str) -> PromptDraft {
        PromptDraft { text: text.to_owned(), ..PromptDraft::default() }
    }

    /// The blink phase is a pure function of the injected clock: on
    /// for the first 500 ms, off for the next 500, cycling.
    #[test]
    fn cursor_blink_follows_the_clock() {
        assert!(cursor_blink_on(0), "visible at t=0 (start visible)");
        assert!(cursor_blink_on(499));
        assert!(!cursor_blink_on(500), "hidden in the second half-phase");
        assert!(!cursor_blink_on(999));
        assert!(cursor_blink_on(1000), "the phase cycles");
        // Deterministic: the same clock always yields the same phase.
        assert_eq!(cursor_blink_on(1234), cursor_blink_on(1234));
    }

    /// The caret lands where the surrounding text renders: within a
    /// word, at wrap boundaries (the consumed break space), across
    /// explicit newlines, and on an empty draft.
    #[test]
    fn caret_cell_tracks_the_wrap_layout() {
        // Empty draft: the block sits at the text origin.
        assert_eq!(caret_cell(&draft(""), 37, 0), (0, 0));
        // Mid-word: col = chars before the caret.
        assert_eq!(caret_cell(&draft("hello"), 37, 2), (0, 2));
        // End of text: col = full width.
        assert_eq!(caret_cell(&draft("hello"), 37, 5), (0, 5));
        // A wrap that fits exactly then breaks: the boundary space is
        // consumed by the break (layout keeps it out of both rows).
        let wrapped = "word word word word word word word word word word";
        let (row, col) = caret_cell(&draft(wrapped), 37, wrapped.chars().count());
        assert_eq!(row, 1, "the tail wraps to the second row");
        assert!(col <= 37, "col within the inner width: {col}");
        // Explicit newline: the caret moves to the next row's start.
        assert_eq!(caret_cell(&draft("ab\ncd"), 37, 3), (1, 0));
        assert_eq!(caret_cell(&draft("ab\ncd"), 37, 5), (1, 2));
    }

    /// The block cursor paints ONLY when the caller passes the caret
    /// (blink phase on): bg = `text.base` over the prompt-bg cell, fg
    /// inverted for legibility; `None` keeps the golden bytes.
    #[test]
    fn frame_paints_the_blinking_block_cursor() {
        let theme = ThemeState::v2_dark();
        let owned = draft("hi");
        let view = |caret: Option<usize>| PromptView {
            draft: &owned,
            placeholder: String::new(),
            autocomplete: None,
            footer: FooterHints::default(),
            metadata: MetadataLine::default(),
            terminal_width: 40,
            top: 0,
            rail: None,
            caret,
        };
        let with = render_prompt_frame(&view(Some(2)), 40, 8, &theme);
        // text_x = 1 + pad(1 <44) = 2, text_y = top + 1 = 1.
        let cell = with.cell(1, 4);
        assert_eq!(cell.bg, CellColor::Rgb(238, 238, 238), "text.base block");
        assert_eq!(cell.fg, CellColor::Rgb(30, 30, 30), "prompt-bg fg over the block");
        let without = render_prompt_frame(&view(None), 40, 8, &theme);
        assert_ne!(
            without.cell(1, 4).bg,
            CellColor::Rgb(238, 238, 238),
            "no block without the caret"
        );
    }

    /// The agent-colored rail: `rail: Some(blue)` recolors the `┃`
    /// border and `╹` corner; `None` keeps the golden `border.base`
    /// gray (the no-agent capture state).
    #[test]
    fn rail_color_follows_the_agent_override() {
        let theme = ThemeState::v2_dark();
        let owned = draft("");
        let view = |rail: Option<CellColor>| PromptView {
            draft: &owned,
            placeholder: "Ask".into(),
            autocomplete: None,
            footer: FooterHints::default(),
            metadata: MetadataLine::default(),
            terminal_width: 40,
            top: 0,
            rail,
            caret: None,
        };
        let gray = render_prompt_frame(&view(None), 40, 8, &theme);
        assert_eq!(gray.cell(1, 0).fg, CellColor::Rgb(72, 72, 72), "border.base fallback");
        let blue = render_prompt_frame(&view(Some(CellColor::Rgb(92, 156, 245))), 40, 8, &theme);
        assert_eq!(blue.cell(1, 0).fg, CellColor::Rgb(92, 156, 245), "categorical[0] rail");
        // The ╹ corner carries the same rail color.
        let corner_y = blue.cells.iter().rposition(|row| row[0].ch == '╹').expect("corner");
        assert_eq!(blue.cell(corner_y, 0).fg, CellColor::Rgb(92, 156, 245));
        // The AUTOCOMPLETE popup rails stay border.base (v2:
        // autocomplete.tsx borderColor={theme.border.base}).
    }

    #[test]
    fn chip_draft_wraps_keeping_the_break_space() {
        // The w40 attachment golden: line 1 keeps the trailing space
        // ("…the current " 35 cells), "design" wraps whole.
        let start = "Compare [Image 1] with the current design".find("[Image").unwrap();
        let end = start + "[Image 1]".len();
        let d = PromptDraft {
            text: "Compare [Image 1] with the current design".into(),
            files: vec![FileMention {
                name: "screenshot.png".into(),
                start,
                end,
                payload: None,
                mention: Some("[Image 1]".into()),
            }],
            ..PromptDraft::default()
        };
        let lines = layout_draft_lines(&d, 37);
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0][0].text, "Compare ");
        assert_eq!(lines[0][1].text, "[Image 1]");
        assert!(lines[0][1].chip);
        assert_eq!(lines[0][2].text, " with the current ");
        assert_eq!(lines[0].iter().map(|span| display_width(&span.text)).sum::<usize>(), 35);
        assert_eq!(lines[1][0].text, "design");
    }

    #[test]
    fn cjk_breaks_anywhere_and_consumes_the_boundary_space() {
        // The w40 CJK golden: line 1 fills 37 exactly ("…（English"),
        // the boundary space wraps and is consumed, まじり） follows.
        let text = "検証：日本語プロンプトの入力（English まじり）";
        let lines = layout_draft_lines(&draft(text), 37);
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0][0].text, "検証：日本語プロンプトの入力（English");
        assert_eq!(display_width(&lines[0][0].text), 37);
        assert_eq!(lines[1][0].text, "まじり）", "boundary space consumed");
    }

    #[test]
    fn explicit_newlines_are_hard_breaks() {
        let lines = layout_draft_lines(&draft("one\ntwo\nthree"), 80);
        assert_eq!(lines.len(), 3);
        assert_eq!(lines[1][0].text, "two");
    }

    // -----------------------------------------------------------------
    // Lane/chrome (#4/#5/#20/NEW-S1a, metadata.tsx + file-path.tsx)
    // -----------------------------------------------------------------

    fn meta_view(meta: MetadataLine, location: &str, width: usize) -> Frame {
        let owned = draft("");
        let view = PromptView {
            draft: &owned,
            placeholder: String::new(),
            autocomplete: None,
            footer: FooterHints {
                location: location.to_owned(),
                agent_shortcut: "shift+tab".into(),
                command_shortcut: "ctrl+p".into(),
                ..FooterHints::default()
            },
            metadata: meta,
            terminal_width: width,
            top: 0,
            // The rail/agent categorical color the live route passes
            // (`composer_rail_color`); the metadata agent paints it.
            rail: Some(CellColor::from(ThemeState::v2_dark().styles().agent_colors[0])),
            caret: None,
        };
        render_prompt_frame(&view, width, 8, &ThemeState::v2_dark())
    }

    /// #5: the metadata row renders `Build · Fake Model parityfake`
    /// inside the prompt box — agent in the rail color, `·` muted,
    /// model in text.base, provider muted.
    #[test]
    fn metadata_row_renders_agent_model_provider() {
        let theme = ThemeState::v2_dark();
        let frame = meta_view(
            MetadataLine {
                agent: "Build".into(),
                model: "Fake Model".into(),
                provider: "parityfake".into(),
                ..MetadataLine::default()
            },
            "",
            80,
        );
        // The composer mount at width 80, top 0, empty draft: pad row
        // 0, textarea rows 1-2 (wrapped + cursor row), metadata row 3.
        let row = frame.text_grid()[3].trim_end().to_owned();
        assert!(row.ends_with("Build · Fake Model parityfake"), "{row}");
        let tokens = &theme.resolved.tokens;
        let line = &frame.cells[3];
        let text: String = line.iter().map(|cell| cell.ch).collect();
        let build_x = text.find('B').expect("Build");
        let fake_x = text.find("Fake").expect("Fake Model");
        let parity_x = text.find("parityfake").expect("provider");
        let agent_colors = theme.styles().agent_colors;
        assert_eq!(line[build_x].fg, CellColor::from(agent_colors[0]), "agent categorical");
        assert_eq!(line[fake_x].fg, cell_color_of(tokens.text.base), "model text.base");
        assert_eq!(line[parity_x].fg, cell_color_of(tokens.text.muted), "provider muted");
    }

    /// The metadata layout's drop order (metadata.tsx:112-136): full
    /// fits, then no-auto, then the provider's last ` / ` segment,
    /// then no provider, then a width-truncated model.
    #[test]
    fn metadata_layout_drops_in_the_reference_order() {
        let meta = MetadataLine {
            agent: "Build".into(),
            auto: true,
            model: "Fake Model".into(),
            provider: "NanoGPT / parityfake".into(),
            variant: String::new(),
        };
        assert_eq!(meta.layout(80).text(), "Build auto · Fake Model NanoGPT / parityfake");
        // Same budget as 80 for any width ≥ 44 (the drop only reads
        // the terminal width bucket).
        assert_eq!(meta.layout(60).text(), "Build auto · Fake Model NanoGPT / parityfake");
        let no_auto = meta.layout(52);
        assert_eq!(no_auto.text(), "Build · Fake Model NanoGPT / parityfake");
        let last_segment = meta.layout(48);
        assert_eq!(last_segment.text(), "Build · Fake Model parityfake");
        // At 44 the budget is 31: the last-segment candidate fits.
        let last_segment_wins = meta.layout(44);
        assert_eq!(last_segment_wins.text(), "Build · Fake Model parityfake");
        // A longer model pushes past the last-segment candidate to the
        // provider-less one.
        let longer = MetadataLine {
            agent: "Build".into(),
            model: "A Bit Longer Model Name".into(),
            provider: "NanoGPT / parityfake".into(),
            ..MetadataLine::default()
        };
        let no_provider = longer.layout(44);
        assert_eq!(no_provider.text(), "Build · A Bit Longer Model Name");
        // Below 44 the agent and provider drop entirely.
        let narrow = MetadataLine {
            agent: "Build".into(),
            model: "Fake Model".into(),
            provider: "parityfake".into(),
            ..MetadataLine::default()
        }
        .layout(40);
        assert_eq!(narrow.text(), "Fake Model");
        // A model longer than the budget truncates with an ellipsis.
        let long = MetadataLine {
            agent: "Build".into(),
            model: "A Very Long Model Name Indeed".into(),
            ..MetadataLine::default()
        }
        .layout(44);
        assert!(long.text().starts_with("Build · A Very"), "{}", long.text());
        assert!(long.text().ends_with('…'), "{}", long.text());
    }

    /// #4/#20: the footer row — location muted left, `shift+tab
    /// agents  ctrl+p commands` right, and a LONG location truncates
    /// via `truncateFilePath` without ever touching the hint block
    /// (the 60-col collision the corpus caught).
    #[test]
    fn footer_row_location_and_hints_never_collide() {
        let long_path = "/tmp/opencode/tui-diff/type-edit-submit-80x24-r0/project:master";
        for width in [80usize, 60] {
            let frame = meta_view(MetadataLine::default(), long_path, width);
            let footer = frame.text_grid()[5].trim_end().to_owned();
            // The hint block stays INTACT and right-aligned to the
            // frame edge; the truncated location never touches it.
            assert!(footer.ends_with("shift+tab agents  ctrl+p commands"), "at {width}: {footer}");
            let hints_x = footer.find("shift+tab agents").expect("hints present");
            let location = footer[..hints_x].trim_end();
            assert!(location.ends_with("project:master"), "location tail at {width}: {location}");
            assert!(location.starts_with("/…/"), "location head at {width}: {location}");
        }
        // At 44 the budget collapses to the clipped basename; at 40
        // the hints vanish and only the location remains.
        let frame = meta_view(MetadataLine::default(), long_path, 44);
        let footer = frame.text_grid()[5].trim_end().to_owned();
        assert!(footer.contains("shift+tab agents"), "hints at the 44 gate: {footer}");
        assert!(footer.starts_with("/…/proje…"), "clipped basename at 44: {footer}");
        let frame = meta_view(MetadataLine::default(), long_path, 40);
        let footer = frame.text_grid()[5].trim_end().to_owned();
        assert!(!footer.contains("shift+tab"), "{footer}");
        assert!(footer.contains("project:master"), "{footer}");
    }

    /// `truncateFilePath` against the corpus-observed shapes
    /// (ui/file-path.tsx:23-77): exact fit, the `/…/seg…/base`
    /// middle form, basename-extension clip, and the short-input
    /// fallbacks.
    #[test]
    fn truncate_file_path_matches_the_reference_shapes() {
        assert_eq!(truncate_file_path("/a/b", 10), "/a/b");
        // The corpus's exact 80-col footer shape (budget 40; the
        // 120-col run truncates differently: the whole parent fits).
        assert_eq!(
            truncate_file_path(
                "/tmp/opencode/tui-diff/type-edit-submit-80x24-r0/project:master",
                40
            ),
            "/…/type-edit-submit-80x2…/project:master"
        );
        assert_eq!(
            truncate_file_path("/tmp/opencode/tui-diff/boot-home-120x30-r0/project:master", 40),
            "/…/t…/boot-home-120x30-r0/project:master"
        );
        // A dot in the basename keeps its extension (the LAST dot).
        assert_eq!(truncate_file_path("/a/b/archive.tar.gz", 13), "/…/archiv….gz");
        // No parents: root + clipped basename.
        assert_eq!(truncate_file_path("/longname.txt", 11), "/longn….txt");
        // Zero budget: nothing.
        assert_eq!(truncate_file_path("/a/b", 0), "");
    }

    /// NEW-S1a: no painted caret — the live caret is the TERMINAL
    /// cursor; the software block never renders on the live route
    /// (the helper returns the cell for `set_cursor_position`).
    #[test]
    fn live_caret_positions_the_terminal_cursor_never_cells() {
        let owned = draft("hi");
        let view = PromptView {
            draft: &owned,
            placeholder: String::new(),
            autocomplete: None,
            footer: FooterHints::default(),
            metadata: MetadataLine::default(),
            terminal_width: 40,
            top: 0,
            rail: None,
            caret: None,
        };
        let frame = render_prompt_frame(&view, 40, 8, &ThemeState::v2_dark());
        for cell in &frame.cells[1] {
            assert_ne!(cell.bg, CellColor::Rgb(238, 238, 238), "no block caret bg");
        }
        // The position helper: offset 2 in "hi" → row 0, col 2 →
        // x = 1 + pad(1 at w40) + 2, y = top + 1.
        assert_eq!(prompt_caret_screen_cell(&owned, 40, 0, 8, 2), Some((4, 1)));
    }

    fn cell_color_of(color: oc_theme::Rgba) -> CellColor {
        CellColor::Rgb(color.r, color.g, color.b)
    }
}

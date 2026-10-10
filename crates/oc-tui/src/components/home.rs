//! The home route (`routes/home.tsx`): the OPENCODE wordmark, the
//! centered prompt composer (the same `Prompt` the session route
//! docks), and the `home.footer` plugin slot — **golden-pinned**
//! (`home.w80` / `home.w160`, text grid + canonical ANSI byte-exact;
//! the Phase-8 export chore, closed).
//!
//! ## Layout (read off the golden + `routes/home.tsx`)
//!
//! ```text
//! [ flexGrow spacer ]      ← the column centers its content
//! [ 3 fixed rows ]
//! [ Logo (4 rows, 39 cells wide, centered) ]
//! [ 1 spacer row ]
//! [ Prompt mount: paddingTop 1 + the composer (6 rows empty) ]
//! [ flexGrow spacer ]
//! [ home.footer slot (3 rows at h≥16: pad · version · pad) ]
//! ```
//!
//! * the outer column pads `2` (1 below 44 cols) each side; centered
//!   children round their offset **up** (`(free + 1) / 2`) — taffy's
//!   half-pixel rounding, pinned by both goldens (logo x21@80/x61@160,
//!   prompt x3@80/x43@160);
//! * the prompt box is `width 100% maxWidth 75` — 75 cells wide at
//!   both golden widths;
//! * the footer slot (`feature-plugins/home/footer.tsx`) shows the app
//!   version right-aligned (`paddingRight 2`); MCP/plugin status is
//!   width-gated and empty in every pinned state (the fixture server
//!   has none) — the store carries no MCP/plugin state yet, so only
//!   the version renders;
//! * the whole footer hides below 44 cols or 12 rows, the logo below
//!   12 rows (`homeFooterVisibility` / `Logo`).

use crate::components::composer::render::{render_prompt_frame, FooterHints, PromptView};
use crate::components::composer::PromptDraft;
use crate::render::{Cell, CellColor, Frame};
use crate::theme::ThemeState;
use crate::wrap::display_width;
use oc_theme::Rgba;

/// The home placeholder rotation (`routes/home.tsx` `placeholder.normal`)
/// — the reference picks its entry at RANDOM (`randomIndex`); the port
/// pins index 0 (the exporter seeds `Math.random = () => 0` for the
/// same reason).
pub const PLACEHOLDERS: &[&str] =
    &["Fix a TODO in the codebase", "What is the tech stack of this project?", "Fix broken tests"];

/// The wordmark art (`logo.ts`) — 19-cell halves joined by a 1-cell
/// gap (`gap={1}`): `_` renders as a space over the shadow, `^` as `▀`
/// over the shadow, `~` as `▀` IN the shadow, `,` as `▄` in the shadow.
const LOGO_LEFT: [&str; 4] =
    ["                   ", "█▀▀█ █▀▀█ █▀▀█ █▀▀▄", "█__█ █__█ █^^^ █__█", "▀▀▀▀ █▀▀▀ ▀▀▀▀ ▀~~▀"];
const LOGO_RIGHT: [&str; 4] =
    ["             ▄     ", "█▀▀▀ █▀▀█ █▀▀█ █▀▀█", "█___ █__█ █__█ █^^^", "▀▀▀▀ ▀▀▀▀ ▀▀▀▀ ▀▀▀▀"];

/// The logo's full width: left 19 + gap 1 + right 19.
const LOGO_WIDTH: usize = 19 + 1 + 19;

/// The prompt box's `maxWidth` (`routes/home.tsx`).
const PROMPT_MAX_WIDTH: usize = 75;

/// The home render's inputs (the store-assembled form drives the
/// prompt draft + footer; the golden harness injects the fixture
/// location/version directly).
pub struct HomeView<'a> {
    pub draft: &'a PromptDraft,
    /// The rotated placeholder entry (`""` = none — an empty list).
    pub placeholder: String,
    /// The status row's `{directory}:{branch}` label.
    pub location: String,
    /// The app version (`home.footer`'s right-aligned label).
    pub version: String,
}

/// Render the home route at `size` (the golden-pinned layout).
#[must_use]
pub fn render_home_view(view: &HomeView, theme: &ThemeState, size: (u16, u16)) -> Frame {
    let (width, height) = (usize::from(size.0), usize::from(size.1));
    let tokens = &theme.resolved.tokens;
    let base = cell(tokens.background.base);
    let default_fg = CellColor::Rgb(255, 255, 255);
    let mut frame = Frame {
        cells: vec![vec![Cell { ch: ' ', attrs: 0, fg: default_fg, bg: base }; width]; height],
        width,
        height,
    };

    let padding = if width < 44 { 1 } else { 2 };
    let content = width.saturating_sub(padding * 2);
    // Centered offset (taffy rounds the half pixel up — golden-pinned).
    let centered = |item_width: usize| padding + content.saturating_sub(item_width).div_ceil(2);

    let show_logo = height >= 12;
    let show_footer = height >= 12 && width >= 44;
    // The prompt mount: paddingTop 1 + the composer's own height.
    let prompt_width = content.clamp(1, PROMPT_MAX_WIDTH);
    let mount =
        1 + crate::components::composer::render::mount_height(view.draft, prompt_width, height);
    let fixed = if show_logo { 3 + 4 + 1 } else { 0 } + mount;
    let free = height.saturating_sub(if show_footer { 3 } else { 0 } + fixed);
    let spacer_top = free.div_ceil(2);
    let mut y = spacer_top;
    if show_logo {
        y += 3; // the fixed 3-row spacer (`height={3}`)
        paint_logo(&mut frame, centered(LOGO_WIDTH), y, theme);
        y += 4 + 1; // logo + the 1-row spacer
    }
    // The prompt mount (the composer's own frame, pinned at `top` 1
    // for the wrapper's paddingTop).
    let prompt_view = PromptView {
        draft: view.draft,
        placeholder: view.placeholder.clone(),
        autocomplete: None,
        footer: FooterHints {
            location: view.location.clone(),
            agent_shortcut: "shift+tab".into(),
            command_shortcut: "ctrl+p".into(),
        },
        top: 1,
    };
    let prompt_frame = render_prompt_frame(&prompt_view, prompt_width, mount, theme);
    paste(&mut frame, &prompt_frame, centered(prompt_width), y);

    // The home.footer slot: pad · version · pad at h≥16, else one row.
    if show_footer {
        let padded = height >= 16;
        let row = if padded { height - 2 } else { height - 1 };
        let muted = cell(tokens.text.muted);
        let version_width = display_width(&view.version);
        let x = width.saturating_sub(padding + version_width);
        if version_width > 0 {
            for (index, ch) in view.version.chars().enumerate() {
                if let Some(slot) =
                    frame.cells.get_mut(row).and_then(|line| line.get_mut(x + index))
                {
                    *slot = Cell { ch, attrs: 0, fg: muted, bg: base };
                }
            }
        }
    }
    frame
}

/// One logo cell: `_`/`^` paint over the shadow, `~`/`,` paint IN the
/// shadow (`Logo.renderLine`); everything else is the half's color
/// over the base. The shadow = `tint(background.base, fg, 0.25)`
/// (theme/color.ts — 0.25·fg + 0.75·base per channel, rounded).
fn paint_logo(frame: &mut Frame, origin_x: usize, origin_y: usize, theme: &ThemeState) {
    let tokens = &theme.resolved.tokens;
    let base = cell(tokens.background.base);
    let base_rgb = base_rgb_as(base);
    let muted = cell(tokens.text.muted);
    let text_base = cell(tokens.text.base);
    let shadow = |fg: CellColor| -> CellColor {
        let (red, green, blue) = base_rgb_as(fg);
        CellColor::Rgb(
            tint_channel(red, base_rgb.0),
            tint_channel(green, base_rgb.1),
            tint_channel(blue, base_rgb.2),
        )
    };
    let shadow_muted = shadow(muted);
    let shadow_base = shadow(text_base);
    for (row, (left, right)) in LOGO_LEFT.iter().zip(&LOGO_RIGHT).enumerate() {
        paint_logo_half(frame, origin_x, origin_y + row, left, muted, shadow_muted, false, base);
        // The gap cell stays untouched (the `gap={1}` box paints
        // nothing — the golden pins the frame fill there).
        paint_logo_half(
            frame,
            origin_x + 20,
            origin_y + row,
            right,
            text_base,
            shadow_base,
            true,
            base,
        );
    }
}

fn base_rgb_as(color: CellColor) -> (u8, u8, u8) {
    match color {
        CellColor::Rgb(r, g, b) => (r, g, b),
        _ => (10, 10, 10),
    }
}

fn tint_channel(channel: u8, base: u8) -> u8 {
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    {
        (f32::from(channel) * 0.25 + f32::from(base) * 0.75).round() as u8
    }
}

#[allow(clippy::too_many_arguments)]
fn paint_logo_half(
    frame: &mut Frame,
    x: usize,
    y: usize,
    line: &str,
    fg: CellColor,
    shadow: CellColor,
    bold: bool,
    base: CellColor,
) {
    let attrs = u16::from(bold);
    for (index, ch) in line.chars().enumerate() {
        let cell = match ch {
            '_' => Cell { ch: ' ', attrs, fg, bg: shadow },
            '^' => Cell { ch: '▀', attrs, fg, bg: shadow },
            '~' => Cell { ch: '▀', attrs, fg: shadow, bg: base },
            ',' => Cell { ch: '▄', attrs, fg: shadow, bg: base },
            other => Cell { ch: other, attrs, fg, bg: base },
        };
        if let Some(slot) = frame.cells.get_mut(y).and_then(|row| row.get_mut(x + index)) {
            *slot = cell;
        }
    }
}

fn cell(color: Rgba) -> CellColor {
    CellColor::Rgb(color.r, color.g, color.b)
}

/// Paste `region`'s cells at (`x`, `y`); anything left keeps the fill.
fn paste(target: &mut Frame, region: &Frame, x: usize, y: usize) {
    for (row, cells) in region.cells.iter().enumerate() {
        let Some(slot) = target.cells.get_mut(y + row) else { break };
        let count = cells.len().min(slot.len().saturating_sub(x));
        slot[x..x + count].clone_from_slice(&cells[..count]);
    }
}

/// Render the home route over the store: the home prompt's draft
/// (`HOME_PROMPT_KEY`'s composer, placeholder rotation pinned to entry
/// 0) and the crate version in the footer slot. The location label is
/// the pump's seam (unset until the server wires it — the footer then
/// renders the label muted like the session route).
#[must_use]
pub fn render_home(store: &Store, theme: &crate::theme::ThemeStyles, size: (u16, u16)) -> Frame {
    let state = store.theme();
    let composer = store.composer(crate::app::HOME_PROMPT_KEY);
    let placeholder = if composer.placeholders.is_empty() {
        format!(
            "Ask anything… \"{}\"",
            PLACEHOLDERS[composer.placeholder_index % PLACEHOLDERS.len()]
        )
    } else {
        composer.placeholder_text()
    };
    let view = HomeView {
        draft: &composer.draft,
        placeholder,
        location: String::new(),
        version: env!("CARGO_PKG_VERSION").to_owned(),
    };
    let _ = theme;
    render_home_view(&view, &state, size)
}

use crate::store::Store;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::reactive::Runtime;

    #[test]
    fn home_renders_the_logo_and_hint() {
        let store = Store::new(Runtime::new());
        let theme = crate::theme::ThemeState::v2_dark();
        let frame = render_home(&store, &theme.styles(), (40, 12));
        let grid = frame.text_grid().join("\n");
        assert!(grid.contains("Ask anything"), "{grid}");
    }

    /// The data-driven mount: the store's home draft types into the
    /// composer and the mount grows.
    #[test]
    fn home_prompt_reads_the_store_draft() {
        let store = Store::new(Runtime::new());
        let mut composer = store.composer(crate::app::HOME_PROMPT_KEY);
        composer.draft.text = "hello".into();
        store.set_composer(crate::app::HOME_PROMPT_KEY, &composer);
        let theme = crate::theme::ThemeState::v2_dark();
        let frame = render_home(&store, &theme.styles(), (80, 30));
        let grid = frame.text_grid().join("\n");
        assert!(grid.contains("hello"), "the draft typed into the home prompt: {grid}");
    }
}

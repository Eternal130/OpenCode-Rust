//! The session route: the transcript pane (lane C's renderer) with
//! the composer docked underneath (`routes/session/index.tsx` +
//! `component/session-frame.tsx` shape), replacing the wave-1 chrome
//! stub. The composer is the wave-2 component
//! ([`crate::components::composer`]) — golden-exact on its own, docked
//! here over the store's per-session draft.
//!
//! Footer seam: the composer's status row ([`FooterHints`]) is the
//! prompt-idle integration point — lane B's
//! [`crate::components::footer::prompt_idle`] composes the same
//! `{directory}:{branch}` + keybind-hints inputs when its panel mounts
//! over the composer.

use crate::components::composer::render::{render_prompt_frame, FooterHints, PromptView};
use crate::components::message::list::{render_transcript, TranscriptOptions};
use crate::render::{paint_placed_with_fills, Frame};
use crate::store::Store;
use ratatui::style::Style;

/// Render the session route: the title row, the transcript (lane C's
/// renderer over the store's messages), and the composer docked at
/// the bottom with one blank spacing row above it (the session
/// column's gap).
#[must_use]
pub fn render_session(
    store: &Store,
    session_id: &str,
    styles: &crate::theme::ThemeStyles,
    size: (u16, u16),
) -> Frame {
    // The composer's renderer consumes the resolved token tree
    // (decrease/action surfaces); the store carries the live theme.
    let theme = store.theme();
    let (width, height) = (usize::from(size.0), usize::from(size.1));
    let tabs = store.tabs();
    let title = tabs
        .tabs
        .iter()
        .find(|tab| tab.session_id == session_id)
        .and_then(|tab| tab.title.clone())
        .unwrap_or_else(|| "Untitled session".to_owned());
    let fill =
        Style::new().fg(crate::components::tool_rows::DEFAULT_FOREGROUND).bg(styles.background);

    // The composer's mount frame (its own height grows with the
    // draft); dock it at the bottom, transcript above.
    let composer = store.composer(session_id);
    let view = PromptView {
        draft: &composer.draft,
        placeholder: composer.placeholder_text(),
        autocomplete: None,
        footer: FooterHints::default(),
        top: 0,
    };
    let mount_height =
        crate::components::composer::render::mount_height(&composer.draft, width, height)
            .min(height);
    let mount = render_prompt_frame(&view, width, mount_height, &theme);
    let spacing = usize::from(mount_height + 1 < height);
    let transcript_height = height.saturating_sub(mount_height + spacing + 1);

    let transcript = render_transcript(
        &store.messages(session_id),
        &TranscriptOptions::default(),
        styles,
        width,
    );
    let mut placed = Vec::new();
    let mut fills = Vec::new();
    // Title row (muted, inset like the transcript).
    let inset = if width < 44 { 1 } else { 2 };
    let title_width = crate::wrap::display_width(&title).min(width.saturating_sub(inset * 2));
    placed.push(crate::layout::Placed {
        x: inset,
        y: 0,
        width: title_width,
        lines: vec![crate::layout::PlacedLine::plain(
            crate::wrap::take_width(&title, width.saturating_sub(inset * 2)),
            styles.text_muted,
        )],
        tag: None,
    });
    for row in &transcript.rows {
        let y = 1 + row.y;
        if y >= transcript_height {
            break;
        }
        for line in &row.placed {
            let mut shifted = line.clone();
            shifted.y += 1;
            placed.push(shifted);
        }
        fills.extend(row.fills.iter().map(|fill| crate::render::Fill {
            x: fill.x,
            y: fill.y + 1,
            width: fill.width,
            height: fill.height,
            style: fill.style,
        }));
    }
    let mut frame = paint_placed_with_fills(&placed, &fills, size, fill);
    paste(&mut frame, &mount, 0, height - mount_height);
    frame
}

/// Paste `region`'s cells at (`x`, `y`); anything left keeps the fill.
fn paste(target: &mut Frame, region: &Frame, x: usize, y: usize) {
    for (row, cells) in region.cells.iter().enumerate() {
        let Some(slot) = target.cells.get_mut(y + row) else { break };
        let count = cells.len().min(slot.len().saturating_sub(x));
        slot[x..x + count].clone_from_slice(&cells[..count]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::reactive::Runtime;

    #[test]
    fn session_frame_docks_the_composer_under_the_transcript() {
        let store = Store::new(Runtime::new());
        store.open_tab("s1", Some("Bug hunt".to_owned()));
        let theme = crate::theme::ThemeState::v2_dark();
        let frame = render_session(&store, "s1", &theme.styles(), (40, 12));
        let grid = frame.text_grid().join("\n");
        assert!(grid.contains("Bug hunt"), "title present: {grid}");
        // The docked composer's chrome: left rail + bottom transition
        // on the LAST rows (docked, not floating).
        assert!(grid.contains('┃'), "composer rail present: {grid}");
        assert!(grid.contains('╹'), "composer bottom corner present: {grid}");
        let docked: Vec<String> = frame
            .text_grid()
            .into_iter()
            .rev()
            .take(3)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect();
        assert!(
            docked.iter().any(|row| row.contains('╹')),
            "composer docked at the bottom: {grid}"
        );
    }

    #[test]
    fn submit_prompt_records_history_and_clears_the_draft() {
        let store = Store::new(Runtime::new());
        let mut composer = store.composer("s1");
        composer.draft.text = "hello".into();
        store.set_composer("s1", &composer);
        let submitted = store.submit_prompt("s1").expect("submitted");
        assert_eq!(submitted.text, "hello");
        assert!(store.prompt_draft("s1").is_empty(), "draft cleared");
        assert_eq!(store.composer("s1").history.entries, vec!["hello".to_owned()]);
    }
}

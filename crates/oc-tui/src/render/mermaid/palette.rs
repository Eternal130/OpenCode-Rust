//! `palette.ts` — the OpenCode diagram palette
//! (`createOpenCodeDiagramPalette` / `resolveOpenCodeDiagramPalette`).

use crate::render::mermaid::core::color::{blend_color, Rgba};

/// `OpenCodeDiagramPaletteInput` (the theme-derived subset).
#[derive(Debug, Clone, Copy)]
pub struct OpenCodeDiagramPaletteInput {
    pub text: Rgba,
    pub subdued: Rgba,
    pub info: Rgba,
    pub success: Rgba,
    pub warning: Rgba,
    pub background: Rgba,
    pub accent_soft: Rgba,
    pub accent_clear: Rgba,
}

/// The resolved palette — the `colors` record the mermaid markdown
/// renderer feeds each diagram style resolver.
#[derive(Debug, Clone, Copy)]
pub struct OpenCodeDiagramPalette {
    pub text: Rgba,
    pub primary: Rgba,
    pub secondary: Rgba,
    pub muted: Rgba,
    pub warning: Rgba,
    pub background: Rgba,
    pub request: Rgba,
    pub response: Rgba,
    pub note: Rgba,
    pub note_background: Rgba,
    pub box_text: Rgba,
    pub box_border: Rgba,
    pub line: Rgba,
    pub label_background: Rgba,
    pub group: Rgba,
    pub group_text: Rgba,
    pub marker: Rgba,
    pub note_border: Rgba,
    pub note_text: Rgba,
    pub note_connector: Rgba,
}

/// `createOpenCodeDiagramPalette`.
#[must_use]
pub fn create_open_code_diagram_palette(
    input: &OpenCodeDiagramPaletteInput,
) -> OpenCodeDiagramPalette {
    let secondary = blend_color(Some(input.text), Some(input.subdued), 0.5).unwrap_or(input.text);
    let muted = blend_color(Some(input.text), Some(input.subdued), 0.7).unwrap_or(input.text);
    OpenCodeDiagramPalette {
        text: input.text,
        primary: input.text,
        secondary,
        muted,
        warning: input.info,
        background: input.background,
        request: input.success,
        response: input.warning,
        note: input.text,
        note_background: blend_color(Some(input.background), Some(input.subdued), 0.25)
            .unwrap_or(input.background),
        box_text: input.text,
        box_border: muted,
        line: input.subdued,
        label_background: Rgba::from_ints(input.text.r, input.text.g, input.text.b, 20),
        group: input.accent_soft,
        group_text: input.accent_soft,
        marker: secondary,
        note_border: input.accent_soft,
        note_text: input.accent_clear,
        note_connector: input.accent_soft,
    }
}

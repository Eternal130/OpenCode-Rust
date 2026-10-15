//! The opentui-core EDITOR adapter (lane M-engine): the composer's
//! editing engine over the REAL `@opentui/core` textarea line
//! (`opentui-core`'s `EditBuffer`/`EditorView`, the proven zero-diff
//! port of edit-buffer.zig/editor-view.zig), selected by
//! `OC_ENGINE_OPENTUI`. The hand-rolled [`crate::text_edit::TextArea`]
//! stays the `=0` fallback for one release.
//!
//! Phase 2 of the lane — see `docs/mengine-rig-ab.log`.

pub struct TextAreaOpentui {
    _reserved: (),
}

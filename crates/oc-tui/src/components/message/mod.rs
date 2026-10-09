//! Message flow (7.2 lane C): the session transcript —
//!
//! * [`model`] — the message data model + the grouping projection
//!   (`grouping/{tree,session}.ts`, `reduceSessionRows`) and the
//!   label/duration derivations the headers paint;
//! * [`markdown`] — the markdown TEXT renderer (C1) as `TextPart`
//!   drives it (`@opentui/core` `MarkdownRenderable` text behavior);
//! * [`rows`] — the row widgets (`message-parts.tsx`): the running
//!   spinner row (`ManualClock`), the completed exploration header, the
//!   reasoning header, the user-message block, the assistant footer;
//! * [`list`] — the non-virtualized transcript with the wave-2 window
//!   seam (`slice_at`), the follow/jump state-machine sketch and the
//!   golden fixtures.

pub mod list;
pub mod markdown;
pub mod model;
pub mod rows;

pub use list::{
    render_transcript, resolve_part, vscroll_appended_turn, vscroll_messages_fixture, FollowState,
    MeasuredRow, ThinkingMode, Transcript, TranscriptOptions,
};
pub use markdown::{parse_blocks, parse_inline, render_markdown, MarkdownLine, MarkdownStyles};
pub use model::{
    canonical_tool_name, exploration_label, locale_duration, reasoning_summary, reduce_rows,
    titlecase, turn_duration, Entry, GroupKind, Message, Part, PartTime, Row,
};
pub use rows::{
    assistant_footer_row, completed_tool_row, reasoning_header_row, running_tool_row,
    spinner_frame, spinner_frozen_frame, user_message_row, RowRender, SPINNER_FRAMES,
    SPINNER_INTERVAL_MS,
};

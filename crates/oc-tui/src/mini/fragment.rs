//! The message-part fragment reconciler — port of
//! `packages/tui/src/mini/stream-v2.fragment.ts:1-83`.
//!
//! One entry per `(messageID, partID)` part fragment carries TWO
//! strings: the part's committed `text` (the append-only
//! accumulation) and the yet-unseen `projected` tail (what a
//! projection showed but streaming has not yet delivered). That
//! second string is what makes part RE-PROJECTION safe: when a
//! message render (hydrate, replay, a resize replay) re-projects a
//! part's full text, `project` only advances the committed prefix;
//! when the live stream re-delivers a chunk the projection already
//! showed, `delta` consumes it from anywhere in the remaining
//! projection instead of double-appending; and when the local rows
//! must be re-admitted after a reset, `restore` classifies each row
//! against the server's truth as `append` (a suffix the server has
//! not shown yet), `covered` (the server already has this text —
//! drop the row), or `conflict` (neither is a prefix of the other —
//! keep the row as-is).

use std::collections::HashMap;

/// `FragmentRef` — one `(messageID, partID)` part fragment.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct FragmentRef {
    pub message_id: String,
    pub part_id: String,
}

/// `fragmentRef(messageID, kind, ordinal)`: the streaming parts'
/// `partID` form (`text:0`, `reasoning:1`).
#[must_use]
pub fn fragment_ref(message_id: &str, kind: &str, ordinal: usize) -> FragmentRef {
    FragmentRef { message_id: message_id.to_owned(), part_id: format!("{kind}:{ordinal}") }
}

/// The map key (`messageID \0 partID`).
fn key(fragment: &FragmentRef) -> String {
    format!("{}\u{0}{}", fragment.message_id, fragment.part_id)
}

/// `FragmentUpdate`: the state change a projection/delta/end caused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FragmentUpdate {
    pub key: String,
    /// The part's text BEFORE the update ("" when first seen).
    pub previous: String,
    /// The part's text AFTER the update.
    pub text: String,
}

/// `FragmentRestore` — how a local row re-admits after a reset.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FragmentRestore {
    /// The server text EXTENDS the local row: replay only the suffix.
    Append { suffix: String },
    /// The server text is a PREFIX of (or equal to) the local row:
    /// the server already showed it — drop the row.
    Covered,
    /// Neither is a prefix of the other: keep the row verbatim.
    Conflict,
}

/// `createFragmentReconciler()` — the per-transport fragment table.
#[derive(Debug, Default)]
pub struct FragmentReconciler {
    fragments: HashMap<String, FragmentState>,
}

#[derive(Debug, Clone, Default)]
struct FragmentState {
    text: String,
    /// The yet-undelivered tail of the last projection (`None` =
    /// never projected).
    projected: Option<String>,
}

impl FragmentReconciler {
    /// `clear()`.
    pub fn clear(&mut self) {
        self.fragments.clear();
    }

    /// `value(fragment)`: the part's committed text.
    #[must_use]
    pub fn value(&self, fragment: &FragmentRef) -> Option<&str> {
        self.fragments.get(&key(fragment)).map(|state| state.text.as_str())
    }

    /// `project(fragment, text, visible)`: re-project a part's FULL
    /// text (a message render). The committed text snaps to `text`;
    /// the projected tail records what a VISIBLE projection showed
    /// beyond the committed prefix (an invisible render keeps the
    /// previous tail).
    pub fn project(&mut self, fragment: &FragmentRef, text: &str, visible: bool) -> FragmentUpdate {
        let id = key(fragment);
        let previous = self.fragments.get(&id).map(|state| state.text.clone()).unwrap_or_default();
        let projected = if visible {
            Some(text.to_owned())
        } else {
            self.fragments.get(&id).and_then(|state| state.projected.clone())
        };
        self.fragments.insert(id, FragmentState { text: text.to_owned(), projected });
        FragmentUpdate { key: key(fragment), previous, text: text.to_owned() }
    }

    /// `delta(fragment, delta)`: one streamed chunk. `None` = the
    /// chunk was CONSUMED by the projection (a covered tail — the
    /// projection already showed it, so nothing changes); `Some` =
    /// the committed text advanced.
    pub fn delta(&mut self, fragment: &FragmentRef, delta: &str) -> Option<FragmentUpdate> {
        let id = key(fragment);
        // Replay may start after an unseen prefix, so consume a
        // covered chunk from anywhere in the remaining projection
        // rather than only its start.
        if let Some(state) = self.fragments.get_mut(&id) {
            if let Some(projected) = state.projected.as_mut() {
                if let Some(covered) = projected.find(delta) {
                    // Only a NON-empty delta consumes (an empty chunk
                    // would match at 0 and no-op — the ref's indexOf("")
                    // is 0, slice(0) is identity, and it returns
                    // undefined WITHOUT advancing the text: same net).
                    if delta.is_empty() {
                        return None;
                    }
                    let skip = covered + delta.len();
                    *projected = projected[skip..].to_owned();
                    return None;
                }
            }
        }
        let previous = self.fragments.get(&id).map(|state| state.text.clone()).unwrap_or_default();
        let mut text = previous.clone();
        text.push_str(delta);
        let projected = self.fragments.get(&id).and_then(|state| state.projected.clone());
        self.fragments.insert(id.clone(), FragmentState { text: text.clone(), projected });
        Some(FragmentUpdate { key: id, previous, text })
    }

    /// `end(fragment, text)`: the part closed — the committed text
    /// becomes the final text verbatim.
    pub fn end(&mut self, fragment: &FragmentRef, text: &str) -> FragmentUpdate {
        let id = key(fragment);
        let previous = self.fragments.get(&id).map(|state| state.text.clone()).unwrap_or_default();
        self.fragments.insert(id.clone(), FragmentState { text: text.to_owned(), projected: None });
        FragmentUpdate { key: id, previous, text: text.to_owned() }
    }

    /// `restore(fragment, text)`: re-admit a LOCAL row after a reset
    /// against the server's `text` for the part.
    pub fn restore(&mut self, fragment: &FragmentRef, text: &str) -> FragmentRestore {
        let id = key(fragment);
        let Some(current) = self.fragments.get(&id) else {
            self.fragments.insert(
                id,
                FragmentState { text: text.to_owned(), projected: Some(text.to_owned()) },
            );
            return FragmentRestore::Append { suffix: text.to_owned() };
        };
        if text.starts_with(current.text.as_str()) {
            let suffix = text[current.text.len()..].to_owned();
            self.fragments.insert(
                id,
                FragmentState { text: text.to_owned(), projected: Some(text.to_owned()) },
            );
            return FragmentRestore::Append { suffix };
        }
        if current.text.starts_with(text) {
            return FragmentRestore::Covered;
        }
        FragmentRestore::Conflict
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frag() -> FragmentRef {
        fragment_ref("msg_1", "text", 0)
    }

    /// Fresh parts append; re-projection advances only past the
    /// committed prefix; the projected tail rides the visible flag.
    #[test]
    fn project_tracks_the_committed_prefix_and_visible_tail() {
        let mut reconciler = FragmentReconciler::default();
        let update = reconciler.project(&frag(), "Hello world", true);
        assert_eq!(update.previous, "");
        assert_eq!(update.text, "Hello world");
        assert_eq!(reconciler.value(&frag()), Some("Hello world"));
        // An INVISIBLE re-projection (hydrate with render=false)
        // keeps the committed text and never widens the tail.
        let update = reconciler.project(&frag(), "Hello world again", false);
        assert_eq!(update.previous, "Hello world");
        assert_eq!(update.text, "Hello world again");
    }

    /// The delta path: a plain chunk appends; a chunk the projection
    /// already showed is consumed from ANYWHERE in the remaining
    /// projection (no double-append, no commit).
    #[test]
    fn delta_appends_or_consumes_the_covered_chunk() {
        let mut reconciler = FragmentReconciler::default();
        // Project first (hydrate rendered the part), then stream.
        reconciler.project(&frag(), "one two three", true);
        // The chunk "two " sits mid-projection: consumed.
        assert_eq!(reconciler.delta(&frag(), "two "), None);
        assert_eq!(reconciler.value(&frag()), Some("one two three"));
        // ...so the projection now only holds the tail past it.
        assert_eq!(reconciler.delta(&frag(), "three"), None);
        // A chunk past the projection appends.
        let update = reconciler.delta(&frag(), " four").expect("appends");
        assert_eq!(update.previous, "one two three");
        assert_eq!(update.text, "one two three four");
        // Without any projection, every delta appends (the plain
        // streaming path).
        let mut plain = FragmentReconciler::default();
        plain.delta(&frag(), "a");
        let update = plain.delta(&frag(), "b").expect("appends");
        assert_eq!(update.text, "ab");
    }

    /// `end` snaps the committed text to the final text verbatim.
    #[test]
    fn end_replaces_with_the_final_text() {
        let mut reconciler = FragmentReconciler::default();
        reconciler.delta(&frag(), "partial");
        let update = reconciler.end(&frag(), "partial but final");
        assert_eq!(update.previous, "partial");
        assert_eq!(update.text, "partial but final");
        assert_eq!(reconciler.value(&frag()), Some("partial but final"));
    }

    /// `restore`'s three modes: unseen (append whole), extended
    /// (append suffix), covered (drop), conflict (keep).
    #[test]
    fn restore_classifies_local_rows_against_the_server() {
        let mut reconciler = FragmentReconciler::default();
        // Unseen part: the whole row replays.
        assert_eq!(
            reconciler.restore(&frag(), "abc"),
            FragmentRestore::Append { suffix: "abc".to_owned() }
        );
        // Server extends the row: only the suffix replays.
        assert_eq!(
            reconciler.restore(&frag(), "abcdef"),
            FragmentRestore::Append { suffix: "def".to_owned() }
        );
        // Server already covered the row: drop it.
        assert_eq!(reconciler.restore(&frag(), "abc"), FragmentRestore::Covered);
        assert_eq!(reconciler.restore(&frag(), "abcd"), FragmentRestore::Covered);
        // Neither a prefix: keep the row verbatim.
        assert_eq!(reconciler.restore(&frag(), "zzz"), FragmentRestore::Conflict);
    }
}

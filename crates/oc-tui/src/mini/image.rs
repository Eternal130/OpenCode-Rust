//! The mini image commits — port of
//! `packages/tui/src/mini/stream-v2.image.ts:1-20` (+ the transport's
//! `freshImages` dedup, stream-v2.transport.ts:592-599).
//!
//! Image-bearing parts commit as FINAL rows carrying the image's
//! data URI and a display label (`{name}` / `[Image N]`); repeats are
//! deduped per `(messageID, partID)` so a replayed projection or a
//! re-delivered part never re-renders.

use serde_json::Value;

use super::fragment::FragmentRef;
use super::scrollback::{Commit, CommitKind, Phase};

/// `userImageCommits`: one commit per VISIBLE image file of a user
/// message (`deduplicateVisibleImages` + the `image/*` filter),
/// `partID = image:{index}`.
#[must_use]
pub fn user_image_commits(message_id: &str, files: &[Value]) -> Vec<Commit> {
    let mut seen_payloads: Vec<&str> = Vec::new();
    let mut commits = Vec::new();
    let mut index = 0usize;
    for file in files {
        let text = |key: &str| file.get(key).and_then(Value::as_str).unwrap_or_default();
        let mime = text("mime");
        if !mime.starts_with("image/") {
            continue;
        }
        let data = text("data");
        // `deduplicateVisibleImages`: identical payloads render once.
        if seen_payloads.contains(&data) {
            continue;
        }
        seen_payloads.push(data);
        let name = text("name");
        let mention = file
            .get("mention")
            .and_then(|mention| mention.get("text"))
            .and_then(Value::as_str)
            .unwrap_or_default();
        let label = if !name.is_empty() {
            name.to_owned()
        } else if !mention.is_empty() {
            mention.to_owned()
        } else {
            format!("[Image {}]", index + 1)
        };
        commits.push(image_commit(
            message_id,
            &format!("image:{index}"),
            &label,
            &format!("data:{mime};base64,{data}"),
        ));
        index += 1;
    }
    commits
}

/// `toolImageCommits`: one commit per `data:image/*` file in a tool
/// part's display content, `partID = {partID}:image:{index}`.
#[must_use]
pub fn tool_image_commits(part_id: &str, part_state: &Value, message_id: &str) -> Vec<Commit> {
    let content = part_state.get("content").and_then(Value::as_array);
    let mut commits = Vec::new();
    let mut index = 0usize;
    for item in content.into_iter().flatten() {
        let text = |key: &str| item.get(key).and_then(Value::as_str).unwrap_or_default();
        if text("type") != "file" || !text("mime").starts_with("image/") {
            continue;
        }
        let uri = text("uri");
        if !uri.starts_with("data:image/") {
            continue;
        }
        let name = text("name");
        let label =
            if name.is_empty() { format!("[Image {}]", index + 1) } else { name.to_owned() };
        commits.push(image_commit(message_id, &format!("{part_id}:image:{index}"), &label, uri));
        index += 1;
    }
    commits
}

/// The shared `ImageCommit` shape (`kind`/`source`/`phase: "final"`
/// + the image URI).
fn image_commit(message_id: &str, part_id: &str, label: &str, image: &str) -> Commit {
    Commit {
        kind: CommitKind::User,
        text: label.to_owned(),
        phase: Phase::Final,
        message_id: Some(message_id.to_owned()),
        part_id: Some(part_id.to_owned()),
        image: Some(image.to_owned()),
        ..Commit::default()
    }
}

/// `freshImages(commits)`: the dedup filter — a `(messageID, partID)`
/// pair commits at most once per transport (`state.imageIDs`).
#[must_use]
pub fn fresh_images(
    image_ids: &mut std::collections::HashSet<String>,
    commits: Vec<Commit>,
) -> Vec<Commit> {
    commits
        .into_iter()
        .filter(|commit| {
            let Some(message_id) = &commit.message_id else { return false };
            let Some(part_id) = &commit.part_id else { return false };
            if commit.image.is_none() {
                return false;
            }
            let key = format!("{message_id}\u{0}{part_id}");
            image_ids.insert(key)
        })
        .collect()
}

/// The dedup key of one image commit (`streamPartKey`).
#[must_use]
pub fn image_key(fragment: &FragmentRef) -> String {
    format!("{}\u{0}{}", fragment.message_id, fragment.part_id)
}

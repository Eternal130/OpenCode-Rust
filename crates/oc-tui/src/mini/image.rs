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

/// `deduplicateVisibleImages`'s identity (attachment.ts:65-74): an
/// INLINE image file WITH mention text keys on
/// `JSON.stringify([mime, name, description, mention.text])` +
/// payload — identical metadata+payload renders once. `None` = the
/// file never dedups (the caller keeps it; the mime filter runs
/// AFTER, per the ref's `dedupe(…).filter(image/*)` order).
fn image_identity(file: &Value) -> Option<(String, String)> {
    let text = |key: &str| file.get(key).and_then(Value::as_str).unwrap_or_default();
    let mime = text("mime");
    let mention = file
        .get("mention")
        .and_then(|mention| mention.get("text"))
        .and_then(Value::as_str)
        .unwrap_or_default();
    if !mime.starts_with("image/") || mention.is_empty() {
        return None;
    }
    let opt =
        |value: &str| (!value.is_empty()).then(|| serde_json::Value::String(value.to_owned()));
    let name = text("name");
    let description = text("description");
    let metadata = serde_json::Value::Array(vec![
        serde_json::Value::String(mime.to_owned()),
        opt(&name).unwrap_or(serde_json::Value::Null),
        opt(&description).unwrap_or(serde_json::Value::Null),
        serde_json::Value::String(mention.to_owned()),
    ])
    .to_string();
    Some((metadata, text("data").to_owned()))
}

/// `userImageCommits`: one commit per VISIBLE image file of a user
/// message — `deduplicateVisibleImages(files).filter(image/*)`
/// (stream-v2.image.ts:9-19: the dedup runs BEFORE the mime filter;
/// only the SURVIVORS number the `[Image N]` labels),
/// `partID = image:{index}`.
#[must_use]
pub fn user_image_commits(message_id: &str, files: &[Value]) -> Vec<Commit> {
    // `deduplicateByIdentity` (attachment.ts:24-38): metadata → the
    // payloads seen; a repeat metadata+payload drops.
    let mut seen: std::collections::HashMap<String, std::collections::HashSet<String>> =
        std::collections::HashMap::new();
    let mut commits = Vec::new();
    let mut index = 0usize;
    for file in files {
        if let Some((metadata, payload)) = image_identity(file) {
            let payloads = seen.entry(metadata).or_default();
            if payloads.contains(&payload) {
                continue;
            }
            payloads.insert(payload);
        }
        let text = |key: &str| file.get(key).and_then(Value::as_str).unwrap_or_default();
        let mime = text("mime");
        if !mime.starts_with("image/") {
            continue;
        }
        let data = text("data");
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
            CommitKind::User,
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
        commits.push(image_commit(
            message_id,
            &format!("{part_id}:image:{index}"),
            &label,
            uri,
            CommitKind::Tool,
        ));
        index += 1;
    }
    commits
}

/// The shared `ImageCommit` shape (`phase: "final"` + the image URI;
/// `kind` = the ref's source row — "user" for user files, "tool" for
/// tool display content, stream-v2.image.ts:11/22).
fn image_commit(
    message_id: &str,
    part_id: &str,
    label: &str,
    image: &str,
    kind: CommitKind,
) -> Commit {
    Commit {
        kind,
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

//! text/event-stream subscription: parse `data:` frames and heartbeat
//! comments off `GET /api/event`, collect frames (count-based, or the
//! scenario policy: N frames / quiescence), normalize the collected
//! sequence, and compare two sequences per the canonical grouping policy
//! (`NORMALIZATION.md` §"Sequence comparison").

use std::time::Duration;

use anyhow::{anyhow, Context, Result};
use serde_json::Value;

use crate::minih::{self, BodyStream};
use crate::normalize::{normalize_value_with, IdNumbering, NormEnv};

/// Default scenario quiescence window: a sequence is considered complete
/// when no data frame arrived for this long (spec §"Sequence comparison").
pub const DEFAULT_QUIESCENCE: Duration = Duration::from_millis(2000);

/// One parsed SSE frame. Comment-only frames (heartbeats) have empty `data`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Frame {
    /// `event:` field, when present.
    pub event: Option<String>,
    /// Joined `data:` lines (`\n` between multiples).
    pub data: String,
    /// `:` comment lines seen since the previous dispatch.
    pub comments: Vec<String>,
}

impl Frame {
    /// Whether this frame carries a payload (as opposed to a heartbeat).
    #[must_use]
    pub fn is_data(&self) -> bool {
        !self.data.is_empty()
    }
}

/// Incremental SSE line parser (one `feed_line` per line; blank line
/// dispatches). Kept separate from IO so it is unit-testable.
#[derive(Default)]
pub(crate) struct Parser {
    event: Option<String>,
    data_lines: Vec<String>,
    comments: Vec<String>,
}

impl Parser {
    /// Feed one line (without its terminator). Returns a frame when a blank
    /// line dispatches one.
    pub(crate) fn feed_line(&mut self, line: &str) -> Option<Frame> {
        if line.is_empty() {
            let frame = Frame {
                event: self.event.take(),
                data: std::mem::take(&mut self.data_lines).join("\n"),
                comments: std::mem::take(&mut self.comments),
            };
            return Some(frame);
        }
        if let Some(comment) = line.strip_prefix(':') {
            self.comments.push(comment.trim().to_string());
            return None;
        }
        let (field, value) = line.split_once(':').unwrap_or((line, ""));
        // SSE strips a single leading space after the colon.
        let value = value.strip_prefix(' ').unwrap_or(value);
        match field {
            "event" => self.event = Some(value.to_string()),
            "data" => self.data_lines.push(value.to_string()),
            _ => {} // id:/retry:/unknown fields are not needed for parity
        }
        None
    }
}

/// A live subscription to an SSE endpoint.
pub struct SseStream {
    body: BodyStream,
    parser: Parser,
    pending: Vec<u8>,
    eof: bool,
}

impl SseStream {
    /// Subscribe to `path` (normally `/api/event`) with Basic auth.
    /// Fails on non-200 or non-event-stream responses.
    pub async fn subscribe(
        base: &str,
        password: &str,
        path: &str,
        timeout: Duration,
    ) -> Result<Self> {
        let auth = minih::basic_auth("opencode", password);
        let (status, headers, body) =
            minih::stream_request(base, Some(&auth), "text/event-stream", path, timeout).await?;
        if status != 200 {
            return Err(anyhow!("SSE subscribe to {path} returned HTTP {status}"));
        }
        let content_type = headers
            .iter()
            .find(|(name, _)| name == "content-type")
            .map(|(_, value)| value.clone())
            .unwrap_or_default();
        if !content_type.starts_with("text/event-stream") {
            return Err(anyhow!("SSE subscribe to {path} returned content-type {content_type}"));
        }
        Ok(Self { body, parser: Parser::default(), pending: Vec::new(), eof: false })
    }

    /// Pull one complete line from the wire (or the flush remainder on EOF).
    async fn next_line(&mut self, timeout: Duration) -> Result<Option<String>> {
        loop {
            if let Some(pos) = self.pending.iter().position(|&b| b == b'\n') {
                let line: Vec<u8> = self.pending.drain(..=pos).collect();
                let mut text = String::from_utf8_lossy(&line).into_owned();
                text.pop(); // '\n'
                if text.ends_with('\r') {
                    text.pop();
                }
                return Ok(Some(text));
            }
            if self.eof {
                if self.pending.is_empty() {
                    return Ok(None);
                }
                let text = String::from_utf8_lossy(&self.pending).into_owned();
                self.pending.clear();
                return Ok(Some(text));
            }
            match self.body.next_bytes(timeout).await? {
                Some(bytes) => self.pending.extend_from_slice(&bytes),
                None => self.eof = true,
            }
        }
    }

    /// Await the next frame. `idle_timeout` bounds the wait for any single
    /// wire read, so a silent server surfaces as an error instead of a hang.
    pub async fn next_frame(&mut self, idle_timeout: Duration) -> Result<Frame> {
        loop {
            let dispatched = match self.next_line(idle_timeout).await? {
                Some(line) => self.parser.feed_line(&line),
                None => {
                    // Flush a trailing partial frame, then fail: the stream
                    // ended with nothing more to dispatch.
                    self.parser.feed_line("")
                }
            };
            if let Some(frame) = dispatched {
                return Ok(frame);
            }
            if self.eof && self.pending.is_empty() {
                return Err(anyhow!("SSE stream ended"));
            }
        }
    }

    /// Collect `count` data frames (heartbeats are skipped) within
    /// `overall_timeout`.
    pub async fn collect(&mut self, count: usize, overall_timeout: Duration) -> Result<Vec<Frame>> {
        let deadline = tokio::time::Instant::now() + overall_timeout;
        let mut frames = Vec::with_capacity(count);
        while frames.len() < count {
            let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
            if remaining.is_zero() {
                return Err(anyhow!(
                    "timed out after {overall_timeout:?} with {}/{} SSE frames",
                    frames.len(),
                    count
                ));
            }
            let frame = self.next_frame(remaining).await.context("collecting SSE frames")?;
            if frame.is_data() {
                frames.push(frame);
            }
        }
        Ok(frames)
    }

    /// Scenario alignment collection (spec §"Sequence comparison" policy b):
    /// collect data frames until one of
    ///
    /// 1. `max_frames` data frames were seen (scenario watermark), or
    /// 2. **quiescence** — no *data* frame for `quiescence` (heartbeats are
    ///    transport keep-alive and do not reset the window), or
    /// 3. `overall_timeout` elapsed,
    ///
    /// then return whatever arrived (possibly fewer than `max_frames`).
    /// Both sides of a scenario comparison use the same parameters.
    pub async fn collect_scenario(
        &mut self,
        max_frames: usize,
        quiescence: Duration,
        overall_timeout: Duration,
    ) -> Result<Vec<Frame>> {
        let overall_at = tokio::time::Instant::now() + overall_timeout;
        let mut quiet_at = tokio::time::Instant::now() + quiescence;
        let mut frames = Vec::with_capacity(max_frames.min(1024));
        loop {
            if frames.len() >= max_frames {
                return Ok(frames);
            }
            let now = tokio::time::Instant::now();
            if now >= quiet_at || now >= overall_at {
                return Ok(frames);
            }
            let wait = quiet_at.min(overall_at) - now;
            // The inner idle bound is the overall budget; the *outer* timeout
            // enforces quiescence, so a quiet socket surfaces as Elapsed here
            // (quiescent), never as a spurious read error.
            let idle = overall_at - now;
            match tokio::time::timeout(wait, self.next_frame(idle)).await {
                // Elapsed: the window lapses; the next loop pass returns.
                Err(_elapsed) => {}
                Ok(Err(e)) => return Err(e),
                Ok(Ok(frame)) => {
                    if frame.is_data() {
                        frames.push(frame);
                        quiet_at = tokio::time::Instant::now() + quiescence;
                    }
                    if self.eof {
                        // Stream ended: nothing more can arrive; the
                        // scenario is complete by definition.
                        return Ok(frames);
                    }
                }
            }
        }
    }
}

/// Render collected frames as a normalized JSON array preserving sequence:
/// each entry records the dispatch index, optional `event`, optional
/// `comments`, and the payload (parsed JSON normalized through
/// [`normalize_value_with`; non-JSON payloads stay strings).
///
/// The **whole sequence is one normalization run**: id tokens use one
/// first-seen numbering across all frames, so an id referenced by frame 1
/// and frame 9 gets the same `<PREFIX_ID:n>` token (referential integrity
/// across the sequence, spec §5.4).
#[must_use]
pub fn frames_to_json(frames: &[Frame], env: &NormEnv) -> Value {
    let array = Value::Array(
        frames.iter().enumerate().map(|(index, frame)| frame_entry(frame, Some(index))).collect(),
    );
    normalize_value_with(&array, env, &mut IdNumbering::new())
}

/// One raw (pre-normalization) frame entry: `{index, event?, comments?,
/// data}`. The global dispatch `index` is interleaving-dependent and is used
/// only by [`frames_to_json`]; sequence comparison builds entries without it.
fn frame_entry(frame: &Frame, index: Option<usize>) -> Value {
    let mut entry = serde_json::Map::new();
    if let Some(index) = index {
        entry.insert("index".to_string(), Value::from(index));
    }
    if let Some(event) = &frame.event {
        entry.insert("event".to_string(), Value::String(event.clone()));
    }
    if !frame.comments.is_empty() {
        entry.insert(
            "comments".to_string(),
            Value::Array(frame.comments.iter().map(|c| Value::String(c.clone())).collect()),
        );
    }
    let data = serde_json::from_str::<Value>(&frame.data)
        .unwrap_or_else(|_| Value::String(frame.data.clone()));
    entry.insert("data".to_string(), data);
    Value::Object(entry)
}

// ---------------------------------------------------------------------------
// Sequence comparison (spec §"Sequence comparison")
// ---------------------------------------------------------------------------

/// First divergence between two collected frame sequences, reported per
/// group at the first divergent frame index (see [`compare_sequences`]).
#[derive(Debug, Clone, PartialEq)]
pub struct SequenceDiff {
    /// Diagnostic group key (see [`compare_sequences`] for its shape).
    pub group: String,
    /// Index of the first frame where the group's canonical orderings
    /// differ (equals the shorter side's length when one side ran out).
    pub index: usize,
    /// Reference-side frame entry at `index` (`Null` when absent).
    pub left: Value,
    /// Port-side frame entry at `index` (`Null` when absent).
    pub right: Value,
}

impl std::fmt::Display for SequenceDiff {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "sequence divergence in group {} at frame {}: reference={} port={}",
            self.group, self.index, self.left, self.right
        )
    }
}

/// One side's frames grouped per the canonical policy, with each group's
/// canonical ordering already normalized (one id-numbering run per group).
struct GroupedSequence {
    /// `(group key, normalized canonical ordering)` in first-appearance
    /// order of the groups.
    groups: Vec<(String, Vec<Value>)>,
}

/// Grouping metadata extracted from one frame payload.
struct FrameMeta {
    key: String,
    seq: Option<u64>,
}

/// Extract the grouping key and optional `seq` from a frame payload.
///
/// Key policy (spec): `location` + aggregate id — the aggregate is
/// `durable.aggregateID` (envelope), or top-level `aggregateID`
/// (`log.synced`), falling back to `data.sessionID` (the inner payload of
/// session events). Frames with neither location nor aggregate/session id
/// (e.g. `server.connected`) share the fixed `"global"` group.
fn frame_meta(payload: &Value) -> FrameMeta {
    let obj = payload.as_object();
    let location = obj.and_then(|o| o.get("location")).and_then(Value::as_object);
    let directory =
        location.and_then(|l| l.get("directory")).and_then(Value::as_str).unwrap_or_default();
    let workspace =
        location.and_then(|l| l.get("workspaceID")).and_then(Value::as_str).unwrap_or_default();
    let aggregate = obj
        .and_then(|o| o.get("durable"))
        .and_then(Value::as_object)
        .and_then(|d| d.get("aggregateID"))
        .and_then(Value::as_str)
        .or_else(|| obj.and_then(|o| o.get("aggregateID")).and_then(Value::as_str))
        .or_else(|| {
            obj.and_then(|o| o.get("data"))
                .and_then(Value::as_object)
                .and_then(|d| d.get("sessionID"))
                .and_then(Value::as_str)
        });
    let seq = obj
        .and_then(|o| o.get("durable"))
        .and_then(Value::as_object)
        .and_then(|d| d.get("seq"))
        .and_then(Value::as_u64)
        .or_else(|| obj.and_then(|o| o.get("seq")).and_then(Value::as_u64));

    let key = if directory.is_empty() && workspace.is_empty() && aggregate.is_none() {
        "global".to_string()
    } else {
        let ws = if workspace.is_empty() { String::new() } else { format!("+ws={workspace}") };
        let agg = aggregate.unwrap_or("?");
        format!("loc={directory}{ws}|agg={agg}")
    };
    FrameMeta { key, seq }
}

/// Group a side's data frames per the canonical policy and normalize each
/// group's canonical ordering. Heartbeat (non-data) frames are transport
/// noise and skipped.
///
/// Canonical order within a group: **when every frame of the group carries
/// `seq`, ascending `seq`; otherwise arrival order** (within one aggregate,
/// arrival order is deterministic — only cross-aggregate interleaving is
/// not). Each group is normalized as its own id-numbering run, so tokens are
/// invariant to cross-group interleaving; cross-aggregate references are the
/// job of whole-body compares, not sequence compares (spec).
fn group_frames(frames: &[Frame], env: &NormEnv) -> GroupedSequence {
    let mut order: Vec<String> = Vec::new();
    let mut buckets: std::collections::BTreeMap<String, Vec<(Option<u64>, Value)>> =
        std::collections::BTreeMap::new();
    for frame in frames.iter().filter(|frame| frame.is_data()) {
        let payload =
            serde_json::from_str::<Value>(&frame.data).unwrap_or(Value::String(frame.data.clone()));
        let meta = frame_meta(&payload);
        if !buckets.contains_key(&meta.key) {
            order.push(meta.key.clone());
        }
        buckets.entry(meta.key).or_default().push((meta.seq, frame_entry(frame, None)));
    }
    let groups = order
        .into_iter()
        .map(|key| {
            let mut bucket = buckets.remove(&key).unwrap_or_default();
            let all_sequenced = bucket.iter().all(|(seq, _)| seq.is_some());
            if all_sequenced {
                bucket.sort_by_key(|(seq, _)| seq.unwrap_or_default());
            }
            // One id-numbering run per group: encounter order = the group's
            // canonical order.
            let normalized = normalize_value_with(
                &Value::Array(bucket.into_iter().map(|(_, entry)| entry).collect()),
                env,
                &mut IdNumbering::new(),
            );
            let entries = normalized.as_array().cloned().unwrap_or_default();
            (key, entries)
        })
        .collect();
    GroupedSequence { groups }
}

/// Compare two collected frame sequences per the canonical policy (spec
/// §"Sequence comparison"):
///
/// 1. each side is grouped per `location`+aggregate (fallback `sessionID`),
/// 2. each group's ordering is canonicalized (all-seq → ascending `seq`,
///    else arrival order) and normalized (ids renumbered first-seen *within
///    the group*, so independent-aggregate interleaving cannot perturb
///    tokens),
/// 3. the two sides' **multisets of group orderings** are compared — group
///    *keys* are diagnostics only, because raw ids differ per run and groups
///    are paired by content, not by key,
/// 4. the first unmatched group (reference side first, in first-appearance
///    order, then port side) is reported against its best counterpart (the
///    opposite-side leftover with the longest common frame prefix) at the
///    **first divergent frame index**.
///
/// Returns `None` when the sequences agree.
#[must_use]
pub fn compare_sequences(
    frames_reference: &[Frame],
    env_reference: &NormEnv,
    frames_port: &[Frame],
    env_port: &NormEnv,
) -> Option<SequenceDiff> {
    let a = group_frames(frames_reference, env_reference);
    let b = group_frames(frames_port, env_port);

    // Multiset matching: pair equal orderings, greedily, A order × B order.
    let mut b_taken = vec![false; b.groups.len()];
    let mut a_unmatched: Vec<usize> = Vec::new();
    for (i, (_, a_entries)) in a.groups.iter().enumerate() {
        let match_at = b.groups.iter().enumerate().find(|(j, (_, b_entries))| {
            !b_taken[*j] && a_entries.as_slice() == b_entries.as_slice()
        });
        match match_at {
            Some((j, _)) => b_taken[j] = true,
            None => a_unmatched.push(i),
        }
    }
    let b_unmatched: Vec<usize> =
        b.groups.iter().enumerate().filter(|(j, _)| !b_taken[*j]).map(|(j, _)| j).collect();
    if a_unmatched.is_empty() && b_unmatched.is_empty() {
        return None;
    }

    // Report the first unmatched group; reference side takes priority.
    let (report_from_a, report_index) =
        if let Some(i) = a_unmatched.first() { (true, *i) } else { (false, b_unmatched[0]) };
    let (group, mine) = if report_from_a {
        (&a.groups[report_index].0, a.groups[report_index].1.as_slice())
    } else {
        (&b.groups[report_index].0, b.groups[report_index].1.as_slice())
    };
    // Best counterpart: opposite-side leftover with the longest common
    // prefix (ties → first).
    let counterparts: Vec<usize> =
        if report_from_a { b_unmatched.clone() } else { a_unmatched.clone() };
    let other = |k: usize| -> &[Value] {
        if report_from_a {
            b.groups[k].1.as_slice()
        } else {
            a.groups[k].1.as_slice()
        }
    };
    let counterpart = counterparts
        .iter()
        .copied()
        .max_by_key(|k| common_prefix_len(mine, other(*k)))
        .map(|k| other(k).to_vec());

    let (left, right) = if report_from_a {
        (Some(mine), counterpart.as_deref())
    } else {
        (counterpart.as_deref(), Some(mine))
    };
    let (index, left_frame, right_frame) =
        first_divergent_frame(left.unwrap_or(&[]), right.unwrap_or(&[]));
    Some(SequenceDiff { group: group.clone(), index, left: left_frame, right: right_frame })
}

/// Length of the common prefix of two frame-entry sequences.
fn common_prefix_len(a: &[Value], b: &[Value]) -> usize {
    a.iter().zip(b.iter()).take_while(|(x, y)| x == y).count()
}

/// Golden form of a scenario capture: the captured data frames grouped per
/// the canonical policy (§8.1), each group canonicalized (§8.2) and
/// normalized as its own id-numbering run (§8.3), with the group **keys
/// dropped** (raw ids and sandbox paths are per-run diagnostics) and the
/// groups **sorted by canonical serialization** — a deterministic
/// serialization of the §8.4 multiset: two captures produce equal vectors
/// iff their multisets of group orderings are equal.
///
/// This is exactly what a scenario golden stores, and what a fresh capture
/// is compared against in run mode (`scenario::assert_golden`).
#[must_use]
pub fn scenario_capture(frames: &[Frame], env: &NormEnv) -> Vec<Value> {
    let grouped = group_frames(frames, env);
    let mut groups: Vec<Value> =
        grouped.groups.into_iter().map(|(_, entries)| Value::Array(entries)).collect();
    groups.sort_by_key(std::string::ToString::to_string);
    groups
}

/// First divergence between a golden capture and a fresh capture, both in
/// the [`scenario_capture`] form (sorted group orderings). Multiset
/// semantics: groups are paired greedily by equality (order in the sorted
/// vectors is canonical, so positional pairing is exact); the first
/// unmatched position is reported against its best counterpart — the
/// opposite-side leftover with the longest common frame prefix — at the
/// first divergent frame index, mirroring [`compare_sequences`]'s report.
#[must_use]
pub fn compare_scenario_captures(golden: &[Value], actual: &[Value]) -> Option<SequenceDiff> {
    if golden == actual {
        return None;
    }
    let mut golden_taken = vec![false; golden.len()];
    let mut actual_taken = vec![false; actual.len()];
    for (i, g) in golden.iter().enumerate() {
        if let Some((j, _)) = actual.iter().enumerate().find(|(j, a)| !actual_taken[*j] && *a == g)
        {
            golden_taken[i] = true;
            actual_taken[j] = true;
        }
    }
    let golden_leftovers: Vec<usize> =
        golden.iter().enumerate().filter(|(i, _)| !golden_taken[*i]).map(|(i, _)| i).collect();
    let actual_leftovers: Vec<usize> =
        actual.iter().enumerate().filter(|(j, _)| !actual_taken[*j]).map(|(j, _)| j).collect();
    // Golden side takes priority; its counterpart is the actual leftover
    // with the longest common frame prefix (mirrors compare_sequences).
    let (index_golden, index_actual) = match (golden_leftovers.first(), actual_leftovers.first()) {
        (Some(&g), Some(&_)) => {
            let g_frames = golden[g].as_array().map_or::<&[Value], _>(&[], Vec::as_slice);
            let best = actual_leftovers
                .iter()
                .copied()
                .max_by_key(|&a| {
                    common_prefix_len(
                        g_frames,
                        actual[a].as_array().map_or::<&[Value], _>(&[], Vec::as_slice),
                    )
                })
                .unwrap_or(usize::MAX);
            (g, best)
        }
        // A count imbalance reports the present side against an absent
        // (empty) counterpart.
        (Some(&g), None) => (g, usize::MAX),
        (None, Some(&a)) => (usize::MAX, a),
        (None, None) => return None,
    };
    let group = if index_golden == usize::MAX {
        format!("capture group #{index_actual}")
    } else {
        format!("golden group #{index_golden}")
    };
    let left = golden
        .get(index_golden)
        .and_then(Value::as_array)
        .map_or::<&[Value], _>(&[], Vec::as_slice);
    let right = actual
        .get(index_actual)
        .and_then(Value::as_array)
        .map_or::<&[Value], _>(&[], Vec::as_slice);
    let (frame, left_frame, right_frame) = first_divergent_frame(left, right);
    Some(SequenceDiff { group, index: frame, left: left_frame, right: right_frame })
}

/// First index where two canonical orderings differ; the side missing the
/// frame renders `Null` (absent frames and absent groups both surface as
/// `Null` on the missing side).
fn first_divergent_frame(a: &[Value], b: &[Value]) -> (usize, Value, Value) {
    let shared = common_prefix_len(a, b);
    let index = shared.min(a.len().min(b.len()));
    if shared == a.len() && shared == b.len() {
        // Equal orderings should not reach here; report defensively.
        return (index, Value::Null, Value::Null);
    }
    (
        index,
        a.get(index).cloned().unwrap_or(Value::Null),
        b.get(index).cloned().unwrap_or(Value::Null),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_data_event_and_heartbeat_frames() {
        let mut parser = Parser::default();
        assert!(parser
            .feed_line(
                "data: {\"id\":\"evt_11c556f9e001H4T7m94mksITaJ\",\"type\":\"server.connected\"}"
            )
            .is_none());
        assert!(parser.feed_line("").is_some());
        let heartbeat = parser.feed_line(": heartbeat").is_none();
        assert!(heartbeat);
        let frame = parser.feed_line("").unwrap();
        assert!(!frame.is_data());
        assert_eq!(frame.comments, vec!["heartbeat".to_string()]);
    }

    #[test]
    fn joins_multi_line_data_and_strips_single_space() {
        let mut parser = Parser::default();
        parser.feed_line("event: session.updated");
        parser.feed_line("data:line1");
        parser.feed_line("data:  line2");
        let frame = parser.feed_line("").unwrap();
        assert_eq!(frame.event.as_deref(), Some("session.updated"));
        // Exactly one leading space is stripped after the colon (SSE spec).
        assert_eq!(frame.data, "line1\n line2");
    }

    #[test]
    fn normalizes_frame_sequence_to_json() {
        let frames = vec![
            Frame {
                event: None,
                data: "{\"id\":\"evt_11c556f9e001H4T7m94mksITaJ\",\"type\":\"server.connected\",\"data\":{}}".to_string(),
                comments: vec![],
            },
            Frame {
                event: None,
                data: "{\"id\":\"evt_11c556f9e001H4T7m94mksITaJ\",\"type\":\"server.connected\",\"data\":{}}".to_string(),
                comments: vec![],
            },
            Frame { event: None, data: "plain text".to_string(), comments: vec!["heartbeat".to_string()] },
        ];
        let out = frames_to_json(&frames, &NormEnv::new());
        assert_eq!(out[0]["data"]["id"], Value::from("<EVT_ID:1>"));
        assert_eq!(out[0]["data"]["type"], Value::from("server.connected"));
        // One numbering run across the sequence: the same evt id in the
        // second frame reuses its token (cross-frame reference agreement).
        assert_eq!(out[1]["data"]["id"], Value::from("<EVT_ID:1>"));
        assert_eq!(out[2]["data"], Value::from("plain text"));
        assert_eq!(out[2]["comments"][0], Value::from("heartbeat"));
    }

    // -- sequence comparison (spec §"Sequence comparison") ------------------

    fn frame(data: impl Into<String>) -> Frame {
        Frame { event: None, data: data.into(), comments: vec![] }
    }

    fn env() -> NormEnv {
        NormEnv::new()
    }

    /// A durable session event for aggregate `ses`, at `seq`, of `kind`.
    fn session_event(event_id: &str, ses: &str, seq: u64, kind: &str) -> Frame {
        frame(format!(
            r#"{{"id":"{event_id}","created":1760000000000,"type":"session.{kind}","durable":{{"aggregateID":"{ses}","seq":{seq},"version":1}},"data":{{"sessionID":"{ses}","kind":"{kind}"}}}}"#
        ))
    }

    /// An ephemeral session event (no `durable`, no `seq`): grouping falls
    /// back to `data.sessionID`; canonical order is arrival order.
    fn ephemeral_session_event(event_id: &str, ses: &str, text: &str) -> Frame {
        frame(format!(
            r#"{{"id":"{event_id}","created":1760000000000,"type":"tui.prompt.append","location":{{"directory":"/repo"}},"data":{{"sessionID":"{ses}","text":"{text}"}}}}"#
        ))
    }

    fn connected(event_id: &str) -> Frame {
        frame(format!(
            r#"{{"id":"{event_id}","created":1760000000000,"type":"server.connected","data":{{}}}}"#
        ))
    }

    #[test]
    fn interleaved_aggregates_compare_equal() {
        // Two independent aggregates with different shapes, interleaved in
        // different global orders on the two sides (and different raw ids):
        // the multiset of canonical per-group orderings is identical.
        let a = vec![
            session_event(
                "evt_11c556f9e001AAAAAAAAAAAAAAAA",
                "ses_01JZY0ID0123456789AAAA",
                1,
                "message.updated",
            ),
            session_event(
                "evt_11c556f9e001BBBBBBBBBBBBBBBB",
                "ses_01JZY0ID0123456789BBBB",
                1,
                "idle",
            ),
            session_event(
                "evt_11c556f9e001CCCCCCCCCCCCCCCC",
                "ses_01JZY0ID0123456789AAAA",
                2,
                "message.updated",
            ),
            session_event(
                "evt_11c556f9e001DDDDDDDDDDDDDDDD",
                "ses_01JZY0ID0123456789BBBB",
                2,
                "idle",
            ),
        ];
        let b = vec![
            session_event(
                "evt_22c556f9e001EEEEEEEEEEEEEEEE",
                "ses_01JZY0ID0123456789EEEE",
                2,
                "idle",
            ),
            session_event(
                "evt_22c556f9e001FFFFFFFFFFFFFFFF",
                "ses_01JZY0ID0123456789FFFF",
                2,
                "message.updated",
            ),
            session_event(
                "evt_22c556f9e001GGGGGGGGGGGGGGGG",
                "ses_01JZY0ID0123456789EEEE",
                1,
                "idle",
            ),
            session_event(
                "evt_22c556f9e001HHHHHHHHHHHHHHHH",
                "ses_01JZY0ID0123456789FFFF",
                1,
                "message.updated",
            ),
        ];
        assert!(compare_sequences(&a, &env(), &b, &env()).is_none());
    }

    #[test]
    fn seq_orders_frames_regardless_of_arrival_order() {
        // Same aggregate, frames arriving in opposite orders but carrying
        // seq: canonical (ascending seq) orderings are identical.
        let a = vec![
            session_event(
                "evt_11c556f9e001AAAAAAAAAAAAAAAA",
                "ses_01JZY0ID0123456789AAAA",
                1,
                "started",
            ),
            session_event(
                "evt_11c556f9e001BBBBBBBBBBBBBBBB",
                "ses_01JZY0ID0123456789AAAA",
                2,
                "streamed",
            ),
            session_event(
                "evt_11c556f9e001CCCCCCCCCCCCCCCC",
                "ses_01JZY0ID0123456789AAAA",
                3,
                "completed",
            ),
        ];
        let mut b: Vec<Frame> = a
            .iter()
            .map(|f| {
                let mut f = f.clone();
                f.data = f.data.replace("evt_11c556f9e001", "evt_99c556f9e001");
                f
            })
            .collect();
        b.reverse();
        assert!(compare_sequences(&a, &env(), &b, &env()).is_none());
    }

    #[test]
    fn wrong_order_within_unsequenced_group_fails() {
        // No seq on these frames: canonical order is arrival order, and the
        // port side emits the two events swapped — a genuine divergence.
        let a = vec![
            ephemeral_session_event(
                "evt_11c556f9e001AAAAAAAAAAAAAAAA",
                "ses_01JZY0ID0123456789AAAA",
                "first",
            ),
            ephemeral_session_event(
                "evt_11c556f9e001BBBBBBBBBBBBBBBB",
                "ses_01JZY0ID0123456789AAAA",
                "second",
            ),
        ];
        let b = vec![
            ephemeral_session_event(
                "evt_22c556f9e001AAAAAAAAAAAAAAAA",
                "ses_01JZY0ID0123456789BBBB",
                "second",
            ),
            ephemeral_session_event(
                "evt_22c556f9e001BBBBBBBBBBBBBBBB",
                "ses_01JZY0ID0123456789BBBB",
                "first",
            ),
        ];
        let diff =
            compare_sequences(&a, &env(), &b, &env()).expect("swapped arrival order must diverge");
        assert_eq!(diff.index, 0);
        assert_ne!(diff.left, diff.right);
        assert_eq!(diff.left["data"]["data"]["text"], Value::from("first"));
    }

    #[test]
    fn wrong_seq_fails() {
        // Both sides carry seq, but the port side assigns the middle two
        // seq numbers to the opposite event kinds: after canonical sorting,
        // the orderings diverge at the second frame.
        let a = vec![
            session_event(
                "evt_11c556f9e001AAAAAAAAAAAAAAAA",
                "ses_01JZY0ID0123456789AAAA",
                1,
                "started",
            ),
            session_event(
                "evt_11c556f9e001BBBBBBBBBBBBBBBB",
                "ses_01JZY0ID0123456789AAAA",
                2,
                "streamed",
            ),
            session_event(
                "evt_11c556f9e001CCCCCCCCCCCCCCCC",
                "ses_01JZY0ID0123456789AAAA",
                3,
                "completed",
            ),
        ];
        let b = vec![
            session_event(
                "evt_22c556f9e001AAAAAAAAAAAAAAAA",
                "ses_01JZY0ID0123456789BBBB",
                1,
                "started",
            ),
            session_event(
                "evt_22c556f9e001BBBBBBBBBBBBBBBB",
                "ses_01JZY0ID0123456789BBBB",
                3,
                "streamed",
            ),
            session_event(
                "evt_22c556f9e001CCCCCCCCCCCCCCCC",
                "ses_01JZY0ID0123456789BBBB",
                2,
                "completed",
            ),
        ];
        let diff =
            compare_sequences(&a, &env(), &b, &env()).expect("wrong seq assignment must diverge");
        assert_eq!(diff.index, 1);
        // At index 1 the reference has the seq-2 "streamed" event; the port
        // side's canonical order puts "completed" there.
        assert_eq!(diff.left["data"]["data"]["kind"], Value::from("streamed"));
        assert_eq!(diff.right["data"]["data"]["kind"], Value::from("completed"));
    }

    #[test]
    fn missing_group_reports_null_counterpart() {
        // The reference produced a second aggregate the port never emitted:
        // reported with a Null counterpart at frame 0.
        let a = vec![
            session_event(
                "evt_11c556f9e001AAAAAAAAAAAAAAAA",
                "ses_01JZY0ID0123456789AAAA",
                1,
                "started",
            ),
            session_event(
                "evt_11c556f9e001BBBBBBBBBBBBBBBB",
                "ses_01JZY0ID0123456789BBBB",
                1,
                "idle",
            ),
        ];
        let b = vec![session_event(
            "evt_22c556f9e001AAAAAAAAAAAAAAAA",
            "ses_01JZY0ID0123456789CCCC",
            1,
            "started",
        )];
        let diff =
            compare_sequences(&a, &env(), &b, &env()).expect("a missing aggregate must diverge");
        assert_eq!(diff.index, 0);
        assert_ne!(diff.left, Value::Null);
        assert_eq!(diff.right, Value::Null);
    }

    #[test]
    fn global_frames_group_together_by_arrival() {
        // Frames with no location and no aggregate/session id (e.g.
        // server.connected) share the "global" group; equal arrival order
        // compares equal even when interleaved differently against other
        // groups.
        let a = vec![
            connected("evt_11c556f9e001AAAAAAAAAAAAAAAA"),
            session_event(
                "evt_11c556f9e001BBBBBBBBBBBBBBBB",
                "ses_01JZY0ID0123456789AAAA",
                1,
                "started",
            ),
            connected("evt_11c556f9e001CCCCCCCCCCCCCCCC"),
        ];
        let b = vec![
            connected("evt_22c556f9e001AAAAAAAAAAAAAAAA"),
            session_event(
                "evt_22c556f9e001BBBBBBBBBBBBBBBB",
                "ses_01JZY0ID0123456789BBBB",
                1,
                "started",
            ),
            connected("evt_22c556f9e001CCCCCCCCCCCCCCCC"),
        ];
        assert!(compare_sequences(&a, &env(), &b, &env()).is_none());
    }

    // -- scenario capture (golden form) -------------------------------------

    #[test]
    fn scenario_capture_is_interleaving_invariant_and_keyless() {
        // Same two aggregates, interleaved differently: the capture vectors
        // (sorted group orderings) are identical — cross-aggregate
        // interleaving must not leak into a golden.
        let a = vec![
            session_event(
                "evt_11c556f9e001AAAAAAAAAAAAAAAA",
                "ses_01JZY0ID0123456789AAAA",
                1,
                "started",
            ),
            session_event(
                "evt_11c556f9e001BBBBBBBBBBBBBBBB",
                "ses_01JZY0ID0123456789BBBB",
                1,
                "idle",
            ),
            session_event(
                "evt_11c556f9e001CCCCCCCCCCCCCCCC",
                "ses_01JZY0ID0123456789AAAA",
                2,
                "streamed",
            ),
        ];
        let mut b: Vec<Frame> = a
            .iter()
            .map(|f| {
                let mut f = f.clone();
                f.data = f.data.replace("evt_11c556f9e001", "evt_99c556f9e001");
                f
            })
            .collect();
        b.reverse();
        let capture_a = scenario_capture(&a, &env());
        let capture_b = scenario_capture(&b, &env());
        assert_eq!(capture_a, capture_b);
        assert_eq!(capture_a.len(), 2, "two aggregates → two groups");
        // Each group is a bare array of frame entries (no group keys: raw
        // ids are per-run diagnostics).
        for group in &capture_a {
            assert!(group.is_array());
        }
    }

    #[test]
    fn scenario_capture_compare_detects_frame_and_count_divergence() {
        let base = vec![
            session_event(
                "evt_11c556f9e001AAAAAAAAAAAAAAAA",
                "ses_01JZY0ID0123456789AAAA",
                1,
                "started",
            ),
            session_event(
                "evt_11c556f9e001BBBBBBBBBBBBBBBB",
                "ses_01JZY0ID0123456789AAAA",
                2,
                "idle",
            ),
        ];
        let golden = scenario_capture(&base, &env());

        // 1. Same sequence, different raw ids: no divergence.
        let same: Vec<Frame> = base
            .iter()
            .map(|f| {
                let mut f = f.clone();
                f.data = f.data.replace("evt_11c556f9e001", "evt_99c556f9e001");
                f
            })
            .collect();
        assert!(compare_scenario_captures(&golden, &scenario_capture(&same, &env())).is_none());

        // 2. One frame changed: divergence reported at that frame.
        let changed = vec![
            session_event(
                "evt_11c556f9e001AAAAAAAAAAAAAAAA",
                "ses_01JZY0ID0123456789AAAA",
                1,
                "started",
            ),
            session_event(
                "evt_11c556f9e001BBBBBBBBBBBBBBBB",
                "ses_01JZY0ID0123456789AAAA",
                2,
                "deleted",
            ),
        ];
        let diff = compare_scenario_captures(&golden, &scenario_capture(&changed, &env()))
            .expect("a changed frame must diverge");
        assert_eq!(diff.index, 1, "first divergent frame index");

        // 3. An extra aggregate on the capture side: count divergence with
        // an absent golden counterpart.
        let extra = vec![
            base[0].clone(),
            base[1].clone(),
            session_event(
                "evt_11c556f9e001DDDDDDDDDDDDDDDD",
                "ses_01JZY0ID0123456789CCCC",
                1,
                "idle",
            ),
        ];
        let diff = compare_scenario_captures(&golden, &scenario_capture(&extra, &env()))
            .expect("an extra group must diverge");
        assert!(diff.group.contains("capture"), "extra group is reported on the capture side");
    }

    // -- scenario collection (policy b) -------------------------------------

    /// Serve an SSE endpoint: send a 200 event-stream head, then replay
    /// `script` (bytes per write, with the given delay before each write),
    /// then hold the connection open.
    async fn serve_sse(script: Vec<(std::time::Duration, Vec<u8>)>) -> String {
        use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
        const HEAD: &[u8] = b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\n\r\n";
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://127.0.0.1:{}", listener.local_addr().unwrap().port());
        tokio::spawn(async move {
            if let Ok((mut socket, _)) = listener.accept().await {
                let mut sink = [0u8; 512];
                let _ = socket.read(&mut sink).await;
                if socket.write_all(HEAD).await.is_err() {
                    return;
                }
                for (delay, bytes) in script {
                    tokio::time::sleep(delay).await;
                    if socket.write_all(&bytes).await.is_err() {
                        return;
                    }
                }
                // Hold the connection open (quiet server).
                tokio::time::sleep(std::time::Duration::from_secs(30)).await;
            }
        });
        base
    }

    fn sse_bytes(id: u64) -> Vec<u8> {
        format!("data: {{\"id\":\"evt_11c556f9e001{id:018}\"}}\n\n").into_bytes()
    }

    #[tokio::test]
    async fn collect_scenario_stops_at_quiescence() {
        let base = serve_sse(vec![
            (std::time::Duration::from_millis(10), sse_bytes(1)),
            (std::time::Duration::from_millis(10), sse_bytes(2)),
            (std::time::Duration::from_millis(10), sse_bytes(3)),
            // then silence forever
        ])
        .await;
        let mut stream =
            SseStream::subscribe(&base, "pw", "/api/event", std::time::Duration::from_secs(5))
                .await
                .unwrap();
        let frames = stream
            .collect_scenario(
                10,
                std::time::Duration::from_millis(300),
                std::time::Duration::from_secs(10),
            )
            .await
            .unwrap();
        assert_eq!(frames.len(), 3, "quiescence (300ms without data) must end collection");
    }

    #[tokio::test]
    async fn collect_scenario_stops_at_frame_watermark() {
        let base = serve_sse(vec![
            (std::time::Duration::from_millis(20), sse_bytes(1)),
            (std::time::Duration::from_millis(20), sse_bytes(2)),
            (std::time::Duration::from_millis(20), sse_bytes(3)),
            (std::time::Duration::from_millis(20), sse_bytes(4)),
            (std::time::Duration::from_millis(20), sse_bytes(5)),
        ])
        .await;
        let mut stream =
            SseStream::subscribe(&base, "pw", "/api/event", std::time::Duration::from_secs(5))
                .await
                .unwrap();
        let started = std::time::Instant::now();
        let frames = stream
            .collect_scenario(
                3,
                std::time::Duration::from_secs(30),
                std::time::Duration::from_secs(30),
            )
            .await
            .unwrap();
        assert_eq!(frames.len(), 3, "the N-frame watermark must end collection");
        assert!(started.elapsed() < std::time::Duration::from_secs(10));
    }

    #[tokio::test]
    async fn collect_scenario_heartbeats_do_not_reset_quiescence() {
        let base = serve_sse(vec![
            (std::time::Duration::from_millis(10), sse_bytes(1)),
            (std::time::Duration::from_millis(100), b": heartbeat\n\n".to_vec()),
            (std::time::Duration::from_millis(100), b": heartbeat\n\n".to_vec()),
        ])
        .await;
        let mut stream =
            SseStream::subscribe(&base, "pw", "/api/event", std::time::Duration::from_secs(5))
                .await
                .unwrap();
        let started = std::time::Instant::now();
        let frames = stream
            .collect_scenario(
                10,
                std::time::Duration::from_millis(400),
                std::time::Duration::from_secs(10),
            )
            .await
            .unwrap();
        assert_eq!(frames.len(), 1);
        // Heartbeats after the single data frame must not hold collection
        // open: it ends at the quiescence window, not the overall timeout.
        assert!(started.elapsed() < std::time::Duration::from_secs(2));
    }
}

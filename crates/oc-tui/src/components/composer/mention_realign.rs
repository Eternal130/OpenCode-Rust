//! G-93 — mention re-attachment after the `$EDITOR` round-trip.
//!
//! Port of `packages/tui/src/prompt/mention.ts`
//! (`realignPromptMentions` + `realignPromptInputMentions`,
//! lines 20-74): when the editor returns edited text, every
//! attachment chip (`@file` / `@agent` / `@skill`) whose mention
//! text still occurs in the new content is re-attached to the best
//! matching occurrence; mentions that no longer occur are dropped.
//!
//! Bounded deviation from the ref (documented per the lane
//! contract): the ref's cost anchor and offsets use
//! `promptOffsetWidth` (DISPLAY columns); the port's
//! [`crate::components::composer::PromptDraft`] tracks chip ranges
//! as CHAR indices, so this port performs the identical DP over
//! char offsets instead. The matching outcome (max matches, then
//! min total displacement) is unchanged for any text where chip
//! width equals char count; wide/narrow graphemes only perturb the
//! tie-break cost, never the match set.

use super::{AgentMention, FileMention, PromptDraft, SkillMention};

/// One chip snapshot fed to the realigner (ref `PromptMention`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct MentionCandidate {
    /// The chip's text (`@path`, `@agent`, …) as it occurred in the
    /// pre-edit draft.
    pub text: String,
    /// The chip's char offset in the EXPANDED pre-edit text (the
    /// editor buffer's coordinate space — ref
    /// `expandPromptInputPastedText` shifts mentions the same way).
    pub start: usize,
}

/// A candidate occurrence of a mention text inside the new content
/// (ref `CandidateRange`; `offset` is a char offset here).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Candidate {
    #[allow(dead_code)]
    start: usize,
    end: usize,
    offset: usize,
}

/// The realigned chip list: `Some(range)` = re-attached at the char
/// range in the new text, `None` = dropped (mention.ts:39-47).
pub(crate) fn realign_mentions(
    content: &str,
    mentions: &[MentionCandidate],
) -> Vec<Option<std::ops::Range<usize>>> {
    let mut aligned: Vec<Option<std::ops::Range<usize>>> = vec![None; mentions.len()];
    // Group the mention items by text (mention.ts:26-32).
    let mut texts: Vec<String> = Vec::new();
    let mut groups: Vec<Vec<usize>> = Vec::new();
    for (index, mention) in mentions.iter().enumerate() {
        if mention.text.is_empty() {
            continue;
        }
        if let Some(group) = texts.iter().position(|text| text == &mention.text) {
            groups[group].push(index);
        } else {
            texts.push(mention.text.clone());
            groups.push(vec![index]);
        }
    }
    // Longest text first, ties lexicographic (mention.ts:34-36 —
    // `localeCompare` approximated by byte order).
    let mut order: Vec<usize> = (0..texts.len()).collect();
    order.sort_by(|left, right| {
        texts[*right]
            .chars()
            .count()
            .cmp(&texts[*left].chars().count())
            .then_with(|| texts[*left].cmp(&texts[*right]))
    });

    let mut protected: Vec<std::ops::Range<usize>> = Vec::new();
    for group in order {
        let text = &texts[group];
        let items = &groups[group];
        let candidates = mention_ranges(content, text);
        let available: Vec<Candidate> = candidates
            .iter()
            .copied()
            .filter(|candidate| {
                !protected
                    .iter()
                    .any(|range| range.start < candidate.end && range.end > candidate.start)
            })
            .collect();
        for (item, candidate) in assign_mentions(mentions, items, &available) {
            aligned[item] = Some(candidate.offset..candidate.offset + text.chars().count());
        }
        // mention.ts:46 protects EVERY candidate of the text (used
        // or not) from later groups.
        protected.extend(candidates.iter().map(|candidate| candidate.start..candidate.end));
    }
    aligned
}

/// All occurrences of `text` in `content` (mention.ts:99-108).
fn mention_ranges(content: &str, text: &str) -> Vec<Candidate> {
    let mut ranges = Vec::new();
    let mut search_from = 0usize;
    let width = text.chars().count();
    loop {
        let Some(start) = content[search_from..].find(text) else {
            return ranges;
        };
        let start = search_from + start;
        let offset = content[..start].chars().count();
        ranges.push(Candidate { start, end: start + width, offset });
        search_from = start + width.max(1);
    }
}

/// The min-cost max-matching (mention.ts:110-140): items ordered by
/// their original start, a memoized recursion over
/// (item, candidate) choosing the outcome with the MOST matches and
/// the LEAST total `|candidate.offset - item.start|` cost.
fn assign_mentions(
    mentions: &[MentionCandidate],
    items: &[usize],
    candidates: &[Candidate],
) -> Vec<(usize, Candidate)> {
    #[derive(Clone)]
    struct Solved {
        matches: usize,
        cost: i64,
        pairs: Vec<(usize, Candidate)>,
    }
    let mut memo: std::collections::HashMap<(usize, usize), Solved> =
        std::collections::HashMap::new();
    let mut ordered: Vec<usize> = items.to_vec();
    ordered.sort_by_key(|index| (mentions[*index].start, *index));

    fn solve(
        mentions: &[MentionCandidate],
        ordered: &[usize],
        candidates: &[Candidate],
        memo: &mut std::collections::HashMap<(usize, usize), Solved>,
        item: usize,
        candidate: usize,
    ) -> Solved {
        if item >= ordered.len() || candidate >= candidates.len() {
            return Solved { matches: 0, cost: 0, pairs: Vec::new() };
        }
        if let Some(cached) = memo.get(&(item, candidate)) {
            return cached.clone();
        }
        let current = ordered[item];
        let range = candidates[candidate];
        let tail = solve(mentions, ordered, candidates, memo, item + 1, candidate + 1);
        let mut matched = Solved {
            matches: tail.matches + 1,
            cost: tail.cost
                + i64::try_from(range.offset.abs_diff(mentions[current].start)).unwrap_or(i64::MAX),
            pairs: Vec::with_capacity(tail.pairs.len() + 1),
        };
        matched.pairs.push((current, range));
        matched.pairs.extend(tail.pairs);
        let skip_item = solve(mentions, ordered, candidates, memo, item + 1, candidate);
        let skip_candidate = solve(mentions, ordered, candidates, memo, item, candidate + 1);
        let best = [matched, skip_item, skip_candidate]
            .into_iter()
            .reduce(|best, next| {
                if next.matches != best.matches {
                    return if next.matches > best.matches { next } else { best };
                }
                if next.cost < best.cost {
                    next
                } else {
                    best
                }
            })
            .expect("non-empty");
        memo.insert((item, candidate), best.clone());
        best
    }

    solve(mentions, &ordered, candidates, &mut memo, 0, 0).pairs
}

/// The composer-facing realign entry: rebuild a draft for the edited
/// `text` keeping the chips whose mention text still occurs
/// (`realignPromptInputMentions` + the `setStore("prompt", …)` +
/// `pasted: []` in prompt/index.tsx:570-573).
///
/// The mention snapshot MUST be taken in the editor's coordinate
/// space: the caller passes `expanded` = the draft the editor opened
/// over (paste chips already expanded) so chip starts align with the
/// pre-edit content the ref compares against.
pub(crate) fn realign_draft(
    original: &PromptDraft,
    expanded_text: &str,
    text: &str,
) -> PromptDraft {
    // The mention list order matches the ref concat (files, then
    // agents, then skills — mention.ts:56-60).
    let chars: Vec<char> = expanded_text.chars().collect();
    let chip = |start: usize, end: usize| -> String {
        if start <= end && end <= chars.len() {
            chars[start..end].iter().collect()
        } else {
            String::new()
        }
    };
    let mut mentions = Vec::new();
    let mut files = Vec::new();
    for file in &original.files {
        mentions.push(MentionCandidate { text: chip(file.start, file.end), start: file.start });
        files.push(file.clone());
    }
    let agents: Vec<&AgentMention> = original.agents.iter().collect();
    let skills: Vec<&SkillMention> = original.skills.iter().collect();
    for agent in &agents {
        mentions.push(MentionCandidate { text: chip(agent.start, agent.end), start: agent.start });
    }
    for skill in &skills {
        mentions.push(MentionCandidate { text: chip(skill.start, skill.end), start: skill.start });
    }

    let aligned = realign_mentions(text, &mentions);
    let mut draft = PromptDraft { text: text.to_owned(), ..Default::default() };
    // Bounded deviation: the ref's no-text mentions (mention.ts:25,
    // 63) are KEPT untouched; a Rust chip with an empty/unresolvable
    // text range (e.g. an out-of-band FileMention carrying no
    // in-text chip) is dropped instead — keeping it would leave a
    // stale range pointing into the edited text.
    for (index, range) in aligned.iter().enumerate() {
        let Some(range) = range else { continue };
        if index < files.len() {
            let mut file: FileMention = files[index].clone();
            file.start = range.start;
            file.end = range.end;
            draft.files.push(file);
        } else if index < files.len() + agents.len() {
            let mut agent: AgentMention = (*agents[index - files.len()]).clone();
            agent.start = range.start;
            agent.end = range.end;
            draft.agents.push(agent);
        } else {
            let mut skill: SkillMention = (*skills[index - files.len() - agents.len()]).clone();
            skill.start = range.start;
            skill.end = range.end;
            draft.skills.push(skill);
        }
    }
    // prompt/index.tsx:572: the editor buffer held the EXPANDED
    // pastes — the restored draft carries no folded chips.
    draft
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mention(text: &str, start: usize) -> MentionCandidate {
        MentionCandidate { text: text.to_owned(), start }
    }

    /// A mention whose text occurs once re-attaches at the occurrence
    /// even after surrounding text was edited.
    #[test]
    fn mention_reattaches_after_edit() {
        let content = "see @file.rs please @build thanks";
        let mentions = [mention("@file.rs", 4), mention("@build", 21)];
        let aligned = realign_mentions(content, &mentions);
        assert_eq!(aligned[0], Some(4..12));
        assert_eq!(aligned[1], Some(20..26));
    }

    /// A mention the edit REMOVED is dropped, not re-attached
    /// (mention.ts:63-65's flatMap drops the undefined slots).
    #[test]
    fn dropped_mention_is_not_reattached() {
        let content = "now only @build remains";
        let mentions = [mention("@file.rs", 4), mention("@build", 21)];
        let aligned = realign_mentions(content, &mentions);
        assert_eq!(aligned[0], None, "the removed @file.rs is dropped");
        assert_eq!(aligned[1], Some(9..15));
    }

    /// Two mentions of the SAME text both re-attach, each to its own
    /// occurrence (min-cost assignment keeps the original order).
    #[test]
    fn duplicate_mentions_split_occurrences() {
        let content = "a @x b @x c";
        let mentions = [mention("@x", 2), mention("@x", 7)];
        let aligned = realign_mentions(content, &mentions);
        assert_eq!(aligned[0], Some(2..4));
        assert_eq!(aligned[1], Some(7..9));
    }

    /// Identical texts never share one occurrence: the protected-range
    /// pass keeps later groups off earlier candidates.
    #[test]
    fn protected_ranges_prevent_sharing() {
        let content = "one @foo two";
        let mentions = [mention("@foo", 4), mention("@foo", 4)];
        let aligned = realign_mentions(content, &mentions);
        assert_eq!(aligned[0], Some(4..8));
        assert_eq!(aligned[1], None, "only one occurrence exists");
    }

    /// The full draft rebuild keeps names/payloads and drops pasted
    /// chips (the editor opened over their expansion).
    #[test]
    fn realign_draft_rebuilds_attachments() {
        let original = PromptDraft {
            text: "hi @build".to_owned(),
            files: vec![FileMention {
                name: "a.ts".to_owned(),
                start: 0,
                end: 0,
                payload: None,
                mention: None,
            }],
            agents: vec![AgentMention { name: "build".to_owned(), start: 3, end: 9 }],
            skills: Vec::new(),
            pasted: Vec::new(),
        };
        // The file mention has an empty chip text (no occurrence) —
        // dropped; the agent chip survives the edit.
        let draft = realign_draft(&original, &original.text, "edited: @build ok");
        assert_eq!(draft.text, "edited: @build ok");
        assert!(draft.files.is_empty(), "the empty-text mention dropped");
        assert_eq!(draft.agents.len(), 1);
        assert_eq!(draft.agents[0].name, "build");
        assert_eq!(draft.agents[0].start, 8);
        assert_eq!(draft.agents[0].end, 14);
        assert_eq!(draft.pasted, [] as [crate::components::composer::PastedPart; 0]);
    }
}

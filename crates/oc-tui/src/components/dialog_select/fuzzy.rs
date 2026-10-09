//! A faithful port of `fuzzysort` 3.1.0 (the reference TUI's fuzzy
//! scorer, `packages/tui/node_modules/fuzzysort`) — the dialog-select
//! filter's ordering contract. Ported operation-for-operation from the
//! reference source so scores are bit-identical `f64`s for the same
//! inputs (same operation order over IEEE-754 doubles); the golden
//! query orders in `fuzzy_corpus.json` pin the behavior end-to-end.
//!
//! Ported surface (everything `ui/dialog-select.tsx` exercises):
//! * [`go_keys`] — `fuzzysort.go(needle, targets, { keys, scoreFn,
//!   threshold })`, the multi-key path with the dialog's
//!   `title×2 + category + searchText` weighting (scoreFn receives
//!   NORMALIZED per-key scores; the threshold is normalized too);
//! * [`go_simple`] — the no-keys ranking;
//! * the spaces search (`algorithmSpaces` with partial-match
//!   aggregation across keys), `normalizeScore`/`denormalizeScore`,
//!   the bitflag early-out, the substring/strict/simple match tiers,
//!   and the exact `FastPriorityQueue` ordering (ties included — the
//!   heap's deterministic layout, not a stable sort).
//!
// A verbatim algorithm port: match arms mirror the reference's data
// tables, integer→double casts mirror its number coercion, and the
// scorer is one long function exactly like the original — the pedantic
// lints would rewrite the port away from its source of truth.
#![allow(clippy::too_many_lines)]
#![allow(clippy::cast_precision_loss)]
#![allow(clippy::cast_possible_wrap)]
#![allow(clippy::match_same_arms)]
#![allow(clippy::needless_bool)]
#![allow(clippy::nonminimal_bool)]
#![allow(clippy::needless_for_each)]
#![allow(clippy::single_match_else)]
#![allow(clippy::manual_find_map)]

//! Divergences (documented; none reachable from the corpus):
//! * `remove_accents` decomposes precomposed Latin-1/Latin Extended
//!   letters via a table instead of full NFD (the corpus is ASCII);
//! * `toLowerCase` is Rust `to_lowercase` (differs only on U+0130-class
//!   edge cases);
//! * the prepared-target cache (with its cross-call `_score` mutation)
//!   is a pure per-call preparation — equivalent for whole calls.

/// No-match raw score (fuzzysort's `-Infinity`).
pub const NEGATIVE_INFINITY: f64 = f64::NEG_INFINITY;

fn normalize_score(score: f64) -> f64 {
    if score == NEGATIVE_INFINITY {
        return 0.0;
    }
    if score > 1.0 {
        return score;
    }
    std::f64::consts::E.powf(((-score + 1.0).powf(0.043_07) - 1.0) * -2.0)
}

fn denormalize_score(normalized: f64) -> f64 {
    if normalized == 0.0 {
        return NEGATIVE_INFINITY;
    }
    if normalized > 1.0 {
        return normalized;
    }
    1.0 - (normalized.ln() / -2.0 + 1.0).powf(1.0 / 0.043_07)
}

/// One prepared string (`prepareLowerInfo` + the original-case text
/// `prepareNextBeginningIndexes` reads): UTF-16 code units of the
/// accent-stripped text in both casings, plus the char-class bitflags
/// used as an early-out. Indices throughout are UTF-16 units, exactly
/// like the reference's `charCodeAt`/`indexOf`.
#[derive(Debug, Clone)]
struct Prepared {
    /// Accent-stripped lowercase units (the matching surface).
    lower: Vec<u16>,
    /// Accent-stripped ORIGINAL-case units — word beginnings are
    /// detected on this (upper-case transitions disappear in the
    /// lowercase form).
    original: Vec<u16>,
    bitflags: u32,
    contains_space: bool,
    space_searches: Vec<Prepared>,
}

impl Prepared {
    fn prepare(text: &str) -> Self {
        let text = remove_accents(text);
        let original: Vec<u16> = text.encode_utf16().collect();
        let lower = text.to_lowercase();
        let mut codes = Vec::new();
        let mut bitflags = 0u32;
        let mut contains_space = false;
        for unit in lower.encode_utf16() {
            codes.push(unit);
            if unit == 32 {
                contains_space = true;
                continue;
            }
            let bit = if (97..=122).contains(&unit) {
                u32::from(unit - 97)
            } else if (48..=57).contains(&unit) {
                26
            } else if unit <= 127 {
                30
            } else {
                31
            };
            bitflags |= 1 << bit;
        }
        let space_searches = if contains_space {
            let mut searches = Vec::new();
            let mut seen = std::collections::HashSet::new();
            for word in lower.split_whitespace() {
                if word.is_empty() || !seen.insert(word.to_owned()) {
                    continue;
                }
                let mut prepared = Prepared::prepare(word);
                prepared.contains_space = false;
                searches.push(prepared);
            }
            searches
        } else {
            Vec::new()
        };
        Self { lower: codes, original, bitflags, contains_space, space_searches }
    }
}

/// `remove_accents` — Latin-script runs NFD-decomposed then
/// U+0300-036F stripped (see the module docs for the table approach).
fn remove_accents(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        if matches!(ch, '\u{0300}'..='\u{036f}') {
            continue;
        }
        match decompose_latin(ch) {
            Some(base) => out.push(base),
            None => out.push(ch),
        }
    }
    out
}

/// Decompose one precomposed Latin letter to its base letter with the
/// original case.
fn decompose_latin(ch: char) -> Option<char> {
    let lower = match ch {
        'À'..='Å' | 'à'..='å' | 'Ā' | 'ā' | 'Ă' | 'ă' | 'Ą' | 'ą' => 'a',
        'Ç' | 'ç' | 'Ć' | 'ć' | 'Ĉ' | 'ĉ' | 'Ċ' | 'ċ' | 'Č' | 'č' => 'c',
        'Ď' | 'ď' => 'd',
        'È'..='Ë' | 'è'..='ë' => 'e',
        'Ē' | 'ē' | 'Ĕ' | 'ĕ' | 'Ė' | 'ė' | 'Ę' | 'ę' | 'Ě' | 'ě' => 'e',
        'Ñ' | 'ñ' | 'Ń' | 'ń' | 'Ņ' | 'ņ' | 'Ň' | 'ň' => 'n',
        'Ò'..='Ö' | 'ò'..='ö' => 'o',
        'Ō' | 'ō' | 'Ŏ' | 'ŏ' | 'Ő' | 'ő' => 'o',
        'Ù'..='Ü' | 'ù'..='ü' => 'u',
        'Ū' | 'ū' | 'Ŭ' | 'ŭ' | 'Ů' | 'ů' | 'Ű' | 'ű' | 'Ų' | 'ų' => 'u',
        'Ý' | 'ý' | 'ÿ' | 'Ŷ' | 'ŷ' => 'y',
        _ => return None,
    };
    Some(if ch.is_uppercase() { lower.to_ascii_uppercase() } else { lower })
}

/// `prepareBeginningIndexes`: word-start positions (upper-case
/// transitions and alphanum boundaries) over the ORIGINAL-case units.
fn beginning_indexes(target: &[u16]) -> Vec<usize> {
    let mut indexes = Vec::new();
    let mut was_upper = false;
    let mut was_alphanum = false;
    for (i, &code) in target.iter().enumerate() {
        let is_upper = (65..=90).contains(&code);
        let is_alphanum = is_upper || (97..=122).contains(&code) || (48..=57).contains(&code);
        let is_beginning = (is_upper && !was_upper) || !was_alphanum || !is_alphanum;
        was_upper = is_upper;
        was_alphanum = is_alphanum;
        if is_beginning {
            indexes.push(i);
        }
    }
    indexes
}

/// `prepareNextBeginningIndexes`: for each position, the next
/// beginning index at-or-after it (target length when none).
fn next_beginning_indexes(target: &[u16]) -> Vec<usize> {
    let len = target.len();
    let beginnings = beginning_indexes(target);
    let mut out = vec![len; len];
    let mut last = beginnings.first().copied();
    let mut last_i = 0usize;
    for (i, slot) in out.iter_mut().enumerate() {
        match last {
            Some(beginning) if beginning > i => *slot = beginning,
            _ => {
                last_i += 1;
                last = beginnings.get(last_i).copied();
                *slot = last.unwrap_or(len);
            }
        }
    }
    out
}

/// `indexOf` over UTF-16 slices starting at `from`.
fn find_subslice(haystack: &[u16], needle: &[u16], from: usize) -> Option<usize> {
    if needle.is_empty() || from >= haystack.len() || haystack.len() - from < needle.len() {
        return None;
    }
    haystack[from..].windows(needle.len()).position(|window| window == needle).map(|at| at + from)
}

/// The single-word algorithm (`algorithm` with `allowSpaces` bypassed):
/// simple → strict → substring tiers and the score tally. Returns the
/// raw score plus the matched indexes (the indexes feed the spaces
/// path's next-beginning mutation).
fn algorithm_single(
    search_lower: &[u16],
    target_lower: &[u16],
    next_beginnings: &[usize],
) -> Option<(f64, Vec<usize>)> {
    let search_len = search_lower.len();
    let target_len = target_lower.len();
    if search_len == 0 || target_len == 0 {
        return None;
    }

    // Simple sequential pass — removes non-matching targets ASAP.
    let mut matches_simple: Vec<usize> = Vec::with_capacity(search_len);
    let mut search_i = 0usize;
    let mut target_i = 0usize;
    loop {
        if search_lower[search_i] == target_lower[target_i] {
            matches_simple.push(target_i);
            search_i += 1;
            if search_i == search_len {
                break;
            }
        }
        target_i += 1;
        if target_i >= target_len {
            return None;
        }
    }

    // Strict pass: consecutive or word-beginning matches only, with
    // bounded backtracking.
    let mut search_i = 0usize;
    let mut success_strict = false;
    let mut matches_strict: Vec<usize> = Vec::with_capacity(search_len);
    let mut target_i =
        if matches_simple[0] == 0 { 0 } else { next_beginnings[matches_simple[0] - 1] };
    let mut backtrack_count = 0usize;
    if target_i != target_len {
        loop {
            if target_i >= target_len {
                // Push the previous search char forward from its match.
                if search_i == 0 {
                    break;
                }
                backtrack_count += 1;
                if backtrack_count > 200 {
                    break;
                }
                search_i -= 1;
                let last_match = matches_strict.pop()?;
                target_i = next_beginnings[last_match];
            } else if search_lower[search_i] == target_lower[target_i] {
                matches_strict.push(target_i);
                search_i += 1;
                if search_i == search_len {
                    success_strict = true;
                    break;
                }
                target_i += 1;
            } else {
                target_i = next_beginnings[target_i];
            }
        }
    }

    // Substring detection (a one-char search never counts).
    let mut substring_index: Option<usize> = if search_len <= 1 {
        None
    } else {
        find_subslice(target_lower, search_lower, matches_simple[0])
    };
    let is_substring = substring_index.is_some();
    let mut is_substring_beginning = match substring_index {
        None => false,
        Some(index) => index == 0 || next_beginnings[index - 1] == index,
    };
    // A substring not at a beginning index: look for one that is.
    if is_substring && !is_substring_beginning {
        let found = substring_index.unwrap_or(0);
        let mut i = 0usize;
        while i < next_beginnings.len() {
            if i > found {
                let mut matched_all = true;
                for (s, &code) in search_lower.iter().enumerate() {
                    if target_lower.get(i + s) != Some(&code) {
                        matched_all = false;
                        break;
                    }
                }
                if matched_all {
                    substring_index = Some(i);
                    is_substring_beginning = true;
                    break;
                }
            }
            i = next_beginnings[i];
        }
    }

    let calculate_score = |matches: &[usize]| -> f64 {
        let mut score: f64 = 0.0;
        let mut extra_match_group_count: f64 = 0.0;
        for i in 1..search_len {
            if matches[i] - matches[i - 1] != 1 {
                score -= matches[i] as f64;
                extra_match_group_count += 1.0;
            }
        }
        let unmatched_distance =
            matches[search_len - 1].saturating_sub(matches[0]).saturating_sub(search_len - 1);
        score -= (12.0 + unmatched_distance as f64) * extra_match_group_count;
        if matches[0] != 0 {
            score -= matches[0] as f64 * matches[0] as f64 * 0.2;
        }
        if success_strict {
            // "Bad target" penalty: too many word beginnings.
            let mut unique_beginning_indexes = 1usize;
            let mut i = next_beginnings[0];
            while i < target_len {
                unique_beginning_indexes += 1;
                i = next_beginnings[i];
            }
            if unique_beginning_indexes > 24 {
                score *= ((unique_beginning_indexes - 24) * 10) as f64;
            }
        } else {
            score *= 1000.0;
        }
        score -= (target_len - search_len) as f64 / 2.0;
        if is_substring {
            score /= 1.0 + (search_len * search_len) as f64;
        }
        if is_substring_beginning {
            score /= 1.0 + (search_len * search_len) as f64;
        }
        score -= (target_len - search_len) as f64 / 2.0;
        score
    };

    if !success_strict {
        if is_substring {
            let index = substring_index.unwrap_or(0);
            for (i, slot) in matches_simple.iter_mut().enumerate().take(search_len) {
                *slot = index + i;
            }
        }
        Some((calculate_score(&matches_simple), matches_simple))
    } else if is_substring_beginning {
        let index = substring_index.unwrap_or(0);
        for (i, slot) in matches_simple.iter_mut().enumerate().take(search_len) {
            *slot = index + i;
        }
        Some((calculate_score(&matches_simple), matches_simple))
    } else {
        Some((calculate_score(&matches_strict), matches_strict))
    }
}

/// Outcome of the spaces algorithm: the accumulated raw score and the
/// per-word shares (`allowPartialMatchScores`; `-Infinity` = the word
/// found no home in this target).
struct SpacesOutcome {
    score: f64,
    per_word: Vec<f64>,
}

/// `algorithmSpaces` for one target. `allow_partial` = a word may miss
/// this target (the keys path aggregates across keys); otherwise any
/// missing word is no-match.
fn algorithm_spaces(
    search: &Prepared,
    target_lower: &[u16],
    base_beginnings: &[usize],
    allow_partial: bool,
) -> Option<SpacesOutcome> {
    let searches = &search.space_searches;
    let searches_len = searches.len();
    let mut score: f64 = 0.0;
    let mut per_word = vec![NEGATIVE_INFINITY; searches_len];
    let mut first_seen_index_last_search = 0usize;
    let mut has_at_least_one = false;

    // The reference mutates the shared `_nextBeginningIndexes` between
    // words (a consecutive previous match's tail becomes a beginning,
    // so "straw berry" matches "strawberry"); we copy the map per call
    // and mutate the copy — same effect, no shared state.
    let mut beginnings = base_beginnings.to_vec();
    for (i, word) in searches.iter().enumerate() {
        // In partial mode a word that misses THIS target contributes
        // nothing (no share, no order dock, no map mutation); in full
        // mode any miss is no-match.
        let Some((word_score, indexes)) = algorithm_single(&word.lower, target_lower, &beginnings)
        else {
            if allow_partial {
                continue;
            }
            return None;
        };
        has_at_least_one = true;

        // Mutate the map for the next word when the match is a
        // consecutive run ending before the target end.
        if i + 1 != searches_len {
            let consecutive = indexes.windows(2).all(|pair| pair[1] - pair[0] == 1);
            if consecutive {
                let new_beginning = indexes[indexes.len() - 1] + 1;
                if new_beginning >= 1 && new_beginning - 1 < beginnings.len() {
                    let to_replace = beginnings[new_beginning - 1];
                    let mut at = new_beginning as isize - 1;
                    while at >= 0 {
                        let idx = usize::try_from(at).unwrap_or(0);
                        if to_replace != beginnings[idx] {
                            break;
                        }
                        beginnings[idx] = new_beginning;
                        at -= 1;
                    }
                }
            }
        }

        score += word_score / searches_len as f64;
        per_word[i] = word_score / searches_len as f64;

        // Order dock: words must appear in order.
        if indexes[0] < first_seen_index_last_search {
            score -= (first_seen_index_last_search - indexes[0]) as f64 * 2.0;
        }
        first_seen_index_last_search = indexes[0];
    }
    if allow_partial && !has_at_least_one {
        return None;
    }

    // A whole-phrase substring may beat the split match.
    if let Some((whole, _)) = algorithm_single(&search.lower, target_lower, base_beginnings) {
        if whole > score {
            if allow_partial {
                let share = whole / searches_len as f64;
                per_word.iter_mut().for_each(|slot| *slot = share);
            }
            return Some(SpacesOutcome { score: whole, per_word });
        }
    }
    Some(SpacesOutcome { score, per_word })
}

/// One ranked entry: the target (cloned) and its raw combined score
/// (higher = better).
#[derive(Debug, Clone)]
pub struct Ranked<T> {
    pub target: T,
    pub score: f64,
}

/// The dialog's multi-key fuzzy ranking — `fuzzysort.go(search,
/// targets, { keys, scoreFn, threshold })`.
///
/// * `keys` extract the searchable strings (missing/empty keys score
///   0, never fail the entry);
/// * `score_fn` receives the per-key NORMALIZED scores (0 = no match)
///   and returns the combined normalized score (`0`/NaN drops the
///   entry);
/// * `threshold` is normalized (`0.7` = the command palette's);
///   `None` keeps every match.
///
/// Order reproduces the reference priority queue exactly, ties
/// included.
pub fn go_keys<T: Clone>(
    search: &str,
    targets: &[T],
    keys: &[fn(&T) -> Option<String>],
    score_fn: impl Fn(&[f64]) -> f64,
    threshold: Option<f64>,
) -> Vec<Ranked<T>> {
    go_keys_with(search, targets, keys, move |_, scores| score_fn(scores), threshold)
}

/// [`go_keys`] with the target handed to the score function — for
/// scoreFns that read the entry itself (the prompt autocomplete's
/// prefix-doubling rule, e.g. `display.startsWith("/" + query)`).
pub fn go_keys_with<T: Clone>(
    search: &str,
    targets: &[T],
    keys: &[fn(&T) -> Option<String>],
    score_fn: impl Fn(&T, &[f64]) -> f64,
    threshold: Option<f64>,
) -> Vec<Ranked<T>> {
    let trimmed = search.trim();
    if trimmed.is_empty() {
        return Vec::new();
    }
    let prepared_search = Prepared::prepare(trimmed);
    let threshold_raw = denormalize_score(threshold.unwrap_or(0.0));
    let contains_space = prepared_search.contains_space;
    let mut queue: Queue<Ranked<T>> = Queue::default();

    'outer: for target in targets {
        // Prepare every key; early out on the union bitflags.
        let mut prepared_keys: Vec<Option<Prepared>> = Vec::with_capacity(keys.len());
        let mut keys_bitflags = 0u32;
        for key in keys {
            match key(target).map(|text| Prepared::prepare(&text)) {
                Some(prepared) => {
                    keys_bitflags |= prepared.bitflags;
                    prepared_keys.push(Some(prepared));
                }
                None => prepared_keys.push(None),
            }
        }
        if (prepared_search.bitflags & keys_bitflags) != prepared_search.bitflags {
            continue;
        }

        let mut key_scores: Vec<f64> = Vec::with_capacity(keys.len());
        let mut key_words: Vec<Vec<f64>> = Vec::with_capacity(keys.len());
        for slot in &prepared_keys {
            let Some(prepared) = slot else {
                key_scores.push(NEGATIVE_INFINITY);
                key_words.push(Vec::new());
                continue;
            };
            let target_lower = prepared.lower.clone();
            let beginnings = next_beginning_indexes(&prepared.original);
            if contains_space {
                match algorithm_spaces(&prepared_search, &target_lower, &beginnings, true) {
                    Some(outcome) => {
                        key_scores.push(outcome.score);
                        key_words.push(outcome.per_word);
                    }
                    None => {
                        key_scores.push(NEGATIVE_INFINITY);
                        key_words.push(Vec::new());
                    }
                }
            } else {
                let score = algorithm_single(&prepared_search.lower, &target_lower, &beginnings)
                    .map_or(NEGATIVE_INFINITY, |(score, _)| score);
                key_scores.push(score);
                key_words.push(Vec::new());
            }
        }

        if contains_space {
            // Every word must find a home in at least one key; the
            // across-keys aggregation only feeds the (here unused)
            // no-scoreFn path — the scoreFn below overwrites it, so the
            // gate is all we keep.
            let words = prepared_search.space_searches.len();
            let mut best_per_word = vec![NEGATIVE_INFINITY; words];
            for key_words in &key_words {
                for (i, share) in key_words.iter().enumerate() {
                    if *share > -1000.0 && best_per_word[i] > NEGATIVE_INFINITY {
                        let combined = (best_per_word[i] + share) / 4.0;
                        if combined > best_per_word[i] {
                            best_per_word[i] = combined;
                        }
                    }
                    if *share > best_per_word[i] {
                        best_per_word[i] = *share;
                    }
                }
            }
            if best_per_word.contains(&NEGATIVE_INFINITY) {
                continue 'outer;
            }
        } else {
            let has_match = key_scores.iter().any(|score| *score != NEGATIVE_INFINITY);
            if !has_match {
                continue;
            }
        }

        // scoreFn over normalized per-key scores (0 for missing keys).
        let normalized: Vec<f64> = key_scores.iter().map(|score| normalize_score(*score)).collect();
        let combined = score_fn(target, &normalized);
        if combined == 0.0 || combined.is_nan() {
            continue;
        }
        let raw = denormalize_score(combined);
        if raw < threshold_raw {
            continue;
        }
        queue.add((Ranked { target: target.clone(), score: raw }, raw));
    }

    let mut results = Vec::with_capacity(queue.len());
    while let Some(item) = queue.poll() {
        results.push(item);
    }
    results.reverse();
    results
}

/// The no-keys ranking (`fuzzysort.go` without keys).
pub fn go_simple<T: Clone>(
    search: &str,
    targets: &[T],
    key: impl Fn(&T) -> String,
    threshold: Option<f64>,
) -> Vec<Ranked<T>> {
    let trimmed = search.trim();
    if trimmed.is_empty() {
        return Vec::new();
    }
    let prepared_search = Prepared::prepare(trimmed);
    let threshold_raw = denormalize_score(threshold.unwrap_or(0.0));
    let mut queue: Queue<Ranked<T>> = Queue::default();
    for target in targets {
        let prepared = Prepared::prepare(&key(target));
        if (prepared_search.bitflags & prepared.bitflags) != prepared_search.bitflags {
            continue;
        }
        let target_lower = prepared.lower;
        let beginnings = next_beginning_indexes(&prepared.original);
        let score = if prepared_search.contains_space {
            algorithm_spaces(&prepared_search, &target_lower, &beginnings, false)
                .map_or(NEGATIVE_INFINITY, |outcome| outcome.score)
        } else {
            algorithm_single(&prepared_search.lower, &target_lower, &beginnings)
                .map_or(NEGATIVE_INFINITY, |(score, _)| score)
        };
        if score == NEGATIVE_INFINITY || score < threshold_raw {
            continue;
        }
        queue.add((Ranked { target: target.clone(), score }, score));
    }
    let mut results = Vec::with_capacity(queue.len());
    while let Some(item) = queue.poll() {
        results.push(item);
    }
    results.reverse();
    results
}

/// The reference's `FastPriorityQueue` (a min-heap on score), ported
/// verbatim — including its deterministic tie layout — so ranked order
/// with equal scores matches the reference exactly.
struct Queue<T> {
    /// `(item, score)` pairs; `items[0]` holds the minimum score.
    items: Vec<(T, f64)>,
}

impl<T> Default for Queue<T> {
    fn default() -> Self {
        Self { items: Vec::new() }
    }
}

impl<T: Clone> Queue<T> {
    fn len(&self) -> usize {
        self.items.len()
    }

    /// `add`: push, bubble up while strictly smaller than the parent.
    fn add(&mut self, item: (T, f64)) {
        let mut at = self.items.len();
        self.items.push(item);
        while at > 0 {
            let parent = (at - 1) >> 1;
            if self.items[at].1 >= self.items[parent].1 {
                break;
            }
            self.items.swap(at, parent);
            at = parent;
        }
    }

    /// `poll`: take the root, move the last item to the root, then
    /// percolate down with the reference's hole algorithm.
    fn poll(&mut self) -> Option<T> {
        if self.items.is_empty() {
            return None;
        }
        let root = self.items.swap_remove(0);
        if self.items.is_empty() {
            return Some(root.0);
        }
        self.percolate_down();
        Some(root.0)
    }

    fn percolate_down(&mut self) {
        // The reference saves the root value aside, shifts children up
        // into the hole chain, then bubbles the saved value up from
        // the final hole.
        let value = self.items[0].clone();
        let len = self.items.len();
        let mut at = 0usize;
        let mut child = 1usize;
        while child < len {
            let mut chosen = child;
            let sibling = child + 1;
            if sibling < len && self.items[sibling].1 < self.items[child].1 {
                chosen = sibling;
            }
            // Move the chosen child up into its parent's slot.
            let parent_slot = (chosen - 1) >> 1;
            self.items[parent_slot] = self.items[chosen].clone();
            at = chosen;
            child = 1 + (chosen << 1);
        }
        let mut cursor = at;
        while cursor > 0 {
            let parent = (cursor - 1) >> 1;
            if value.1 >= self.items[parent].1 {
                break;
            }
            self.items[cursor] = self.items[parent].clone();
            cursor = parent;
        }
        self.items[cursor] = value;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, Clone, serde::Deserialize)]
    struct CorpusTarget {
        id: String,
        title: String,
        category: String,
    }

    #[derive(Debug, serde::Deserialize)]
    struct CorpusQuery {
        query: String,
        expected: Vec<String>,
    }

    #[derive(Debug, serde::Deserialize)]
    struct Corpus {
        queries: Vec<CorpusQuery>,
        targets: Vec<CorpusTarget>,
    }

    /// The fuzzy ordering contract, pinned against the REFERENCE
    /// fuzzysort: `fuzzy_corpus.json` was generated by
    /// `tools/parity/tui-dump/fuzzy_corpus.ts` running the reference
    /// scorer over the production palette registry with the dialog's
    /// `title×2 + category + searchText` weighting and the palette's
    /// `0.7` threshold. Order — including ties — must match exactly.
    #[test]
    fn golden_query_orders_match_the_reference_fuzzysort() {
        let corpus: Corpus =
            serde_json::from_str(include_str!("fuzzy_corpus.json")).expect("corpus fixture parses");
        assert!(!corpus.queries.is_empty());
        let keys: &[fn(&CorpusTarget) -> Option<String>] = &[
            |target: &CorpusTarget| Some(target.title.clone()),
            |target: &CorpusTarget| Some(target.category.clone()),
            |target: &CorpusTarget| Some(target.id.clone()),
        ];
        for entry in &corpus.queries {
            let ranked = go_keys(
                &entry.query.to_lowercase(),
                &corpus.targets,
                keys,
                |scores| scores[0] * 2.0 + scores[1] + scores[2],
                Some(0.7),
            );
            let got: Vec<&str> = ranked.iter().map(|item| item.target.id.as_str()).collect();
            let want: Vec<&str> = entry.expected.iter().map(String::as_str).collect();
            assert_eq!(got, want, "query {:?}", entry.query);
        }
    }

    /// The spaces path pins the reference's own example: "c man"
    /// ranks CheatManager.h over Manifest.cpp (the order dock — words
    /// must appear in order). Verified against the reference scorer.
    #[test]
    fn spaces_order_dock_ranks_in_order_matches_first() {
        let targets =
            ["CheatManager.h".to_owned(), "Manifest.cpp".to_owned(), "ChatManifold.tsx".to_owned()];
        let ranked = go_simple("c man", &targets, ToString::to_string, None);
        let order: Vec<&str> = ranked.iter().map(|item| item.target.as_str()).collect();
        assert_eq!(
            order,
            vec!["CheatManager.h", "ChatManifold.tsx", "Manifest.cpp"],
            "order: {order:?}"
        );
    }

    #[test]
    fn missing_search_or_target_is_empty() {
        let targets = vec!["alpha".to_owned()];
        assert!(go_simple("", &targets, ToString::to_string, None).is_empty());
        assert!(go_simple("zzz", &targets, ToString::to_string, None).is_empty());
    }
}

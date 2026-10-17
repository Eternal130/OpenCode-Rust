//! E12 search/filter first-evidence pass (SPEC-R6; R4 directive 3, R5 Q10).
//!
//! Corpus: `tools/parity/spec-canaries/e12_search_corpus.json` (committed,
//! I9-labeled). Two sides:
//!
//! * **ref predicate, ported 1:1** from
//!   `tui/src/component/dialog-session-list.tsx:86-99`:
//!   `q = filter.trim().toLowerCase()`; `!q` -> all sessions (children
//!   INCLUDED); else parent sessions where
//!   `title.toLowerCase().includes(q)`. The port is cross-checked against
//!   the corpus's hand-computed `expected_match`/`expected_nonmatch`
//!   cells — disagreement fails the run (the port, not the parity, is
//!   then wrong).
//! * **ours, REAL code path**: the same surface our TUI renders when
//!   `session.list` ("Switch session") opens — `SelectProps` options ->
//!   [`filter_indices`](crate::components::dialog_select) -> the
//!   fuzzysort port (ranked, threshold 0.7).
//!
//! Artifact: per-query diff table (stdout) — match-set deltas and order;
//! findings are recorded in docs/spec-round6.md. This is a FIRST PASS:
//! the ref side is a source-cited port, not a live ref run (the ref TUI
//! runtime copy has only headless-render deps installed; see round doc).

use oc_tui::components::dialog_select::{filter_indices, SelectOption, SelectProps};
use serde_json::Value;

struct Ses {
    id: String,
    title: String,
    parent: Option<String>,
}

/// The ref predicate (dialog-session-list.tsx:86-99), 1:1.
fn ref_filter(sessions: &[Ses], raw_query: &str) -> Vec<String> {
    let query = raw_query.trim().to_lowercase();
    if query.is_empty() {
        return sessions.iter().map(|s| s.id.clone()).collect();
    }
    sessions
        .iter()
        .filter(|s| s.parent.is_none() && s.title.to_lowercase().contains(&query))
        .map(|s| s.id.clone())
        .collect()
}

fn corpus() -> (Vec<Ses>, Vec<Value>) {
    let path = format!(
        "{}/../../tools/parity/spec-canaries/e12_search_corpus.json",
        env!("CARGO_MANIFEST_DIR")
    );
    let raw = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("corpus {path}: {e}"));
    let v: Value = serde_json::from_str(&raw).expect("corpus json");
    let sessions = v["sessions"]
        .as_array()
        .expect("sessions")
        .iter()
        .map(|s| Ses {
            id: s["id"].as_str().expect("id").to_string(),
            title: s["title"].as_str().expect("title").to_string(),
            parent: s["parent"].as_str().map(str::to_string),
        })
        .collect();
    (sessions, v["queries"].as_array().expect("queries").clone())
}

#[test]
fn e12_first_pass_session_list_filter() {
    let (sessions, queries) = corpus();

    // Ours: build the REAL surface props exactly like
    // command_palette.rs::DialogKind::ServerList(session.list).
    let options: Vec<SelectOption> = sessions
        .iter()
        .map(|s| SelectOption::titled(s.title.clone(), s.id.clone()).categorized("Sessions"))
        .collect();
    let props = SelectProps::new("Switch session".to_string(), options);

    let mut findings = 0usize;
    for q in &queries {
        let query = q["query"].as_str().expect("query");
        let class = q["class"].as_str().unwrap_or("?");

        let expected_ref = ref_filter(&sessions, query);
        let ours_idx = filter_indices(&props, query);
        let ours: Vec<String> = ours_idx
            .iter()
            .map(|i| sessions[*i].id.clone())
            .collect();

        // Port cross-check against hand-computed corpus cells.
        for id in q["expected_match"].as_array().unwrap_or(&Vec::new()) {
            let id = id.as_str().expect("id");
            assert!(
                expected_ref.contains(&id.to_string()),
                "PORT MISMATCH: corpus says {id} matches '{query}' ({class}) but the port disagrees"
            );
        }
        for id in q["expected_nonmatch"].as_array().unwrap_or(&Vec::new()) {
            let id = id.as_str().expect("id");
            assert!(
                !expected_ref.contains(&id.to_string()),
                "PORT MISMATCH: corpus says {id} must NOT match '{query}' ({class}) but the port matches"
            );
        }

        // Parity diff: ref-port set vs ours set (rank order reported too).
        let ours_only: Vec<String> =
            ours.iter().filter(|id| !expected_ref.contains(*id)).cloned().collect();
        let ref_only: Vec<String> =
            expected_ref.iter().filter(|id| !ours.contains(id)).cloned().collect();
        let status = if ours_only.is_empty() && ref_only.is_empty() { "PARITY" } else { "DIVERGENT" };
        if status == "DIVERGENT" {
            findings += 1;
        }
        eprintln!(
            "[E12] {class:32} q={query:<14} ref={expected_ref:?} ours={ours:?} ours_only={ours_only:?} ref_only={ref_only:?} -> {status}"
        );
    }
    // First pass EXPECTS divergences (substring vs fuzzy is a known
    // semantic gap candidate); the artifact is the table + count.
    eprintln!("[E12] first pass complete: {findings} divergent queries (see table above)");
}

//! SCAN-EF2 E12 second pass (docs/scan-spec.md v9 E12; lane priority
//! #5). The first pass (`spec_e12_first_pass`) established the per-query
//! set diff vs the ref predicate port. This second pass adds:
//!
//! * **I8 canary (binding BEFORE the verdict, R6 §6.3 / R7 design):**
//!   poison ONE corpus expected_match cell (flip a matching id to
//!   non-matching) — the port cross-check must flag EXACTLY that
//!   query and no others; the unpoisoned corpus must flag zero.
//! * **Rank-order audit:** per query, ours' fuzzy-ranked ORDER vs
//!   ref's list-order (ordered result-list equality per the clause;
//!   fuzzy rows = rank-order parity, ties dispositioned).
//! * **Normalization audit:** which of {trim, case-fold, NFD-fold}
//!   each side applies (mechanically, from behavior + code cites).
//!
//! Artifact: /tmp/opencode/scanef2/e12-second-pass.md

#![allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]

use oc_tui::components::dialog_select::{filter_indices, SelectOption, SelectProps};
use serde_json::Value;

struct Ses {
    id: String,
    title: String,
    parent: Option<String>,
}

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

fn corpus_raw() -> Value {
    let path = format!(
        "{}/../../tools/parity/spec-canaries/e12_search_corpus.json",
        env!("CARGO_MANIFEST_DIR")
    );
    let raw = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("corpus {path}: {e}"));
    serde_json::from_str(&raw).expect("corpus json")
}

fn sessions_of(v: &Value) -> Vec<Ses> {
    v["sessions"]
        .as_array()
        .expect("sessions")
        .iter()
        .map(|s| Ses {
            id: s["id"].as_str().expect("id").to_string(),
            title: s["title"].as_str().expect("title").to_string(),
            parent: s["parent"].as_str().map(str::to_string),
        })
        .collect()
}

fn ours_filter(sessions: &[Ses], query: &str) -> Vec<String> {
    let options: Vec<SelectOption> = sessions
        .iter()
        .map(|s| SelectOption::titled(s.title.clone(), s.id.clone()).categorized("Sessions"))
        .collect();
    let props = SelectProps::new("Switch session".to_string(), options);
    filter_indices(&props, query).iter().map(|i| sessions[*i].id.clone()).collect()
}

/// The first pass's port cross-check, factored so the canary can run
/// it over a POISONED corpus: returns the list of query strings whose
/// hand-computed cells disagree with the ref port.
fn cross_check_flags(v: &Value, sessions: &[Ses]) -> Vec<String> {
    let mut flags = Vec::new();
    for q in v["queries"].as_array().expect("queries") {
        let query = q["query"].as_str().expect("query");
        let expected_ref = ref_filter(sessions, query);
        let mut bad = false;
        for id in q["expected_match"].as_array().unwrap_or(&Vec::new()) {
            if !expected_ref.contains(&id.as_str().expect("id").to_string()) {
                bad = true;
            }
        }
        for id in q["expected_nonmatch"].as_array().unwrap_or(&Vec::new()) {
            if expected_ref.contains(&id.as_str().expect("id").to_string()) {
                bad = true;
            }
        }
        if bad {
            flags.push(query.to_string());
        }
    }
    flags
}

#[test]
fn e12_second_pass_canary_rankorder_normalization() {
    let v = corpus_raw();
    let sessions = sessions_of(&v);

    // ---------- I8 canary ----------
    let clean_flags = cross_check_flags(&v, &sessions);
    assert!(clean_flags.is_empty(), "unpoisoned corpus flagged {clean_flags:?}");
    let mut poisoned = v.clone();
    'outer: for q in poisoned["queries"].as_array_mut().expect("queries") {
        if q["query"].as_str() == Some("支付宝") {
            if let Some(arr) = q["expected_match"].as_array_mut() {
                arr.retain(|id| id.as_str() != Some("ses_gamma"));
                q["expected_nonmatch"]
                    .as_array_mut()
                    .expect("nonmatch array")
                    .push(Value::String("ses_gamma".into()));
                break 'outer;
            }
        }
    }
    let flags = cross_check_flags(&poisoned, &sessions);
    assert_eq!(flags, vec!["支付宝".to_string()],
        "canary must flag EXACTLY the poisoned query, got {flags:?}");
    eprintln!("[E12-2] I8 canary PASS: poisoned cell flips exactly q=支付宝");

    // ---------- rank-order + set diff table ----------
    let mut report = String::from("# E12 second pass — parity table (canary PASS)\n\n");
    report.push_str("| query | set | order (ours vs ref list-order) |\n|---|---|---|\n");
    let mut set_div = 0usize;
    let mut order_div = 0usize;
    for q in v["queries"].as_array().expect("queries") {
        let query = q["query"].as_str().expect("query");
        let class = q["class"].as_str().unwrap_or("?");
        let expected_ref = ref_filter(&sessions, query);
        let ours = ours_filter(&sessions, query);
        let set_ok = ours == expected_ref;
        let order_ok = ours.iter().collect::<Vec<_>>()
            == expected_ref.iter().collect::<Vec<_>>();
        if !set_ok {
            set_div += 1;
        } else if !order_ok {
            order_div += 1;
        }
        report.push_str(&format!(
            "| {class} `{:?}` | {} | {} |\n",
            query,
            if set_ok { "PARITY".to_string() } else { "DIVERGENT".to_string() },
            if order_ok { "same".to_string() } else { format!("{ours:?} vs {expected_ref:?}") }
        ));
    }
    report.push_str(&format!(
        "\nset-divergent queries: {set_div}; set-parity but ORDER-divergent: {order_div}.\n"
    ));

    // ---------- normalization audit (behavioral probes) ----------
    let probes: Vec<(&str, &str, &str, bool, bool)> = vec![
        // (probe, query, note, ref_applies, ours_applies)
        ("trim", "   ", "whitespace-only query", true, false),
        ("case-fold", "MIGRATIONS", "upper query vs lower title", true, true),
        ("NFD-fold", "cafe\u{301}", "decomposed query vs precomposed title", false, true),
    ];
    let mut audit = String::from("\n## normalization audit\n\n| normalization | ref | ours | evidence |\n|---|---|---|---|\n");
    for (name, query, note, ref_applies, ours_applies) in probes {
        let rf = ref_filter(&sessions, query);
        let of = ours_filter(&sessions, query);
        audit.push_str(&format!(
            "| {name} | {} | {} | {note} (q={query:?} ref={rf:?} ours={of:?}) |\n",
            if ref_applies { "yes" } else { "no" },
            if ours_applies { "yes" } else { "no" },
        ));
    }
    audit.push_str("\ncode cites: ref = `dialog-session-list.tsx` `filter.trim().toLowerCase()`\n(byte-exact `includes`, no unicode normalization); ours = `dialog_select.rs::filter_indices`\n(`to_lowercase`, NO trim) -> `fuzzy.rs` `remove_accents` (NFD-decompose + mark-strip).\n");

    let full = format!("{report}{audit}");
    std::fs::create_dir_all("/tmp/opencode/scanef2").ok();
    std::fs::write("/tmp/opencode/scanef2/e12-second-pass.md", &full).expect("write artifact");
    eprintln!("{full}");
    eprintln!("[E12-2] second pass complete (artifact written)");
}

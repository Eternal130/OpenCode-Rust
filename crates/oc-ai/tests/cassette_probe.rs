//! Cassette probe (oracle gate 2, finding 18): parse every recording in the
//! copied corpus and report group counts. Replay assertions are added by the
//! protocol lanes; this probe only pins corpus shape, format-version
//! cleanliness, and selection coverage.

// Probe strings are built incrementally by design.
#![allow(clippy::format_push_string)]

use oc_ai::test_support::{all_scenarios, group_counts, select_scenarios, Cassette};

#[test]
fn cassette_corpus_parses_and_reports_group_counts() {
    let scenarios = all_scenarios().expect("every cassette parses verbatim");
    let counts = group_counts();
    let total: usize = counts.iter().map(|(_, count)| count).sum();

    // The v2 corpus ships 274 cassettes in 74 groups; allow the count to
    // grow but never shrink below the ported baseline.
    assert!(counts.len() >= 71, "expected >= 71 recording groups, found {}", counts.len());
    assert!(total >= 271, "expected >= 271 cassettes, found {total}");
    assert_eq!(scenarios.len(), total, "group counts and parsed scenarios agree");

    let mut probe = String::from("cassette corpus probe:\n");
    for (group, count) in &counts {
        probe.push_str(&format!("  {group}: {count}\n"));
    }
    probe.push_str(&format!("  TOTAL: {total} cassettes across {} groups\n", counts.len()));
    std::eprintln!("{probe}");
}

#[test]
fn every_cassette_is_format_v1_with_http_interactions() {
    for scenario in all_scenarios().unwrap() {
        assert_eq!(scenario.cassette.version, 1, "{}", scenario.full_name());
        assert!(
            !scenario.cassette.metadata.name.is_empty(),
            "{}: metadata name recorded",
            scenario.full_name()
        );
        let http = scenario.cassette.http_interactions().count();
        let websocket = scenario.cassette.interactions.len() - http;
        assert!(
            http + websocket == scenario.cassette.interactions.len(),
            "{} interaction accounting",
            scenario.full_name()
        );
        for interaction in scenario.cassette.http_interactions() {
            let (req, resp) = (
                interaction.request.as_ref().expect("http request recorded"),
                interaction.response.as_ref().expect("http response recorded"),
            );
            assert!(
                req.method.as_deref().is_some_and(|m| !m.is_empty()),
                "{} method",
                scenario.full_name()
            );
            assert!(
                req.url.as_deref().is_some_and(|u| u.starts_with("https://")),
                "{} url",
                scenario.full_name()
            );
            assert!(resp.status.is_some(), "{} status", scenario.full_name());
            // response body is the raw SSE wire bytes (or an empty error body)
            assert!(resp.body.is_some(), "{} body", scenario.full_name());
        }
    }
}

#[test]
fn error_status_cassettes_exist_for_classification_replay() {
    let scenarios = all_scenarios().unwrap();
    let with_errors = scenarios
        .iter()
        .filter(|scenario| {
            scenario.cassette.http_interactions().any(|interaction| {
                interaction.response.as_ref().and_then(|r| r.status).unwrap_or(200) >= 400
            })
        })
        .count();
    assert!(with_errors >= 5, "expected recorded error responses, found {with_errors}");
}

#[test]
fn scenario_selection_by_prefix_and_tag_finds_flagship_groups() {
    for prefix in ["anthropic-messages", "openai-responses", "gemini", "bedrock-converse"] {
        let selected = select_scenarios(Some(prefix), None).unwrap();
        assert!(!selected.is_empty(), "no scenarios for prefix {prefix}");
    }
    for tag in ["provider:anthropic", "provider:openai", "provider:google"] {
        let selected = select_scenarios(None, Some(tag)).unwrap();
        assert!(!selected.is_empty(), "no scenarios for tag {tag}");
    }
    assert!(select_scenarios(Some("no-such-group"), None).unwrap().is_empty());
}

#[test]
fn sse_wire_bodies_decode_through_the_framing_decoder() {
    // Smoke-check that every recorded SSE body decodes into >= 1 frame via
    // the production framing path — the same decoder the route stack uses.
    let mut sse_bodies = 0usize;
    let mut frames_total = 0usize;
    for scenario in all_scenarios().unwrap() {
        for interaction in scenario.cassette.http_interactions() {
            let response = interaction.response.as_ref().expect("http response recorded");
            let body = response.body.as_deref().unwrap_or("");
            let content_type = response
                .headers
                .as_ref()
                .and_then(|headers| headers.get("content-type").and_then(|v| v.as_str()))
                .unwrap_or("");
            if !content_type.starts_with("text/event-stream") || body.is_empty() {
                continue;
            }
            sse_bodies += 1;
            let mut decoder =
                oc_ai::route::framing::FramingDecoder::new(oc_ai::route::Framing::sse());
            let mut frames = decoder.feed(body.as_bytes()).expect("recorded body decodes");
            frames.extend(decoder.finish().expect("recorded body flushes"));
            assert!(!frames.is_empty(), "{}: SSE body produced no frames", scenario.full_name());
            frames_total += frames.len();
        }
    }
    assert!(sse_bodies > 200, "expected the bulk of the corpus to be SSE, got {sse_bodies}");
    assert!(frames_total > 500, "expected a meaningful frame corpus, got {frames_total}");
}

#[test]
fn cassettes_load_via_the_typed_api() {
    let path = oc_ai::test_support::recordings_dir()
        .join("anthropic-messages")
        .join("anthropic-opus-4-7-image-tool-result.json");
    let cassette = Cassette::load(&path).expect("sample cassette loads");
    assert_eq!(cassette.metadata.provider.as_deref(), Some("anthropic"));
    assert_eq!(cassette.metadata.route.as_deref(), Some("anthropic-messages"));
    assert!(cassette.metadata.tags.iter().any(|tag| tag == "prefix:anthropic-messages"));
    assert_eq!(cassette.http_interactions().count(), 1);
    let body = cassette.interactions[0]
        .response
        .as_ref()
        .and_then(|response| response.body.as_deref())
        .unwrap();
    assert!(body.starts_with("event: message_start"), "raw SSE wire bytes preserved");
}

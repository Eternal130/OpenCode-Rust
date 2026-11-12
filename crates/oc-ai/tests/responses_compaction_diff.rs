//! `/compact` compaction-pipeline differential driver (rust side).
//!
//! Env-gated (zero cost when unset; nextest sees a passing no-op):
//!   RESPONSES_COMPACTION_DIFF_CORPUS=<dir> RESPONSES_COMPACTION_DIFF_OUT=<file> \
//!     cargo nextest run -p oc-ai --test responses_compaction_diff
//!
//! Drives the production [`parse_compaction_response`] (the item → `Message`
//! pure-function family of `protocols/utils/responses_compaction.rs`) over the
//! committed corpus and emits the same normalized wire shape as
//! `tools/diff/drivers/compaction_ref.ts`: accept/reject (coarse buckets) +
//! `{role, parts, providerMetadata}` messages + usage.
//!
//! Normalization (declared in the report): provider-metadata nulls are
//! dropped (TS drops `undefined` keys, rust writes JSON nulls); usage raw
//! passthrough is stripped as generated-source noise.

use oc_ai::protocols::utils::responses_compaction::parse_compaction_response;
use oc_ai::schema::media::Source;
use oc_ai::schema::{ContentPart, Message, MessageRole};
use oc_ai::test_support::{request_for_tests, static_route_for_tests};
use serde_json::{json, Value};

fn role_str(role: &MessageRole) -> &'static str {
    match role {
        MessageRole::System => "system",
        MessageRole::User => "user",
        MessageRole::Assistant => "assistant",
        MessageRole::Tool => "tool",
    }
}

/// Drop null-valued keys recursively (TS `undefined` parity).
fn strip_nulls(value: &Value) -> Value {
    match value {
        Value::Array(items) => Value::Array(items.iter().map(strip_nulls).collect()),
        Value::Object(map) => Value::Object(
            map.iter()
                .filter(|(_, v)| !v.is_null())
                .map(|(k, v)| (k.clone(), strip_nulls(v)))
                .collect(),
        ),
        other => other.clone(),
    }
}

fn norm_meta(meta: &oc_ai::schema::ProviderMetadata) -> Value {
    strip_nulls(&serde_json::to_value(meta).unwrap_or(Value::Null))
}

fn norm_media(asset: &oc_ai::schema::media::Asset) -> Value {
    match &asset.source {
        Source::Base64 { data, media_type } =>
            json!({ "source": "base64", "mediaType": media_type, "data": data }),
        Source::Url { url, media_type, .. } =>
            json!({ "source": "url", "mediaType": media_type, "url": url }),
        Source::Bytes { .. } => json!({ "source": "bytes" }),
        Source::Ref { .. } => json!({ "source": "ref" }),
    }
}

fn norm_part(part: &ContentPart) -> Value {
    match part {
        ContentPart::Text { text, .. } => json!({ "type": "text", "text": text }),
        ContentPart::Reasoning { text, provider_metadata, .. } => json!({
            "type": "reasoning",
            "text": text,
            "providerMetadata": provider_metadata.as_ref().map(norm_meta),
        }),
        ContentPart::Media { media, filename, provider_metadata, .. } => json!({
            "type": "media",
            "media": norm_media(media),
            "filename": filename,
            "providerMetadata": provider_metadata.as_ref().map(norm_meta),
        }),
        ContentPart::Compaction { provider, id, encrypted, .. } => json!({
            "type": "compaction",
            "provider": provider.0,
            "id": id,
            "encrypted": encrypted,
        }),
        other => json!({ "type": other.kind() }),
    }
}

fn norm_message(message: &Message) -> Value {
    json!({
        "role": role_str(&message.role),
        "parts": message.content.iter().map(norm_part).collect::<Vec<_>>(),
        "providerMetadata": message.provider_metadata.as_ref().map(norm_meta),
    })
}

fn norm_usage(usage: &oc_ai::schema::Usage) -> Value {
    json!({
        "inputTokens": usage.input_tokens,
        "outputTokens": usage.output_tokens,
        "nonCachedInputTokens": usage.non_cached_input_tokens,
        "cacheReadInputTokens": usage.cache_read_input_tokens,
        "cacheWriteInputTokens": usage.cache_write_input_tokens,
        "reasoningTokens": usage.reasoning_tokens,
        "totalTokens": usage.total_tokens,
    })
}

/// Coarse reject buckets mirroring the ref driver's classification:
/// serde syntax errors carry `at line N`; the bare message is the
/// object-literal rejection; the checkpoint scan runs before item mapping.
fn reject_bucket(message: &str) -> &'static str {
    if message.contains(" at line ") {
        "json"
    } else if message == "Invalid compaction response" {
        "object"
    } else if message.contains("did not contain a checkpoint") {
        "no-checkpoint"
    } else {
        "item-schema"
    }
}

#[test]
fn responses_compaction_diff() {
    let Some(corpus) = std::env::var_os("RESPONSES_COMPACTION_DIFF_CORPUS") else {
        return; // no corpus staged: driver inactive
    };
    let out = std::path::PathBuf::from(
        std::env::var_os("RESPONSES_COMPACTION_DIFF_OUT").expect("RESPONSES_COMPACTION_DIFF_OUT"),
    );
    let corpus = std::path::PathBuf::from(corpus);

    let mut names: Vec<String> = std::fs::read_dir(&corpus)
        .expect("read corpus dir")
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.ends_with(".json"))
        .collect();
    names.sort();

    let route = static_route_for_tests();
    let request = request_for_tests(Vec::new());

    let mut results: Vec<Value> = Vec::new();
    for name in names {
        let text = std::fs::read_to_string(corpus.join(&name)).expect("read case");
        let mut rec = json!({ "name": name });
        match parse_compaction_response(&text, &route, &request) {
            Err(error) => {
                rec["accept"] = json!(false);
                rec["reject"] = json!(reject_bucket(&error.message()));
            }
            Ok(parsed) => {
                rec["accept"] = json!(true);
                rec["reject"] = Value::Null;
                rec["messages"] =
                    Value::Array(parsed.replacement.iter().map(norm_message).collect());
                rec["usage"] = parsed.usage.as_ref().map(norm_usage).unwrap_or(Value::Null);
            }
        }
        results.push(rec);
    }

    std::fs::write(&out, serde_json::to_vec(&results).expect("serialize")).expect("write out");
    println!("rust driver: {} cases -> {}", results.len(), out.display());
}

//! JSON-Schema validator differential driver (rust side, channel 4).
//!
//! Env-gated (zero cost when unset; nextest sees a passing no-op):
//!   JSON_SCHEMA_DIFF_CORPUS=<dir> JSON_SCHEMA_DIFF_OUT=<file> \
//!     cargo nextest run -p oc-ai --test json_schema_diff_driver
//!
//! Feeds every (schema, instance) pair from the committed corpus to
//! `oc_ai::json_schema::validate_input` (the shared G-59/G-100/G-230
//! engine) and records the accept/reject verdict. Output schema matches
//! tools/diff/drivers/json_schema_ref.ts exactly.

use std::path::PathBuf;

#[test]
fn json_schema_diff_driver() {
    let Some(corpus) = std::env::var_os("JSON_SCHEMA_DIFF_CORPUS") else {
        return; // no corpus staged: driver inactive
    };
    let out = PathBuf::from(std::env::var_os("JSON_SCHEMA_OUT").or(std::env::var_os("JSON_SCHEMA_DIFF_OUT")).expect("JSON_SCHEMA_DIFF_OUT"));
    let corpus = PathBuf::from(corpus);

    let mut names: Vec<String> = std::fs::read_dir(&corpus)
        .expect("read corpus dir")
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.ends_with(".json"))
        .collect();
    names.sort();

    let mut results: Vec<serde_json::Value> = Vec::new();
    for name in names {
        let doc: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(corpus.join(&name)).expect("read case")).expect("parse case");
        let case = doc["name"].as_str().unwrap_or(name.as_str()).to_string();
        let keywords: Vec<String> = doc["keywords"]
            .as_array()
            .map(|a| a.iter().filter_map(|k| k.as_str().map(str::to_string)).collect())
            .unwrap_or_default();
        let schema = doc["schema"].clone();
        for inst in doc["instances"].as_array().into_iter().flatten() {
            let label = inst["label"].as_str().unwrap_or("?").to_string();
            let value = &inst["value"];
            let issues = oc_ai::json_schema::validate_input(&schema, value);
            results.push(serde_json::json!({
                "name": format!("{case}/{label}"),
                "case": case,
                "label": label,
                "keywords": keywords,
                "accept": issues.is_empty(),
                "codec_ok": true,
            }));
        }
    }

    std::fs::write(&out, serde_json::to_vec(&results).expect("serialize")).expect("write out");
    println!("rust driver: {} pairs -> {}", results.len(), out.display());
}

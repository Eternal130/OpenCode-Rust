//! Config-parse differential driver (rust side).
//!
//! Env-gated (zero cost when unset; nextest sees a passing no-op):
//!   CONFIG_DIFF_CORPUS=<dir> CONFIG_DIFF_OUT=<file> \
//!     cargo nextest run -p oc-core --test config_diff_driver
//!
//! Mirrors `parse_info` (config.rs:576-613): jsonc-parse → normalize →
//! final `serde_json::from_value::<Info>` decode. Output schema matches
//! tools/diff/drivers/config_ref.ts exactly.

use std::path::PathBuf;

#[test]
fn config_diff_driver() {
    let Some(corpus) = std::env::var_os("CONFIG_DIFF_CORPUS") else {
        return // no corpus staged: driver inactive
    };
    let out = PathBuf::from(std::env::var_os("CONFIG_DIFF_OUT").expect("CONFIG_DIFF_OUT"));
    let corpus = PathBuf::from(corpus);

    let mut names: Vec<String> = std::fs::read_dir(&corpus)
        .expect("read corpus dir")
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.ends_with(".json") || n.ends_with(".jsonc"))
        .collect();
    names.sort();

    let mut results: Vec<serde_json::Value> = Vec::new();
    for name in names {
        let text = std::fs::read_to_string(corpus.join(&name)).expect("read case");
        let mut rec = serde_json::json!({ "name": name });
        match oc_core::config::jsonc_parse::parse_jsonc(&text) {
            Err(_) => {
                rec["json_parse_ok"] = serde_json::json!(false);
                rec["normalize"] = serde_json::json!("not-reached");
                rec["diagnostics"] = serde_json::json!([]);
                rec["decode_ok"] = serde_json::json!(false);
            }
            Ok(value) => {
                rec["json_parse_ok"] = serde_json::json!(true);
                let json = oc_core::config::Json::from(value);
                match oc_core::config::normalize::normalize(&json) {
                    oc_core::config::NormalizeResult::Normalized { encoded, diagnostics } => {
                        rec["normalize"] = serde_json::json!("normalized");
                        rec["diagnostics"] = serde_json::to_value(
                            diagnostics
                                .iter()
                                .map(|d| serde_json::json!({"kind": format!("{:?}", d.kind), "path": d.path}))
                                .collect::<Vec<_>>(),
                        )
                        .unwrap();
                        let encoded_value = serde_json::Value::from(encoded);
                        let decode_ok =
                            serde_json::from_value::<oc_schema::config::Info>(encoded_value.clone()).is_ok();
                        rec["encoded"] = encoded_value;
                        rec["decode_ok"] = serde_json::json!(decode_ok);
                    }
                    oc_core::config::NormalizeResult::Rejected { diagnostics } => {
                        rec["normalize"] = serde_json::json!("rejected");
                        rec["diagnostics"] = serde_json::to_value(
                            diagnostics
                                .iter()
                                .map(|d| serde_json::json!({"kind": format!("{:?}", d.kind), "path": d.path}))
                                .collect::<Vec<_>>(),
                        )
                        .unwrap();
                        rec["decode_ok"] = serde_json::json!(false);
                    }
                }
            }
        }
        results.push(rec);
    }

    std::fs::write(&out, serde_json::to_vec(&results).expect("serialize")).expect("write out");
    println!("rust driver: {} cases -> {}", results.len(), out.display());
}

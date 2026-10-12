//! `/api/model` listing parity (lane/models #9): the server serves
//! v2's AVAILABLE set — the keyless `opencode` zen free tier plus
//! config-defined custom providers (with their display names and the
//! openai-compatible effort variants) — never the whole bundled
//! catalog. Hermetic: sandboxed roots, port 0, no network (the custom
//! provider's baseURL is unreachable but never called).

use std::path::PathBuf;
use std::time::Duration;

use tokio::io::AsyncBufReadExt as _;

/// The parity rig's exact provider shape (`tools/parity/src/tui_diff.rs`
/// `seed_project`): one openai-compatible custom provider, one named
/// model, `model` pinned to it.
const PARITY_CONFIG: &str = r#"{
    "$schema": "https://opencode.ai/config.json",
    "model": "parityfake/fake-model",
    "providers": {
        "parityfake": {
            "package": "@opencode/ai/providers/openai-compatible",
            "settings": { "baseURL": "http://127.0.0.1:9/v1" },
            "models": { "fake-model": { "name": "Fake Model" } }
        }
    }
}"#;

struct SandboxDrop(PathBuf);
impl Drop for SandboxDrop {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[tokio::test]
async fn api_model_lists_available_set_with_custom_provider_and_variants() {
    let root = PathBuf::from(format!("/tmp/opencode/oc-server-apimodel-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let home = root.join("home");
    let project = root.join("project");
    for dir in [&home, &project] {
        std::fs::create_dir_all(dir).expect("sandbox dir");
    }
    std::fs::write(project.join("opencode.json"), PARITY_CONFIG).expect("write config");
    let _sandbox = SandboxDrop(root.clone());

    let mut child = tokio::process::Command::new(env!("CARGO_BIN_EXE_oc-server"))
        .args(["serve", "--port", "0"])
        .env("HOME", &home)
        .env("XDG_CONFIG_HOME", home.join(".config"))
        .env("XDG_CACHE_HOME", home.join(".cache"))
        .env("XDG_DATA_HOME", home.join(".local/share"))
        .env("XDG_STATE_HOME", home.join(".local/state"))
        .env("OPENCODE_TEST_HOME", &home)
        .env_remove("OPENCODE_PASSWORD")
        .env_remove("OPENCODE_CONFIG")
        .env_remove("OPENCODE_DB")
        .env_remove("OPENCODE_DEBUG_ERRORS")
        .current_dir(&project)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .expect("spawn oc-server");
    let mut stdout = tokio::io::BufReader::new(child.stdout.take().expect("piped stdout"));
    let mut line = String::new();
    let url = tokio::time::timeout(Duration::from_secs(60), async {
        loop {
            line.clear();
            let read = stdout.read_line(&mut line).await.expect("stdout");
            assert!(read != 0, "server exited before printing its URL");
            if let Some(rest) = line.trim().strip_prefix("server listening on ") {
                return rest.to_owned();
            }
        }
    })
    .await
    .expect("server ready line");
    let base = url.trim().trim_end_matches('/').to_owned();
    let client = oc_client::Client::new(&format!("{base}/")).expect("client");

    let listed_response = tokio::time::timeout(
        Duration::from_secs(30),
        client.model().list(&oc_client::types::ModelListQuery { location: None }),
    )
    .await
    .expect("model.list timeout")
    .expect("model.list");
    let listed = listed_response.data;

    // (1) The custom provider's model is listed with its CONFIG NAME
    // (models.<id>.name — config/plugin/provider.ts:112) and the
    // openai-compatible effort variants (provider.ts:151-159).
    let fake = listed
        .iter()
        .find(|model| model.provider_id == "parityfake" && model.id == "fake-model")
        .expect("parityfake/fake-model listed");
    assert_eq!(fake.name, "Fake Model");
    let variants: Vec<&str> = fake.variants.iter().map(|variant| variant.id.as_str()).collect();
    assert_eq!(variants, vec!["low", "medium", "high"], "effort variants");

    // (2) Credential-less catalog providers are ABSENT (the bundled
    // catalog never leaks into the listing): every listed model is
    // either the custom provider's or the zen provider's zero-cost
    // tier.
    for model in &listed {
        assert!(
            model.provider_id == "parityfake" || model.provider_id == "opencode",
            "unexpected provider {} in the listing",
            model.provider_id
        );
    }
    let zen_paid = listed.iter().any(|model| {
        model.provider_id == "opencode" && model.cost.iter().any(|cost| cost.input > 0.0)
    });
    assert!(!zen_paid, "keyless zen serves zero-cost models only");

    // (3) The config default is visible over /api/config (the TUI's
    // configuredModel source — local.tsx:170-179).
    let config = tokio::time::timeout(
        Duration::from_secs(30),
        client.config().get(&oc_client::types::ConfigGetQuery { location: None }),
    )
    .await
    .expect("config timeout")
    .expect("config.get");
    let pinned = config.iter().rev().find_map(|entry| match entry {
        oc_client::types::ConfigEntry::DocumentEncoded(document) => document.info.model.clone(),
        oc_client::types::ConfigEntry::DirectoryEncoded(_) => None,
    });
    // The loader may normalize the string into the `{providerID,
    // model}` object — both shapes are `configuredModel` inputs
    // (local.tsx:176-178).
    match pinned {
        Some(oc_client::types::ConfigInfoEncodedModel::String(raw)) => {
            assert_eq!(raw, "parityfake/fake-model");
        }
        Some(oc_client::types::ConfigInfoEncodedModel::ConfigInfoEncodedModelVariant2(object)) => {
            assert_eq!(object.provider_id, "parityfake");
            assert_eq!(object.model, "fake-model");
        }
        None => panic!("the pinned default rides the LAST config document"),
    }
}

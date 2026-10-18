//! Gate 3 — tools differential (PARITY-LOOP.md §8.3 Gate 3, B0-2
//! skeleton).
//!
//! Dual-end (reference v2.0.24 + Rust port) compare of builtin-tool
//! executions through the real prompt pipeline: a scripted fake LLM emits
//! one tool call per session, the engine executes it against files seeded
//! in each side's lane project directory, and the resulting **tool part**
//! (input + content + metadata, normalized through the canonical
//! normalizer) plus the **file side effects** must match across sides.
//!
//! Live today (happy path): `read`, `edit` always; `glob`, `grep` when a
//! ripgrep binary is reachable for the port (`OPENCODE_RG_PATH` → PATH →
//! the lane-staged `/tmp/opencode/rg`; the port shells out to rg while v2
//! is pure JS) — content + metadata + side-effect compares. Structured
//! checkpoints (recorded, not yet executed — each is a ledger-linkable
//! gap): `shell` stdout/stderr/exit, shell `timeout`, `permission ask`,
//! `patch`/`write` tools, cross-side permission-request compares.
//!
//! Opt-in: `#[ignore]`d AND `OC_P10=1` at runtime (needs bun + reference):
//!
//! ```text
//! OC_P10=1 cargo nextest run --run-ignored only -E 'test(p10_tools)'
//! ```
//!
//! Artifact: `/tmp/opencode/p10-artifacts/gate3-tools.json`.

mod p10_common;

use anyhow::{ensure, Context, Result};
use serde_json::{json, Value};

use p10_common::{enabled, skip, ScriptedFakeLlm};
use parity::scenario::ScenarioHost;

/// A generic tool-call turn (the `p10_common::read_tool_script` shape,
/// name/arguments parameterized).
fn tool_script(call_id: &str, name: &str, arguments: &Value) -> String {
    let call = serde_json::json!({
        "choices":[{"delta":{"role":"assistant","tool_calls":[{
            "index":0,"id":call_id,"type":"function",
            "function":{"name":name,"arguments":arguments.to_string()}
        }]}}]
    })
    .to_string();
    let finish = serde_json::json!({
        "choices":[{"delta":{},"finish_reason":"tool_calls"}],
        "usage":{"prompt_tokens":9,"completion_tokens":3,"total_tokens":12}
    })
    .to_string();
    format!("data: {call}\n\ndata: {finish}\n\ndata: [DONE]\n\n")
}

/// The deterministic toolbox files, seeded identically into both sides'
/// lane project directories.
fn toolbox_files() -> Vec<(&'static str, &'static str)> {
    vec![
        ("toolbox/notes.txt", "gate3 line one\ngate3 line two\n"),
        ("toolbox/editme.txt", "alpha\nbeta\n"),
        ("toolbox/sub/data.txt", "needle in the sub\nplain line\n"),
    ]
}

/// One tool probe: script arguments per side + the expected file mutation.
struct ToolProbe {
    name: &'static str,
    call_id: &'static str,
    /// Build the tool-call arguments for one side's project directory.
    arguments: fn(&str) -> Value,
    /// The file (relative to the project dir) this tool mutates, and the
    /// exact bytes it must contain afterwards; `None` for read-only tools.
    mutates: Option<&'static str>,
    mutates_to: Option<&'static str>,
    /// The port shells these out to an external ripgrep binary
    /// (`oc_core::tool::ripgrep_runner`); the v2 side is pure JS.
    needs_rg: bool,
}

fn probes() -> Vec<ToolProbe> {
    vec![
        ToolProbe {
            name: "read",
            call_id: "call_gate3_read",
            arguments: |project| json!({ "path": format!("{project}/toolbox/notes.txt") }),
            mutates: None,
            mutates_to: None,
            needs_rg: false,
        },
        ToolProbe {
            name: "edit",
            call_id: "call_gate3_edit",
            arguments: |project| {
                json!({
                    "path": format!("{project}/toolbox/editme.txt"),
                    "oldString": "alpha",
                    "newString": "gamma",
                })
            },
            mutates: Some("toolbox/editme.txt"),
            mutates_to: Some("gamma\nbeta\n"),
            needs_rg: false,
        },
        ToolProbe {
            name: "glob",
            call_id: "call_gate3_glob",
            arguments: |project| json!({ "pattern": "toolbox/*.txt", "path": project }),
            mutates: None,
            mutates_to: None,
            needs_rg: true,
        },
        ToolProbe {
            name: "grep",
            call_id: "call_gate3_grep",
            arguments: |project| json!({ "pattern": "needle", "path": format!("{project}/toolbox") }),
            mutates: None,
            mutates_to: None,
            needs_rg: true,
        },
    ]
}

/// The fake-LLM scripts for one side: per probe, the tool call followed by
/// the closing text turn.
fn side_scripts(project_dir: &str, probes: &[ToolProbe]) -> Vec<String> {
    let mut scripts = Vec::new();
    for probe in probes {
        let arguments = (probe.arguments)(project_dir);
        scripts.push(tool_script(probe.call_id, probe.name, &arguments));
        scripts.push(p10_common::text_script("Parity fake says hello."));
    }
    scripts
}

/// Run every probe against one side; returns the normalized completed tool
/// parts in probe order.
async fn run_side(
    base: &str,
    password: &str,
    project_dir: &str,
    probes: &[ToolProbe],
) -> Result<Vec<Value>> {
    let mut parts = Vec::new();
    for probe in probes {
        let payload = json!({
            "title": format!("gate3-{}", probe.name),
            "location": {"directory": project_dir},
            "model": {"providerID": "parityfake", "id": "fake-model"}
        })
        .to_string();
        let (status, row) =
            p10_common::api(base, password, "POST", "/api/session", Some(&payload)).await?;
        ensure!(status == 200, "create gate3-{} session: {status} {row}", probe.name);
        let id = row["data"]["id"].as_str().context("session id")?.to_owned();

        let prompt = json!({ "text": format!("run {}", probe.name) }).to_string();
        let list = format!("/api/session/{id}/message");
        let (status, body) = p10_common::api(
            base,
            password,
            "POST",
            &format!("/api/session/{id}/prompt"),
            Some(&prompt),
        )
        .await?;
        ensure!(status == 200, "prompt gate3-{}: {status} {body}", probe.name);

        // Wait for the completed tool part AND the closing text reply.
        let messages = p10_common::wait_api(
            base,
            password,
            "GET",
            &list,
            None,
            |value| {
                value["data"].as_array().is_some_and(|rows| {
                    let tool_done = rows.iter().any(|message| {
                        message["content"].as_array().is_some_and(|parts| {
                            parts.iter().any(|part| {
                                part["type"] == json!("tool")
                                    && part["name"] == json!(probe.name)
                                    && part["state"]["status"] == json!("completed")
                            })
                        })
                    });
                    let text_done = rows.iter().any(|message| {
                        message["content"].as_array().is_some_and(|parts| {
                            parts.iter().any(|part| {
                                part["type"] == json!("text")
                                    && part["text"] == json!("Parity fake says hello.")
                            })
                        })
                    });
                    tool_done && text_done
                })
            },
            30,
            2,
        )
        .await
        .with_context(|| format!("gate3-{} tool part never completed", probe.name))?;
        let tool_part = messages["data"]
            .as_array()
            .and_then(|rows| {
                rows.iter()
                    .flat_map(|message| message["content"].as_array().cloned().unwrap_or_default())
                    .find(|part| part["type"] == json!("tool") && part["name"] == json!(probe.name))
            })
            .with_context(|| format!("gate3-{} tool part missing from messages", probe.name))?;
        parts.push(tool_part);
    }
    Ok(parts)
}

/// Resolve the ripgrep binary the RUST port's glob/grep shell out to
/// (mirrors `oc_core::tool::ripgrep_runner::binary`: `OPENCODE_RG_PATH` →
/// PATH → the lane-staged `/tmp/opencode/rg`). `None` = the probes are
/// environment-gated off (recorded, never silently dropped).
fn resolve_rg() -> Option<std::path::PathBuf> {
    if let Ok(path) = std::env::var("OPENCODE_RG_PATH") {
        let path = std::path::PathBuf::from(path);
        if path.is_file() {
            return Some(path);
        }
    }
    if let Ok(paths) = std::env::var("PATH") {
        for dir in paths.split(':') {
            let candidate = std::path::PathBuf::from(dir).join("rg");
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    let staged = std::path::PathBuf::from("/tmp/opencode/rg");
    staged.is_file().then_some(staged)
}

#[tokio::test]
#[ignore = "Phase-10 gate: opt in with OC_P10=1 (needs bun + reference)"]
async fn p10_tools_gate3() -> Result<()> {
    if !enabled() {
        skip("OC_P10 != 1");
        return Ok(());
    }
    if !p10_common::reference_runnable() {
        return Ok(());
    }
    if parity::rust_server::skip_if_no_cargo() {
        return Ok(());
    }
    // Deterministic lane sandbox root (B0-2 acceptance: pid-scoped).
    std::env::set_var(
        "PARITY_SANDBOX_ROOT",
        format!("/tmp/opencode/lane-b0-gates-{}-tools", std::process::id()),
    );
    run().await
}

#[allow(clippy::too_many_lines)]
async fn run() -> Result<()> {
    // Two lane project dirs (the session location for each side; the
    // toolbox files live here so tool arguments are absolute and stable).
    let root = format!("/tmp/opencode/lane-b0-gates-{}-tools-projects", std::process::id());
    let ref_project = format!("{root}/ref-project");
    let rust_project = format!("{root}/rust-project");
    for project in [&ref_project, &rust_project] {
        for (relative, contents) in toolbox_files() {
            let path = std::path::Path::new(project).join(relative);
            std::fs::create_dir_all(path.parent().context("toolbox parent")?)?;
            std::fs::write(&path, contents)?;
        }
    }

    // glob/grep need an rg binary for the port; without one they are
    // environment-gated OFF (recorded below, never silently dropped).
    let rg = resolve_rg();
    let live_probes: Vec<ToolProbe> =
        probes().into_iter().filter(|probe| !probe.needs_rg || rg.is_some()).collect();
    let gated: Vec<&str> = probes()
        .iter()
        .filter(|probe| probe.needs_rg && rg.is_none())
        .map(|probe| probe.name)
        .collect();
    if !gated.is_empty() {
        eprintln!(
            "[gate3] env-gated OFF (no ripgrep reachable for the port side; set OPENCODE_RG_PATH \
             or stage /tmp/opencode/rg): {gated:?}"
        );
    }

    // One scripted fake per side (arguments embed the side's own paths).
    let ref_fake = ScriptedFakeLlm::start(side_scripts(&ref_project, &live_probes)).await?;
    let rust_fake = ScriptedFakeLlm::start(side_scripts(&rust_project, &live_probes)).await?;
    std::fs::write(
        std::path::Path::new(&ref_project).join("opencode.json"),
        p10_common::provider_config(&ref_fake.base(), false),
    )?;
    std::fs::write(
        std::path::Path::new(&rust_project).join("opencode.json"),
        p10_common::provider_config(&rust_fake.base(), false),
    )?;

    let reference = parity::reference::spawn().await?;
    let mut rust_spawn = parity::rust_server::RustSpawn::default();
    if let Some(rg) = &rg {
        rust_spawn.extra_env.push((
            "OPENCODE_RG_PATH",
            Box::leak(rg.display().to_string().into_boxed_str()) as &'static str,
        ));
    }
    let port = parity::rust_server::spawn_rust(rust_spawn).await?;
    let result = wait_states(&reference, &port, &ref_project, &rust_project, &live_probes).await;
    let stop_ref = reference.stop();
    let stop_port = port.stop();
    let (ref_parts, rust_parts) = result?;
    stop_ref.await?;
    stop_port.await?;

    // Normalize each side's tool parts with its OWN project root, then
    // deep-compare (ids/timestamps/paths all canonicalized).
    let ref_env = parity::normalize::NormEnv::new().with_root(&ref_project);
    let rust_env = parity::normalize::NormEnv::new().with_root(&rust_project);
    let ref_normalized: Vec<Value> =
        ref_parts.iter().map(|part| parity::normalize::normalize_value(part, &ref_env)).collect();
    let rust_normalized: Vec<Value> =
        rust_parts.iter().map(|part| parity::normalize::normalize_value(part, &rust_env)).collect();
    let mut report_rows = Vec::new();
    for (probe, (reference_part, port_part)) in
        live_probes.iter().zip(ref_normalized.iter().zip(rust_normalized.iter()))
    {
        let divergence = parity::httpdiff::deep_diff(reference_part, port_part);
        ensure!(
            divergence.is_none(),
            "gate3-{} tool part diverged: {:?}\nreference={reference_part}\nport={port_part}",
            probe.name,
            divergence.map(|d| d.to_string())
        );
        report_rows.push(json!({
            "tool": probe.name,
            "state": "checked",
            "normalized_equal": true,
            "reference": reference_part,
            "port": port_part,
        }));
        eprintln!("[gate3] {} — normalized tool part equal", probe.name);
    }

    // File side effects: the edit mutation must be byte-exact on both sides.
    let edit_probe = live_probes.iter().find(|probe| probe.mutates.is_some()).expect("edit probe");
    let expected = edit_probe.mutates_to.expect("edit bytes");
    for project in [&ref_project, &rust_project] {
        let path = std::path::Path::new(project).join(edit_probe.mutates.expect("edit target"));
        let actual = std::fs::read_to_string(&path)
            .with_context(|| format!("side effect read {}", path.display()))?;
        ensure!(
            actual == expected,
            "gate3 edit side effect on {}: {actual:?} (want {expected:?})",
            path.display()
        );
    }
    eprintln!("[gate3] edit — file side effects byte-equal on both sides");

    let report = json!({
        "gate": "gate3-tools",
        "rg_for_port": rg.as_ref().map(|path| path.display().to_string()),
        "env_gated_off": gated,
        "checked": report_rows,
        "checkpoints": [
            {"name": "shell stdout/stderr/exit", "state": "not-wired",
             "note": "dual-end shell tool compare (scripted command, stream + exit code)"},
            {"name": "shell timeout", "state": "not-wired",
             "note": "dual-end timeout envelope (same command, sub-timeout)"},
            {"name": "permission ask", "state": "not-wired",
             "note": "dual-end parked-permission payload compare (outside-project tool ask)"},
            {"name": "patch/write tools", "state": "not-wired",
             "note": "same harness as read/edit once those tools are triaged"},
        ],
        "structured_gaps": "shell/timeout/permission/patch sections recorded as ledger-linkable \
         checkpoints (B0-2 skeleton); wiring them needs per-section payload triage",
    });
    std::fs::create_dir_all(p10_common::artifacts())?;
    std::fs::write(
        p10_common::artifacts().join("gate3-tools.json"),
        format!("{}\n", serde_json::to_string_pretty(&report)?),
    )?;
    eprintln!("[gate3] PASS — {} tool compares equal; 4 checkpoints recorded", report_rows.len());
    Ok(())
}

/// Drive both sides' probe sets; separated so both servers always stop.
async fn wait_states(
    reference: &parity::reference::ReferenceServer,
    port: &parity::rust_server::RustServer,
    ref_project: &str,
    rust_project: &str,
    live_probes: &[ToolProbe],
) -> Result<(Vec<Value>, Vec<Value>)> {
    let ref_parts = run_side(reference.url(), reference.password(), ref_project, live_probes)
        .await
        .context("reference side tool probes")?;
    let rust_parts = run_side(port.url(), port.password(), rust_project, live_probes)
        .await
        .context("port side tool probes")?;
    Ok((ref_parts, rust_parts))
}

//! RESCAN lane — live plugin-armed TUI capture driver (report-only
//! scaffolding; never part of product code).
//!
//! Boots BOTH stacks (ref v2.0.24 via bun + our opencode-rust CLI)
//! against their own servers in ONE shared project directory, with
//! oh-my-opencode-slim ACTUALLY ARMED on both sides (package copied
//! into each side's `~/.config/opencode/plugins/` dir + the npm
//! cache layout + the config `plugins[]` entry). Drives identical
//! key scripts on both PTYs and dumps per-capture grids (text rows,
//! per-cell style digests, raw byte mirrors) for offline diffing,
//! plus a temporal series (fixed screen sampled over time) for the
//! animation/indicator axis.
//!
//! ```text
//! cargo run -p parity --bin rescan-tui -- --scenario boot --scenario session
//! ```

use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::time::Duration;

use parity::scenario::ScenarioHost;
use parity::tui_diff::{Grid, PtyChild, SharedStub};

// ---------------------------------------------------------------------------
// Sandbox
// ---------------------------------------------------------------------------

fn copy_dir_all(source: &Path, destination: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(destination)?;
    for entry in std::fs::read_dir(source)? {
        let entry = entry?;
        let target = destination.join(entry.file_name());
        if entry.path().is_dir() {
            // Skip caches/deps that bloat the copy but never matter to
            // the host loaders (keep node_modules: the plugin resolves
            // @opencode-ai/plugin + solid-js from its hoisted tree).
            copy_dir_all(&entry.path(), &target)?;
        } else {
            std::fs::copy(entry.path(), target)?;
        }
    }
    Ok(())
}

fn omos_source() -> Option<PathBuf> {
    if let Ok(root) = std::env::var("OC_OMOS_ROOT") {
        let root = PathBuf::from(root);
        return root.join("dist/server/index.js").is_file().then_some(root);
    }
    let base = Path::new("/root/.cache/opencode/npm/oh-my-opencode-slim@latest");
    let mut stamps: Vec<PathBuf> = std::fs::read_dir(base)
        .ok()?
        .filter_map(Result::ok)
        .filter(|e| e.path().is_dir())
        .map(|e| e.path())
        .collect();
    stamps.sort();
    stamps
        .into_iter()
        .rev()
        .map(|stamp| stamp.join("node_modules").join("oh-my-opencode-slim"))
        .find(|root| root.join("dist/server/index.js").is_file())
}

/// Seed one side's home with the plugin through the npm-install cache
/// layout. OUR stack reads `<cache>/opencode-rust/npm/...` (`APP_NAME`
/// suffix; the live machine's migrated copy), the `REF` reads
/// `<cache>/opencode/npm/...` (v2 `global.cache`).
fn seed_home_plugin(home: &Path, omos: &Path, suffix: &str) -> std::io::Result<()> {
    for dir in [".config", ".config/opencode", ".local/share", ".local/state", ".cache", "tmp"] {
        std::fs::create_dir_all(home.join(dir))?;
    }
    // omos = <base>/<pkg>@<spec>/<stamp>/node_modules/<pkg>
    let stamp = omos.parent().and_then(Path::parent).map(Path::to_path_buf).expect("stamp dir");
    let name = stamp.file_name().and_then(|n| n.to_str()).unwrap_or("stamp").to_owned();
    copy_dir_all(
        &stamp,
        &home.join(format!(".cache/{suffix}/npm/oh-my-opencode-slim@latest")).join(&name),
    )?;
    Ok(())
}

fn seed_project(project: &Path, stub_url: &str, plugin_spec: &str) -> anyhow::Result<()> {
    std::fs::create_dir_all(project.join("src"))?;
    std::fs::write(project.join("README.md"), "rescan parity project\n\nSeeded fixture.\n")?;
    std::fs::write(
        project.join("src/main.rs"),
        "fn main() {\n    println!(\"rescan fixture\");\n}\n",
    )?;
    let config = format!(
        r#"{{
  "$schema": "https://opencode.ai/config.json",
  "model": "parityfake/fake-model",
  "plugin": ["{plugin_spec}"],
  "plugins": ["{plugin_spec}"],
  "providers": {{
    "parityfake": {{
      "package": "@opencode/ai/providers/openai-compatible",
      "settings": {{ "baseURL": "{stub_url}/v1" }},
      "models": {{ "fake-model": {{ "name": "Fake Model", "body": {{ "temperature": 0 }} }} }}
    }}
  }}
}}
"#
    );
    std::fs::write(project.join("opencode.json"), config)?;
    std::fs::write(
        project.join("tsconfig.json"),
        r#"{
  "compilerOptions": {
    "jsx": "preserve",
    "jsxImportSource": "@opentui/solid",
    "lib": ["ESNext", "DOM", "DOM.Iterable", "DOM.AsyncIterable"],
    "module": "esnext",
    "moduleResolution": "bundler",
    "target": "esnext",
    "types": ["bun"]
  }
}
"#,
    )?;
    let _ =
        std::process::Command::new("git").args(["init", "--quiet"]).current_dir(project).output();
    let _ = std::process::Command::new("git")
        .args(["-c", "user.email=r@opencode.ai", "-c", "user.name=R", "add", "-A"])
        .current_dir(project)
        .output();
    let _ = std::process::Command::new("git")
        .args([
            "-c",
            "user.email=r@opencode.ai",
            "-c",
            "user.name=R",
            "commit",
            "--quiet",
            "-m",
            "seed",
        ])
        .current_dir(project)
        .output();
    Ok(())
}

fn home_env(home: &Path, password: &str, config: &Path) -> Vec<(String, String)> {
    vec![
        ("TERM".to_owned(), "xterm-256color".to_owned()),
        ("HOME".to_owned(), home.display().to_string()),
        ("OPENCODE_TEST_HOME".to_owned(), home.display().to_string()),
        ("XDG_DATA_HOME".to_owned(), home.join(".local/share").display().to_string()),
        ("XDG_STATE_HOME".to_owned(), home.join(".local/state").display().to_string()),
        ("XDG_CONFIG_HOME".to_owned(), home.join(".config").display().to_string()),
        ("XDG_CACHE_HOME".to_owned(), home.join(".cache").display().to_string()),
        ("TMPDIR".to_owned(), home.join("tmp").display().to_string()),
        ("OPENCODE_PASSWORD".to_owned(), password.to_owned()),
        ("OPENCODE_CONFIG".to_owned(), config.display().to_string()),
        ("PATH".to_owned(), "/root/.bun/bin:/usr/local/bin:/usr/bin:/bin".to_owned()),
        ("LANG".to_owned(), "C.UTF-8".to_owned()),
        ("LC_ALL".to_owned(), "C.UTF-8".to_owned()),
        // Plugin logs visible in captured mirrors.
        ("DEBUG".to_owned(), "opencode*".to_owned()),
    ]
}

// ---------------------------------------------------------------------------
// Grid dump
// ---------------------------------------------------------------------------

fn dump_grid(grid: &Grid, path: &Path) -> anyhow::Result<()> {
    let mut out = String::new();
    for y in 0..grid.rows {
        out.push_str(&grid.row_text(y));
        out.push('\n');
    }
    std::fs::write(path.with_extension("txt"), &out)?;
    // style map: "x,y:digest" for every non-default cell
    let mut styles = String::new();
    for y in 0..grid.rows {
        for x in 0..grid.cols {
            let cell = grid.cell(x, y);
            if cell.style != 0 {
                writeln!(styles, "{x},{y}:{:016x} {}", cell.style, cell.ch).ok();
            }
        }
    }
    std::fs::write(path.with_extension("styles.txt"), &styles)?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Driver
// ---------------------------------------------------------------------------

fn ref_checkout_root() -> PathBuf {
    std::env::var("PARITY_REF_ROOT")
        .map_or_else(|_| PathBuf::from("/tmp/opencode/ref"), PathBuf::from)
}

fn resolve_bun() -> PathBuf {
    std::env::var("PARITY_BUN").map_or_else(|_| PathBuf::from("/root/.bun/bin/bun"), PathBuf::from)
}

struct Args {
    scenarios: Vec<String>,
    out: PathBuf,
    cols: u16,
    rows: u16,
    ref_bin: Option<PathBuf>,
}

fn parse_args() -> Args {
    let mut scenarios = Vec::new();
    let mut out = PathBuf::from("/tmp/opencode/rescan-live");
    let mut cols = 160u16;
    let mut rows = 36u16;
    let mut ref_bin: Option<PathBuf> = None;
    let mut argv = std::env::args().skip(1);
    while let Some(arg) = argv.next() {
        match arg.as_str() {
            "--scenario" => scenarios.push(argv.next().expect("scenario name")),
            "--out" => out = PathBuf::from(argv.next().expect("out dir")),
            "--size" => {
                let spec = argv.next().expect("WxH");
                let (c, r) = spec.split_once('x').expect("WxH");
                cols = c.parse().unwrap();
                rows = r.parse().unwrap();
            }
            "--ref-bin" => ref_bin = Some(PathBuf::from(argv.next().expect("bin path"))),
            other => panic!("unknown arg {other}"),
        }
    }
    if scenarios.is_empty() {
        scenarios.push("boot".into());
    }
    Args { scenarios, out, cols, rows, ref_bin }
}

fn wait_stable(child: &mut PtyChild, timeout_ms: u64) {
    let _ = child.wait_grid_stable(4, 120, Duration::from_millis(timeout_ms));
}

fn capture_series(
    side: &mut PtyChild,
    dir: &Path,
    label: &str,
    samples: usize,
    gap_ms: u64,
) -> anyhow::Result<()> {
    std::fs::create_dir_all(dir)?;
    for i in 0..samples {
        std::thread::sleep(Duration::from_millis(gap_ms));
        let grid = side.grid_now();
        dump_grid(&grid, &dir.join(format!("{label}-{i:02}")))?;
        // raw mirror too (escape-level evidence)
        std::fs::write(dir.join(format!("{label}-{i:02}.raw")), side.raw_mirror())?;
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Ref-side server abstraction: the 2.0.24 checkout server OR the user's
// real binary (e.g. /root/.opencode/bin/opencode2 = v2.0.25 — the only
// ref that actually loads the current plugin generation).
// ---------------------------------------------------------------------------

const REF_BIN_PASSWORD: &str = "rescan-pw";

struct RefBinServer {
    child: tokio::process::Child,
    url: String,
}

enum RefSide {
    Checkout(parity::reference::ReferenceServer),
    Bin(RefBinServer),
}

impl RefSide {
    fn url(&self) -> String {
        match self {
            Self::Checkout(server) => server.url().to_owned(),
            Self::Bin(server) => server.url.clone(),
        }
    }
    fn password(&self) -> String {
        match self {
            Self::Checkout(server) => server.password().to_owned(),
            Self::Bin(_) => REF_BIN_PASSWORD.to_owned(),
        }
    }
    async fn stop(self) {
        match self {
            Self::Checkout(server) => {
                let _ = server.stop().await;
            }
            Self::Bin(mut server) => {
                let _ = server.child.start_kill();
                let _ = tokio::time::timeout(Duration::from_secs(5), server.child.wait()).await;
            }
        }
    }
}

fn free_port() -> anyhow::Result<u16> {
    let listener = std::net::TcpListener::bind("127.0.0.1:0")?;
    Ok(listener.local_addr()?.port())
}

async fn spawn_ref_bin(
    bin: &Path,
    home: &Path,
    project: &Path,
    config_env: &str,
) -> anyhow::Result<RefSide> {
    let port = free_port()?;
    let url = format!("http://127.0.0.1:{port}");
    let env = home_env(home, REF_BIN_PASSWORD, Path::new(config_env));
    let mut command = tokio::process::Command::new(bin);
    command
        .args(["serve", "--port", &port.to_string(), "--hostname", "127.0.0.1"])
        .current_dir(project)
        .env_clear()
        .envs(env.iter().map(|(k, v)| (k, v)))
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    let child = command.spawn()?;
    // Readiness: /api/info 200 (≤90 s), then give plugins time to load.
    let auth = parity::minih::basic_auth("opencode", REF_BIN_PASSWORD);
    let mut ready = false;
    for _ in 0..180 {
        if let Ok(response) = parity::minih::request(
            &url,
            Some(&auth),
            "GET",
            "/api/info",
            None,
            "application/json",
            Duration::from_secs(2),
        )
        .await
        {
            if response.status == 200 {
                ready = true;
                break;
            }
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
    anyhow::ensure!(ready, "ref bin server did not become ready on {url}");
    tokio::time::sleep(Duration::from_secs(8)).await;
    Ok(RefSide::Bin(RefBinServer { child, url }))
}

// Retired rescan probe: monolithic scenario driver kept intentionally
// (HYGIENE-CLIPPY-RESCAN-TUI debt item in the parity ledger).
#[tokio::main]
#[allow(clippy::too_many_lines)]
async fn main() -> anyhow::Result<()> {
    let args = parse_args();
    let root = args.out.clone();
    std::fs::create_dir_all(&root)?;

    let omos = omos_source().expect("oh-my-opencode-slim install (OC_OMOS_ROOT or the npm cache)");
    eprintln!("[rescan] plugin source: {}", omos.display());
    let cli = parity::rust_server::cli_binary()?;
    eprintln!("[rescan] our cli: {}", cli.display());

    for scenario in &args.scenarios {
        let sroot = root.join(scenario);
        let _ = std::fs::remove_dir_all(&sroot);
        let project = sroot.join("project");
        let ref_home = sroot.join("ref-home");
        let our_home = sroot.join("our-home");
        std::fs::create_dir_all(&project)?;
        seed_home_plugin(&ref_home, &omos, "opencode")?;
        seed_home_plugin(&our_home, &omos, "opencode-rust")?;

        // Stub first (config embeds its URL).
        let stub = SharedStub::start()?;
        seed_project(&project, &stub.url, "oh-my-opencode-slim@latest")?;
        if scenario == "subagent" || scenario == "clicksub" {
            stub.arm_flow(parity::tui_diff::subagent_flow_scripts());
        } else {
            stub.arm(parity::tui_diff::StubScript::Text);
        }

        let config_path = project.join("opencode.json");
        let config_env = config_path.display().to_string();

        // --- servers (HOME overridden into the seeded homes) ---
        let ref_server = if let Some(bin) = args.ref_bin.clone() {
            spawn_ref_bin(&bin, &ref_home, &project, &config_env).await?
        } else {
            RefSide::Checkout(
                parity::reference::spawn_reference(parity::reference::ReferenceSpawn {
                    extra_env: vec![
                        ("OPENCODE_CONFIG", config_env.as_str()),
                        ("HOME", ref_home.display().to_string().as_str()),
                        (
                            "XDG_CONFIG_HOME",
                            ref_home.join(".config").display().to_string().as_str(),
                        ),
                        ("XDG_CACHE_HOME", ref_home.join(".cache").display().to_string().as_str()),
                        (
                            "XDG_DATA_HOME",
                            ref_home.join(".local/share").display().to_string().as_str(),
                        ),
                        (
                            "XDG_STATE_HOME",
                            ref_home.join(".local/state").display().to_string().as_str(),
                        ),
                        ("OPENCODE_DISABLE_MODELS_FETCH", "1"),
                    ],
                    seed_files: Vec::new(),
                })
                .await?,
            )
        };
        let our_server = parity::rust_server::spawn_rust(parity::rust_server::RustSpawn {
            extra_env: vec![
                ("OPENCODE_CONFIG", config_env.as_str()),
                ("OPENCODE_CHANNEL", "stable"),
                ("HOME", our_home.display().to_string().as_str()),
                ("XDG_CONFIG_HOME", our_home.join(".config").display().to_string().as_str()),
                ("XDG_CACHE_HOME", our_home.join(".cache").display().to_string().as_str()),
                ("XDG_DATA_HOME", our_home.join(".local/share").display().to_string().as_str()),
                ("XDG_STATE_HOME", our_home.join(".local/state").display().to_string().as_str()),
            ],
            seed_files: Vec::new(),
        })
        .await?;

        // --- TUIs on PTYs ---
        let ref_argv = if args.ref_bin.is_some() {
            vec!["--server".to_owned(), ref_server.url()]
        } else {
            let preload = ref_checkout_root()
                .join("packages/cli/node_modules/@opentui/solid/scripts/preload.js");
            let entry = ref_checkout_root().join("packages/cli/src/index.ts");
            vec![
                "--preload".to_owned(),
                preload.display().to_string(),
                "--define".to_owned(),
                "OPENCODE_VERSION=\"2.0.24\"".to_owned(),
                "--define".to_owned(),
                "OPENCODE_CHANNEL=\"stable\"".to_owned(),
                "--define".to_owned(),
                "OPENCODE_ARTIFACT=\"cli\"".to_owned(),
                entry.display().to_string(),
                "--server".to_owned(),
                ref_server.url(),
            ]
        };
        let our_argv = vec!["--server".to_owned(), our_server.url().to_owned()];
        let ref_env = home_env(&ref_home, &ref_server.password(), &config_path);
        let our_env = home_env(&our_home, our_server.password(), &config_path);

        let ref_program: PathBuf = args.ref_bin.clone().unwrap_or_else(resolve_bun);
        let mut ref_tui =
            PtyChild::spawn(&ref_program, &ref_argv, &project, &ref_env, args.cols, args.rows)?;
        let mut our_tui =
            PtyChild::spawn(&cli, &our_argv, &project, &our_env, args.cols, args.rows)?;

        // boot settle (plugins load on both)
        std::thread::sleep(Duration::from_secs(if scenario == "boot" { 12 } else { 10 }));
        wait_stable(&mut ref_tui, 6000);
        wait_stable(&mut our_tui, 6000);

        let dir = sroot.join("captures");
        capture_series(&mut ref_tui, &dir, "ref-boot", 6, 500)?;
        capture_series(&mut our_tui, &dir, "our-boot", 6, 500)?;

        if scenario != "boot" {
            if scenario == "clicksub" {
                // subagent flow first (same dance as the session branch)
                for side in [&mut ref_tui, &mut our_tui] {
                    for ch in "hello world".chars() {
                        side.write(ch.to_string().as_bytes());
                        std::thread::sleep(Duration::from_millis(15));
                    }
                    side.write(b"\r");
                }
                std::thread::sleep(Duration::from_secs(10));
                wait_stable(&mut ref_tui, 6000);
                wait_stable(&mut our_tui, 6000);
                capture_series(&mut ref_tui, &dir, "ref-preclick", 2, 300)?;
                capture_series(&mut our_tui, &dir, "our-preclick", 2, 300)?;
                // then CLICK the transcript subagent card (row 5, col 6)
                let press = b"\x1b[<0;7;6M".to_vec();
                let release = b"\x1b[<0;7;6m".to_vec();
                ref_tui.write(&press);
                std::thread::sleep(Duration::from_millis(60));
                ref_tui.write(&release);
                our_tui.write(&press);
                std::thread::sleep(Duration::from_millis(60));
                our_tui.write(&release);
                std::thread::sleep(Duration::from_secs(4));
                wait_stable(&mut ref_tui, 5000);
                wait_stable(&mut our_tui, 5000);
                capture_series(&mut ref_tui, &dir, "ref-clicked", 3, 300)?;
                capture_series(&mut our_tui, &dir, "our-clicked", 3, 300)?;
                let _ = std::fs::write(sroot.join("stub-requests.json"), String::new());
                stub.stop();
                ref_tui.kill();
                our_tui.kill();
                ref_server.stop().await;
                let _ = our_server.stop().await;
                eprintln!("[rescan] scenario {scenario} captured into {}", dir.display());
                continue;
            }
            if scenario == "temporal" {
                // busy-window rapid sampling: spinner + pulse cadence DURING
                // the reply (StubScript::Reasoning gives a longer busy
                // window), then the rename-shimmer window.
                stub.arm(parity::tui_diff::StubScript::ReasoningLong);
                for side in [&mut ref_tui, &mut our_tui] {
                    for ch in "spin me".chars() {
                        side.write(ch.to_string().as_bytes());
                        std::thread::sleep(Duration::from_millis(10));
                    }
                    side.write(b"\r");
                }
                for i in 0..40 {
                    std::thread::sleep(Duration::from_millis(120));
                    let g1 = ref_tui.grid_now();
                    dump_grid(&g1, &dir.join(format!("ref-busy-{i:02}")))?;
                    let g2 = our_tui.grid_now();
                    dump_grid(&g2, &dir.join(format!("our-busy-{i:02}")))?;
                }
                // settle, then rename dialog + shimmer sampling (the
                // pending title renders with title_shimmer on the ref)
                wait_stable(&mut ref_tui, 5000);
                wait_stable(&mut our_tui, 5000);
                for side in [&mut ref_tui, &mut our_tui] {
                    side.write(b"\x12"); // ctrl+r rename
                }
                std::thread::sleep(Duration::from_millis(800));
                for ch in "renamed title".chars() {
                    ref_tui.write(ch.to_string().as_bytes());
                    our_tui.write(ch.to_string().as_bytes());
                    std::thread::sleep(Duration::from_millis(40));
                }
                for i in 0..30 {
                    std::thread::sleep(Duration::from_millis(120));
                    let g1 = ref_tui.grid_now();
                    dump_grid(&g1, &dir.join(format!("ref-shimmer-{i:02}")))?;
                    let g2 = our_tui.grid_now();
                    dump_grid(&g2, &dir.join(format!("our-shimmer-{i:02}")))?;
                }
                for side in [&mut ref_tui, &mut our_tui] {
                    side.write(b"\x1b"); // esc out of the dialog
                }
                let _ = std::fs::write(sroot.join("stub-requests.json"), String::new());
                stub.stop();
                ref_tui.kill();
                our_tui.kill();
                ref_server.stop().await;
                let _ = our_server.stop().await;
                eprintln!("[rescan] scenario {scenario} captured into {}", dir.display());
                continue;
            }
            if scenario == "plugindiag" {
                for side in [&mut ref_tui, &mut our_tui] {
                    for ch in "/plugins".chars() {
                        side.write(ch.to_string().as_bytes());
                        std::thread::sleep(Duration::from_millis(20));
                    }
                    side.write(b"\r");
                }
                std::thread::sleep(Duration::from_secs(3));
                // select the failed row + view error (ref dialog affordance)
                ref_tui.write(b"\r");
                std::thread::sleep(Duration::from_secs(3));
                wait_stable(&mut ref_tui, 5000);
                wait_stable(&mut our_tui, 5000);
                capture_series(&mut ref_tui, &dir, "ref-plugindiag", 2, 300)?;
                capture_series(&mut our_tui, &dir, "our-plugindiag", 2, 300)?;
                let _ = std::fs::write(sroot.join("stub-requests.json"), String::new());
                stub.stop();
                ref_tui.kill();
                our_tui.kill();
                ref_server.stop().await;
                let _ = our_server.stop().await;
                eprintln!("[rescan] scenario {scenario} captured into {}", dir.display());
                continue;
            }
            // start a session and send a prompt (identical bytes both sides)
            for side in [&mut ref_tui, &mut our_tui] {
                for ch in "hello world".chars() {
                    side.write(ch.to_string().as_bytes());
                    std::thread::sleep(Duration::from_millis(15));
                }
                side.write(b"\r");
            }
            // reply + plugin polls settle
            std::thread::sleep(Duration::from_secs(10));
            wait_stable(&mut ref_tui, 6000);
            wait_stable(&mut our_tui, 6000);
            capture_series(&mut ref_tui, &dir, "ref-session", 8, 400)?;
            capture_series(&mut our_tui, &dir, "our-session", 8, 400)?;
        }

        let recorded = stub.take_recorded();
        let shapes: Vec<String> = recorded.iter().map(|r| format!("{r:?}")).collect();
        let _ = std::fs::write(sroot.join("stub-requests.json"), shapes.join("\n"));
        stub.stop();
        ref_tui.kill();
        our_tui.kill();
        ref_server.stop().await;
        let _ = our_server.stop().await;
        eprintln!("[rescan] scenario {scenario} captured into {}", dir.display());
    }
    Ok(())
}

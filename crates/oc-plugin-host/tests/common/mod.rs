//! Shared helpers for the bun-spawning integration tests.
#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use oc_plugin_host::PluginHost;

/// A PID-unique sandbox under `/tmp/opencode` (gate constraint).
pub fn sandbox(name: &str) -> PathBuf {
    let dir =
        PathBuf::from(format!("/tmp/opencode/oc-plugin-host-test-{}-{name}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("sandbox dir");
    dir
}

/// Copy one committed fixture into the sandbox (tests mutate their
/// copies — hot reload edits must never touch the repo).
pub fn copy_fixture(sandbox: &Path, name: &str) -> PathBuf {
    let source = oc_plugin_host::fixture_dir().join(name);
    let destination = sandbox.join(name);
    std::fs::copy(&source, &destination).unwrap_or_else(|error| panic!("copy {name}: {error}"));
    destination
}

/// Start a host over `entries` with the sandbox env injected.
pub fn host_with(entries: &[PathBuf], sandbox: &Path) -> PluginHost {
    let config = oc_plugin_host::PluginHostConfig {
        env: vec![("OC_PLUGIN_TEST_SANDBOX".to_owned(), sandbox.display().to_string())],
        ..oc_plugin_host::PluginHostConfig::default()
    };
    PluginHost::detect().start_detached_with(config, entries)
}

/// Poll `probe` every 25 ms until it holds or `timeout` elapses.
pub fn wait_for(timeout: Duration, mut probe: impl FnMut() -> bool) -> bool {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if probe() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    probe()
}

/// Read a small text file (markers).
pub fn read(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap_or_default()
}

//! Port of `cli/src/services/service-config.ts` — the CLI-owned
//! `~/.config/opencode/service[.<channel>].json` config plus the
//! `oc_server::lifecycle::EnsureOptions` binding (registration file by
//! channel, version check, spawn command).

use std::collections::BTreeMap;
use std::path::PathBuf;

use oc_server::lifecycle::{self, EnsureOptions, StopOptions};
use serde::{Deserialize, Serialize};
use sha1::Digest as _;

use crate::util;

/// `ServiceConfig.Info`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Info {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub disabled: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hostname: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub port: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub password: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cors: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub env: Option<BTreeMap<String, String>>,
}

/// `ServiceConfig.filename(channel)`.
#[must_use]
pub fn filename(channel: &str) -> String {
    match channel {
        "latest" | "dev" | "beta" | "next" => "service.json".to_owned(),
        other => format!(
            "service-{}.json",
            other
                .chars()
                .map(|c| if c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-') {
                    c
                } else {
                    '-'
                })
                .collect::<String>()
        ),
    }
}

/// Lowercase hex of a digest.
fn hex_lower(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0xf) as usize] as char);
    }
    out
}

/// `ServiceConfig.defaultPort(channel)` (`Hash.fast` = sha1 hex).
#[must_use]
pub fn default_port(channel: &str) -> u16 {
    match channel {
        "latest" | "dev" | "beta" | "next" => 0xc0de,
        "local" => 0xc0df,
        other => {
            let digest = hex_lower(&sha1::Sha1::digest(other.as_bytes()));
            let prefix = u64::from_str_radix(&digest[..8], 16).unwrap_or(0);
            u16::try_from(10_000 + (prefix % 50_000)).unwrap_or(0xc0de)
        }
    }
}

/// The rust port's OWN default service port. Registered divergence
/// (`PORT_REPORT` §3): v2's channel scheme pins `latest` to 0xc0de=49374;
/// a faithful port would collide with any concurrently running v2 on the
/// same machine — the coexistence scenario this port exists for. We
/// therefore derive our default from `"opencode-rust:{channel}"` through
/// the same sha1 scheme (deterministic, disjoint from v2's fixed ports).
/// Explicit configuration (`service set port`, config, `--port`) still
/// wins over this default everywhere.
#[must_use]
pub fn rust_default_port(channel: &str) -> u16 {
    default_port(&format!("opencode-rust:{channel}"))
}

/// `ServiceConfig.legacyFilename(channel)`.
#[must_use]
pub fn legacy_filename(channel: &str) -> Option<String> {
    if channel == "latest" || channel == "local" {
        return None;
    }
    Some(format!("service-{}.json", hex_lower(&sha1::Sha1::digest(channel.as_bytes()))))
}

/// `ServiceConfig.versionBelongsToChannel`.
#[must_use]
pub fn version_belongs_to_channel(version: Option<&str>, channel: &str) -> bool {
    let Some(version) = version else { return false };
    if version == util::VERSION {
        return true;
    }
    let prefix = format!("0.0.0-{channel}-");
    let Some(rest) = version.strip_prefix(&prefix) else { return false };
    !rest.is_empty()
        && rest.bytes().all(|b| b.is_ascii_digit() || b == b'.')
        && rest.split('.').all(|part| !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit()))
}

fn roots() -> oc_util::global_roots::Roots {
    oc_util::global_roots::resolve()
}

/// The registration file path for the current channel.
#[must_use]
pub fn registration_file() -> PathBuf {
    roots().state.join(filename(&util::channel()))
}

/// The config file path for the current channel.
#[must_use]
#[allow(dead_code)]
pub fn config_file() -> PathBuf {
    roots().config.join(filename(&util::channel()))
}

fn migrate_registration(legacy: &std::path::Path, file: &std::path::Path) {
    let Ok(text) = std::fs::read_to_string(legacy) else { return };
    let Ok(value) = serde_json::from_str::<serde_json::Value>(&text) else { return };
    let Some(version) = value.get("version").and_then(|v| v.as_str()) else { return };
    if !version_belongs_to_channel(Some(version), &util::channel()) {
        return;
    }
    if let Some(parent) = file.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    write_private_new(file, &text);
}

fn migrate_config(legacy: &std::path::Path, file: &std::path::Path) {
    let Ok(text) = std::fs::read_to_string(legacy) else { return };
    if serde_json::from_str::<Info>(&text).is_err() {
        return;
    }
    if let Some(parent) = file.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    write_private_new(file, &text);
}

/// `wx` mode-0600 write (fails silently when the destination exists).
fn write_private_new(path: &std::path::Path, text: &str) {
    #[cfg(unix)]
    {
        use std::io::Write as _;
        use std::os::unix::fs::OpenOptionsExt as _;
        if let Ok(mut file) =
            std::fs::OpenOptions::new().write(true).create_new(true).mode(0o600).open(path)
        {
            let _ = file.write_all(text.as_bytes());
        }
    }
    #[cfg(not(unix))]
    {
        if !path.exists() {
            let _ = std::fs::write(path, text);
        }
    }
}

/// `paths()`: migrate legacy files, then the current config + registration.
fn ensure_migrated() -> (PathBuf, PathBuf) {
    let global = roots();
    let channel = util::channel();
    let name = filename(&channel);
    let file = global.state.join(&name);
    if let Some(legacy_name) = legacy_filename(&channel) {
        migrate_registration(&global.state.join(&legacy_name), &file);
        migrate_config(&global.config.join(&legacy_name), &global.config.join(&name));
    }
    if name != "service.json" && channel != "local" {
        migrate_registration(&global.state.join("service.json"), &file);
    }
    (global.config.join(&name), file)
}

/// `ServiceConfig.read()`: migrated + parsed config, `{}` on any error.
#[must_use]
pub fn read() -> Info {
    let (config_file, _) = ensure_migrated();
    std::fs::read_to_string(config_file)
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

/// `ServiceConfig.write()`: `JSON.stringify(value, null, 2) + "\n"` via a
/// 0600 temp file + rename.
pub fn write(value: &Info) -> std::io::Result<()> {
    let (config_file, _) = ensure_migrated();
    if let Some(parent) = config_file.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut text = serde_json::to_string_pretty(value).expect("config serializes");
    text.push('\n');
    #[cfg(unix)]
    {
        use std::io::Write as _;
        use std::os::unix::fs::OpenOptionsExt as _;
        let temp = config_file.with_extension("tmp");
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(&temp)?;
        file.write_all(text.as_bytes())?;
        drop(file);
        std::fs::rename(&temp, &config_file)
    }
    #[cfg(not(unix))]
    {
        std::fs::write(&config_file, text)
    }
}

/// `ServiceConfig.password()`: the persisted password or a fresh one,
/// persisted on first use.
#[must_use]
pub fn password(value: Option<&str>) -> String {
    let existing = read();
    if value.is_none() {
        if let Some(found) = existing.password {
            return found;
        }
    }
    let next = value.map_or_else(util::random_password, str::to_owned);
    let _ = write(&Info { password: Some(next.clone()), ..existing });
    next
}

async fn stop_service() {
    lifecycle::stop(StopOptions {
        file: Some(registration_file()),
        pty: lifecycle::PtyStop::Clear,
        timing: lifecycle::EnsureTiming::default(),
    })
    .await;
}

/// `ServiceConfig.get(key?, name?)`.
pub fn get(key: Option<&str>, name: Option<&str>) -> Result<String, String> {
    let Some(key) = key else {
        let mut safe = read();
        safe.password = None;
        return Ok(serde_json::to_string_pretty(&safe).expect("config serializes"));
    };
    if !matches!(key, "disabled" | "hostname" | "port" | "password" | "cors" | "env") {
        return Err(format!("Unknown service config key: {key}"));
    }
    if key != "env" && name.is_some() {
        return Err(format!("Usage: opencode service get {key}"));
    }
    let info = read();
    match key {
        "disabled" => Ok(info.disabled.unwrap_or(false).to_string()),
        "hostname" => Ok(info.hostname.unwrap_or_default()),
        "port" => Ok(info.port.map(|p| p.to_string()).unwrap_or_default()),
        "password" => Ok(password(None)),
        "cors" => {
            Ok(serde_json::to_string_pretty(&info.cors.unwrap_or_default()).expect("serializes"))
        }
        "env" => {
            let env = info.env.unwrap_or_default();
            match name {
                None => Ok(serde_json::to_string_pretty(&env).expect("serializes")),
                Some(name) => Ok(env.get(name).cloned().unwrap_or_default()),
            }
        }
        _ => Err(format!("Unknown service config key: {key}")),
    }
}

/// `ServiceConfig.set(key, value, nestedValue?)`.
pub async fn set(key: &str, value: &str, nested_value: Option<&str>) -> Result<(), String> {
    if !matches!(key, "disabled" | "hostname" | "port" | "password" | "cors" | "env") {
        return Err(format!("Unknown service config key: {key}"));
    }
    if key != "env" && nested_value.is_some() {
        return Err(format!("Usage: opencode service set {key} <value>"));
    }
    let mut info = read();
    match key {
        "disabled" => {
            if value != "true" && value != "false" {
                return Err("Disabled must be true or false".to_owned());
            }
            if value == "true" {
                stop_service().await;
            }
            info.disabled = Some(value == "true");
        }
        "hostname" => {
            stop_service().await;
            info.hostname = Some(value.to_owned());
        }
        "port" => {
            let port: u16 =
                value.parse().map_err(|_| "Port must be between 1 and 65535".to_owned())?;
            if !(1..=65535).contains(&port) {
                return Err("Port must be between 1 and 65535".to_owned());
            }
            stop_service().await;
            info.port = Some(port);
        }
        "password" => {
            stop_service().await;
            let _ = password(Some(value));
            return Ok(());
        }
        "cors" => {
            let cors: Vec<String> = value.split(',').map(str::trim).map(str::to_owned).collect();
            for origin in &cors {
                let Ok(url) = reqwest::Url::parse(origin) else {
                    return Err(cors_message());
                };
                if !matches!(url.scheme(), "http" | "https")
                    || url.as_str().trim_end_matches('/') != origin
                {
                    return Err(cors_message());
                }
            }
            stop_service().await;
            info.cors = Some(cors);
        }
        "env" => {
            let Some(nested) = nested_value else {
                return Err("Usage: opencode service set env <key> <value>".to_owned());
            };
            stop_service().await;
            let env = info.env.get_or_insert_with(BTreeMap::new);
            env.insert(value.to_owned(), nested.to_owned());
        }
        _ => return Err(format!("Unknown service config key: {key}")),
    }
    write(&info).map_err(|error| error.to_string())
}

fn cors_message() -> String {
    "CORS must be a comma-separated list of HTTP(S) origins without paths or trailing slashes"
        .to_owned()
}

/// `ServiceConfig.unset(key, name?)`.
pub async fn unset(key: &str, name: Option<&str>) -> Result<(), String> {
    if !matches!(key, "disabled" | "hostname" | "port" | "password" | "cors" | "env") {
        return Err(format!("Unknown service config key: {key}"));
    }
    if key != "env" && name.is_some() {
        return Err(format!("Usage: opencode service unset {key}"));
    }
    let mut info = read();
    match key {
        "disabled" => {
            info.disabled = None;
        }
        "hostname" => {
            stop_service().await;
            info.hostname = None;
        }
        "port" => {
            stop_service().await;
            info.port = None;
        }
        "password" => {
            stop_service().await;
            info.password = None;
        }
        "cors" => {
            stop_service().await;
            info.cors = None;
        }
        "env" => {
            let Some(name) = name else {
                return Err("Usage: opencode service unset env <key>".to_owned());
            };
            stop_service().await;
            if let Some(env) = info.env.as_mut() {
                env.remove(name);
                if env.is_empty() {
                    info.env = None;
                }
            }
        }
        _ => return Err(format!("Unknown service config key: {key}")),
    }
    write(&info).map_err(|error| error.to_string())
}

/// `ServiceConfig.options({checkVersion})`: the `EnsureOptions` binding —
/// registration file, exact version pin when checking, config env, and the
/// self-spawn command `serve --service`.
#[must_use]
pub fn options(check_version: bool) -> EnsureOptions {
    let (_, file) = ensure_migrated();
    let mut command = util::self_command();
    command.extend(["serve".to_owned(), "--service".to_owned()]);
    EnsureOptions {
        file: Some(file),
        version: check_version.then(|| util::VERSION.to_owned()),
        command: Some(command),
        env: read().env.unwrap_or_default().into_iter().collect(),
        on_start: None,
        timing: lifecycle::EnsureTiming::default(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sandboxed() -> tempfile::TempDir {
        let dir = tempfile::tempdir_in("/tmp/opencode").expect("sandbox");
        // SAFETY: test-scoped env mutation under a lock would be nicer, but
        // these tests run single-threaded per file.
        std::env::set_var("OPENCODE_TEST_HOME", dir.path());
        std::env::remove_var("XDG_CONFIG_HOME");
        std::env::remove_var("XDG_STATE_HOME");
        std::env::remove_var("XDG_DATA_HOME");
        std::env::remove_var("XDG_CACHE_HOME");
        dir
    }

    #[test]
    fn filenames_per_channel() {
        assert_eq!(filename("latest"), "service.json");
        assert_eq!(filename("local"), "service-local.json");
        assert_eq!(filename("we ird/ch"), "service-we-ird-ch.json");
    }

    #[test]
    fn default_ports() {
        assert_eq!(default_port("latest"), 0xc0de);
        assert_eq!(default_port("local"), 0xc0df);
        let port = default_port("custom");
        assert!((10_000..60_000).contains(&port), "{port}");
        assert_eq!(port, default_port("custom"), "deterministic");
    }

    #[test]
    fn rust_default_port_is_disjoint_from_v2_ports() {
        // Deterministic per channel, never v2's fixed ports, and stable
        // across releases (golden values from the sha1 derivation).
        assert_eq!(rust_default_port("latest"), 52568);
        assert_eq!(rust_default_port("local"), 53131);
        for channel in ["latest", "dev", "beta", "next", "local"] {
            let port = rust_default_port(channel);
            assert_ne!(
                port, 0xc0de,
                "channel {channel} must not collide with v2 latest/dev/beta/next"
            );
            assert_ne!(port, 0xc0df, "channel {channel} must not collide with v2 local");
        }
    }

    #[test]
    fn version_channel_membership() {
        assert!(version_belongs_to_channel(Some("2.0.24"), "latest"));
        assert!(version_belongs_to_channel(Some("0.0.0-beta-1.2"), "beta"));
        assert!(!version_belongs_to_channel(Some("0.0.0-beta-x"), "beta"));
        assert!(!version_belongs_to_channel(None, "latest"));
    }

    #[tokio::test]
    async fn set_get_round_trip() {
        let _sandbox = sandboxed();
        set("hostname", "127.0.0.1", None).await.expect("set hostname");
        assert_eq!(get(Some("hostname"), None).expect("get"), "127.0.0.1");
        set("env", "FOO", Some("bar")).await.expect("set env");
        assert_eq!(get(Some("env"), Some("FOO")).expect("get env"), "bar");
        unset("env", Some("FOO")).await.expect("unset env");
        assert_eq!(get(Some("env"), Some("FOO")).expect("get env"), "");
        assert!(set("disabled", "yes", None).await.is_err());
        assert!(set("port", "0", None).await.is_err());
        assert!(set("bogus", "x", None).await.is_err());
    }

    #[test]
    fn password_persists() {
        let _sandbox = sandboxed();
        let first = password(None);
        assert_eq!(first.len(), 43);
        assert_eq!(password(None), first, "second read returns the persisted value");
    }
}

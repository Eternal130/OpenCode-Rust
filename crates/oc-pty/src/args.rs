//! Daemon spawn arguments — mirror of `daemon.ts:235`
//! `["daemon", "--runtime-dir", root, "--name", id]` plus the
//! test-overridable orphan grace (design §5.1, gate finding 11).

use std::path::PathBuf;
use std::time::Duration;

/// Default orphan grace (design §5.1): CONTINUOUSLY zero terminals AND
/// no owner connection for 60 s.
pub const DEFAULT_ORPHAN_GRACE_SECS: u64 = 60;

/// The parsed `daemon` subcommand configuration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DaemonConfig {
    /// `<root>` of `<root>/<id>` (§5.3).
    pub runtime_root: PathBuf,
    /// `<id>` — the client's fresh random name (§8.2).
    pub name: String,
    /// Orphan-grace self-exit window (§5.1).
    pub orphan_grace: Duration,
}

/// Parses the `daemon` subcommand arguments (`argv` after the binary
/// name). Unknown flags and missing values are usage errors (exit 2).
pub fn parse_daemon_args(args: &[String]) -> Result<DaemonConfig, String> {
    let mut iter = args.iter();
    match iter.next().map(String::as_str) {
        Some("daemon") => {}
        Some(other) => return Err(format!("unknown subcommand: {other} (expected: daemon)")),
        None => {
            return Err(
                "usage: oc-pty daemon --runtime-dir <dir> --name <id> [--orphan-grace <secs>]"
                    .to_owned(),
            )
        }
    }
    let mut runtime_root: Option<PathBuf> = None;
    let mut name: Option<String> = None;
    let mut orphan_grace_secs: Option<u64> = None;
    while let Some(arg) = iter.next() {
        let flag = arg.as_str();
        let mut value = || -> Result<String, String> {
            iter.next().cloned().ok_or_else(|| format!("{flag} requires a value"))
        };
        match flag {
            "--runtime-dir" => {
                if runtime_root.is_some() {
                    return Err(format!("{flag} given twice"));
                }
                runtime_root = Some(PathBuf::from(value()?));
            }
            "--name" => {
                if name.is_some() {
                    return Err(format!("{flag} given twice"));
                }
                name = Some(value()?);
            }
            "--orphan-grace" => {
                if orphan_grace_secs.is_some() {
                    return Err(format!("{flag} given twice"));
                }
                orphan_grace_secs =
                    Some(value()?.parse().map_err(|_| format!("{flag} expects seconds"))?);
            }
            other => return Err(format!("unknown argument: {other}")),
        }
    }
    Ok(DaemonConfig {
        runtime_root: runtime_root.ok_or_else(|| "--runtime-dir is required".to_owned())?,
        name: name.ok_or_else(|| "--name is required".to_owned())?,
        orphan_grace: Duration::from_secs(orphan_grace_secs.unwrap_or(DEFAULT_ORPHAN_GRACE_SECS)),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(std::string::ToString::to_string).collect()
    }

    #[test]
    fn parses_the_ref_spawn_surface() {
        let config = parse_daemon_args(&args(&["daemon", "--runtime-dir", "/r", "--name", "n1"]))
            .expect("parse");
        assert_eq!(config.runtime_root, PathBuf::from("/r"));
        assert_eq!(config.name, "n1");
        assert_eq!(config.orphan_grace, Duration::from_secs(DEFAULT_ORPHAN_GRACE_SECS));
    }

    #[test]
    fn orphan_grace_override_is_honored() {
        let config = parse_daemon_args(&args(&[
            "daemon",
            "--runtime-dir",
            "/r",
            "--name",
            "n1",
            "--orphan-grace",
            "1",
        ]))
        .expect("parse");
        assert_eq!(config.orphan_grace, Duration::from_secs(1));
        let zero = parse_daemon_args(&args(&[
            "daemon",
            "--runtime-dir",
            "/r",
            "--name",
            "n1",
            "--orphan-grace",
            "0",
        ]))
        .expect("parse");
        assert_eq!(zero.orphan_grace, Duration::ZERO);
    }

    #[test]
    fn usage_errors() {
        assert!(parse_daemon_args(&args(&[])).is_err());
        assert!(parse_daemon_args(&args(&["serve"])).is_err());
        assert!(parse_daemon_args(&args(&["daemon", "--name", "n"])).is_err());
        assert!(parse_daemon_args(&args(&["daemon", "--runtime-dir", "/r"])).is_err());
        assert!(parse_daemon_args(&args(&[
            "daemon",
            "--runtime-dir",
            "/r",
            "--name",
            "n",
            "--bogus"
        ]))
        .is_err());
        assert!(parse_daemon_args(&args(&[
            "daemon",
            "--runtime-dir",
            "/r",
            "--name",
            "n",
            "--orphan-grace",
            "x"
        ]))
        .is_err());
    }
}

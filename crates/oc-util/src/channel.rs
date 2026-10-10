//! Release-channel resolution — the runtime port of v2's build-time
//! `OPENCODE_CHANNEL` define (`packages/cli/src/version.ts`).
//!
//! v2 bakes the channel into the binary at build time (there is no
//! runtime override: `const channel = typeof OPENCODE_CHANNEL ===
//! "string" ? OPENCODE_CHANNEL : "local"`); the pinned reference build
//! is compiled with `OPENCODE_CHANNEL="stable"`. The Rust port builds
//! one binary, so the channel is read from the `OPENCODE_CHANNEL`
//! environment variable instead.
//!
//! This is THE one channel source for the whole workspace (P10-DB1):
//! `oc-cli serve` (service-config filenames, registration), the
//! engine's database-path resolution (`oc_core::database::path`), and
//! anything else channel-scoped must agree on it. The default is
//! `latest` — the npm release channel — which is also in the
//! shared-DB-filename list, so an unset environment keeps the
//! historical `opencode.db` filename.

/// Fallback channel when `OPENCODE_CHANNEL` is unset or empty (the npm
/// release channel; v2's from-source fallback would be `local`, but the
/// port ships release-style binaries).
pub const DEFAULT_CHANNEL: &str = "latest";

/// `OPENCODE_CHANNEL` (version.ts): the release channel this process
/// runs as. Empty values are ignored (treated as unset).
#[must_use]
pub fn current() -> String {
    std::env::var("OPENCODE_CHANNEL")
        .ok()
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| DEFAULT_CHANNEL.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::lock_env;
    use std::sync::MutexGuard;

    struct Env {
        _guard: MutexGuard<'static, ()>,
        saved: Option<std::ffi::OsString>,
    }

    impl Drop for Env {
        fn drop(&mut self) {
            match self.saved.as_ref() {
                Some(value) => std::env::set_var("OPENCODE_CHANNEL", value),
                None => std::env::remove_var("OPENCODE_CHANNEL"),
            }
        }
    }

    fn locked() -> Env {
        let guard = lock_env();
        let saved = std::env::var_os("OPENCODE_CHANNEL");
        std::env::remove_var("OPENCODE_CHANNEL");
        Env { _guard: guard, saved }
    }

    #[test]
    fn channel_defaults_to_latest() {
        let _env = locked();
        assert_eq!(current(), "latest");
        std::env::set_var("OPENCODE_CHANNEL", "");
        assert_eq!(current(), "latest", "empty values are ignored");
    }

    #[test]
    fn channel_reads_the_environment() {
        let _env = locked();
        std::env::set_var("OPENCODE_CHANNEL", "stable");
        assert_eq!(current(), "stable");
        std::env::set_var("OPENCODE_CHANNEL", "edge/2");
        assert_eq!(current(), "edge/2");
    }
}

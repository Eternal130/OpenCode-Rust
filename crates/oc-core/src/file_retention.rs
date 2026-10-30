//! G-14 — configurable file-retention sweep cadence.
//!
//! The two retention sweeps (spilled tool output + shell captures) ran on a
//! hardcoded hourly interval. Mirroring the ref's caller-provided
//! `Duration` semantics (`FileRetention.cleanup(fs, files, retention)` in
//! `file-retention.ts`), the sweep interval and the retention window are
//! now caller-provided with production defaults that preserve the previous
//! behavior (hourly sweep, 7-day retention). The composition root seeds
//! them from `experimental.file_retention.cleanup_interval` in the config.

use std::time::Duration;

use oc_schema::config::Experimental;

/// Default sweep interval — hourly (the previous hardcoded cadence).
pub const DEFAULT_CLEANUP_INTERVAL: Duration = Duration::from_secs(3_600);
/// Default retention window — 7 days (tool-output/shell `RETENTION_MS`).
pub const DEFAULT_RETENTION: Duration = Duration::from_millis(7 * 24 * 60 * 60 * 1_000);

/// Effective sweep settings (defaults preserve the pre-G-14 behavior).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FileRetentionSettings {
    /// How often the retention sweeps run.
    pub cleanup_interval: Duration,
}

impl Default for FileRetentionSettings {
    fn default() -> Self {
        Self { cleanup_interval: DEFAULT_CLEANUP_INTERVAL }
    }
}

impl FileRetentionSettings {
    /// Resolves the sweep interval from the config's `experimental` section
    /// (`file_retention.cleanup_interval`, milliseconds). Absent or zero
    /// keeps the default hourly cadence.
    #[must_use]
    pub fn from_experimental(experimental: Option<&Experimental>) -> Self {
        let interval = experimental
            .and_then(|experimental| experimental.file_retention.as_ref())
            .and_then(|retention| retention.cleanup_interval)
            .filter(|ms| *ms > 0)
            .map_or(DEFAULT_CLEANUP_INTERVAL, Duration::from_millis);
        Self { cleanup_interval: interval }
    }

    /// Same resolution over already-loaded config entries (highest-priority
    /// document wins, matching the `latest` fold used for every other
    /// config key).
    #[must_use]
    pub fn from_entries(entries: &[oc_schema::config::Entry]) -> Self {
        Self::from_experimental(
            crate::config::latest(entries, |info| info.experimental.clone()).as_ref(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_retention_settings_default_to_hourly() {
        assert_eq!(FileRetentionSettings::default().cleanup_interval, Duration::from_secs(3_600));
        assert_eq!(DEFAULT_RETENTION, Duration::from_millis(7 * 24 * 60 * 60 * 1_000));
    }

    #[test]
    fn file_retention_settings_honor_a_configured_interval() {
        let experimental = serde_json::from_value::<Experimental>(
            serde_json::json!({"file_retention": {"cleanup_interval": 120_000}}),
        )
        .unwrap();
        assert_eq!(
            FileRetentionSettings::from_experimental(Some(&experimental)).cleanup_interval,
            Duration::from_secs(120)
        );
    }

    #[test]
    fn file_retention_settings_ignore_absent_or_zero_intervals() {
        assert_eq!(
            FileRetentionSettings::from_experimental(None).cleanup_interval,
            DEFAULT_CLEANUP_INTERVAL
        );
        let empty = Experimental::default();
        assert_eq!(
            FileRetentionSettings::from_experimental(Some(&empty)).cleanup_interval,
            DEFAULT_CLEANUP_INTERVAL
        );
        let zero = serde_json::from_value::<Experimental>(
            serde_json::json!({"file_retention": {"cleanup_interval": 0}}),
        )
        .unwrap();
        assert_eq!(
            FileRetentionSettings::from_experimental(Some(&zero)).cleanup_interval,
            DEFAULT_CLEANUP_INTERVAL,
            "zero would stall tokio's interval; the hourly default wins"
        );
    }
}

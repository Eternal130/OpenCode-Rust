//! Gate 0 strict-mode environment checks (PARITY-LOOP.md §8.3, ledger
//! B0-8): the single shared decision point for every harness self-skip
//! that gates on environment availability (bun, the reference checkout,
//! cargo).
//!
//! - `OC_GATE_STRICT` unset (default daily run): skips stay skips —
//!   [`skip_or_fail`] prints the standard note and returns `true`, and
//!   the caller returns `Ok(())` exactly as before.
//! - `OC_GATE_STRICT=1` (batch-merge / release gate runs): a missing
//!   prerequisite FAILS the test instead — [`skip_or_fail`] panics with
//!   an error naming exactly what is missing, so an unusable reference
//!   or Rust side can never produce a fake green (§8.5: a skip is not a
//!   pass; only `gate_ran: true` counts).
//! - `OC_GATE_ALLOW_SKIP=1` re-enables skip behavior for interactive
//!   local runs even under strict mode (§8.3); such runs still cannot
//!   enter `verified` evidence.
//!
//! Every availability gate routes through [`skip_or_fail`] (see
//! [`crate::reference::skip_if_no_bun`] and
//! [`crate::rust_server::skip_if_no_cargo`]) so the skip-or-fail decision
//! is made in exactly one place.

/// Whether gate strict mode is active: `OC_GATE_STRICT=1`, unless the
/// daily-local override `OC_GATE_ALLOW_SKIP=1` re-enables skips.
#[must_use]
pub fn strict() -> bool {
    let flag = |name: &str| std::env::var(name).ok().as_deref() == Some("1");
    flag("OC_GATE_STRICT") && !flag("OC_GATE_ALLOW_SKIP")
}

/// The shared skip-or-fail decision (see the module docs).
///
/// Returns `true` when the caller should skip — non-strict mode only;
/// the standard skip note is printed here, so callers must not duplicate
/// it. Under `OC_GATE_STRICT=1` this panics instead, failing the test
/// with `missing` naming exactly what is unavailable.
#[must_use]
pub fn skip_or_fail(missing: &str) -> bool {
    assert!(
        !strict(),
        "OC_GATE_STRICT=1: gate prerequisite missing — {missing}; strict mode forbids \
         self-skip (fix the environment, or set OC_GATE_ALLOW_SKIP=1 for a local \
         non-verifiable run; see PARITY-LOOP.md §8.3 Gate 0)"
    );
    eprintln!("skipping: {missing}");
    true
}

#[cfg(test)]
mod tests {
    // Strictness is decided purely from the two env vars; these unit
    // tests pin the truth table (each nextest test runs in its own
    // process, so the env mutations are safe).
    fn set(name: &str, value: Option<&str>) {
        match value {
            Some(value) => std::env::set_var(name, value),
            None => std::env::remove_var(name),
        }
    }

    #[test]
    fn strict_requires_the_exact_flag() {
        set("OC_GATE_ALLOW_SKIP", None);
        for (value, expected) in [("1", true), ("", false), ("0", false), ("true", false)] {
            set("OC_GATE_STRICT", Some(value));
            assert_eq!(super::strict(), expected, "OC_GATE_STRICT={value:?}");
        }
        set("OC_GATE_STRICT", None);
        assert!(!super::strict(), "unset must mean non-strict (daily local)");
    }

    #[test]
    fn allow_skip_overrides_strict() {
        set("OC_GATE_STRICT", Some("1"));
        set("OC_GATE_ALLOW_SKIP", Some("1"));
        assert!(!super::strict(), "OC_GATE_ALLOW_SKIP=1 re-enables skips (§8.3)");
        set("OC_GATE_ALLOW_SKIP", None);
        assert!(super::strict());
        set("OC_GATE_STRICT", None);
    }
}

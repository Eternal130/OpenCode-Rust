//! The oc-tui keymap surface — a re-export of the oc-tui-core
//! interaction-contract crate's keymap segment (Wave A moved the
//! engine, table and facade into `oc-tui-core`), plus the two
//! oc-tui-side helpers that shape config errors into toasts (they
//! depend on the crate's `ToastOptions`).

pub use oc_tui_core::keymap::{
    default_table, engine, log_skip, parse_keybind_overrides, take_config_errors, Chord, Clock,
    CommandDef, CommandGroup, KeyStroke, KeybindOverride, Keymap, ManualClock, Outcome,
    OwnedCommandDef, SystemClock, LEADER_DEFAULT, LEADER_TIMEOUT_MS,
};

/// Shape one pending config error as a toast (the F5 surface):
///
/// * `theme {name}: {reason} (skipped)` → the reference's
///   `ThemeErrorToast` — variant `error`, title
///   `Failed to load theme: {name}`, message the parser's reason;
/// * every other skip (bad keybinds, unparsable config documents) → an
///   `error` toast carrying the reason as the message.
#[must_use]
pub fn config_error_toast(reason: &str) -> Option<crate::components::toast::ToastOptions> {
    let strip = |text: &str| {
        text.trim()
            .trim_end_matches(" (skipped)")
            .trim_end_matches(" (defaults apply)")
            .trim()
            .to_owned()
    };
    if let Some(rest) = reason.strip_prefix("theme ") {
        let (name, message) = rest.split_once(':')?;
        return Some(crate::components::toast::ToastOptions {
            title: Some(format!("Failed to load theme: {}", name.trim())),
            message: strip(message),
            variant: "error",
            duration_ms: crate::components::toast::TOAST_DEFAULT_DURATION_MS,
            action_label: None,
        });
    }
    let message = strip(reason);
    if message.is_empty() {
        return None;
    }
    Some(crate::components::toast::ToastOptions {
        title: None,
        message,
        variant: "error",
        duration_ms: crate::components::toast::TOAST_DEFAULT_DURATION_MS,
        action_label: None,
    })
}

/// Drain every pending config error as toasts (the pump's one-call
/// seam — `store.toast_show` accepts each in order).
#[must_use]
pub fn drain_config_error_toasts() -> Vec<crate::components::toast::ToastOptions> {
    take_config_errors().iter().filter_map(|reason| config_error_toast(reason)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// F5: bad keybinds surface as error toasts, not stderr only —
    /// every skip lands in the sink and shapes into a toast.
    #[test]
    fn config_errors_surface_as_toasts() {
        let _ = take_config_errors(); // isolate the sink
        let _ = parse_keybind_overrides(&serde_json::json!({ "app.exit": 42 }));
        let errors = take_config_errors();
        assert!(
            errors.iter().any(|reason| reason.starts_with("keybind \"app.exit\"")),
            "the unparseable value recorded: {errors:?}"
        );
        assert!(take_config_errors().is_empty(), "the drain is total");
        // Nothing pending after the drain above — record one of each
        // kind and check the shaping.
        log_skip("theme neon: unknown color token (skipped)");
        log_skip("keybind \"app.exit\": unparseable value 42 (skipped)");
        let toasts = drain_config_error_toasts();
        assert_eq!(toasts.len(), 2);
        let theme = &toasts[0];
        assert_eq!(theme.title.as_deref(), Some("Failed to load theme: neon"));
        assert_eq!(theme.message, "unknown color token");
        assert_eq!(theme.variant, "error");
        assert_eq!(toasts[1].title, None, "non-theme skips carry the message only");
        assert_eq!(toasts[1].message, "keybind \"app.exit\": unparseable value 42");
    }
}

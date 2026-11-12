//! Terminal/OS description strings (G-388; ref
//! `packages/tui/src/util/system.ts`). Sole consumers are
//! informational: the crash-issue URL's `terminal=`/`os=` params
//! (error-component.tsx:227-228) and the debug dialog's Terminal row.

/// `describeOS()` (system.ts:3-13): `<platform> <release> (<arch>)`.
/// Platform/arch from `std::env::consts`; the kernel release is read
/// from `/proc/sys/kernel/osrelease` on Linux (trimmed) — on other
/// platforms the release is omitted (bounded: `node:os.release()` has
/// no equivalent in `std`, and the param is informational only).
#[must_use]
pub fn describe_os() -> String {
    #[cfg(target_os = "linux")]
    let release = std::fs::read_to_string("/proc/sys/kernel/osrelease")
        .map(|text| format!(" {}", text.trim()))
        .unwrap_or_default();
    #[cfg(not(target_os = "linux"))]
    let release = String::new();
    let arch = match std::env::consts::ARCH {
        "x86_64" => "x64",
        "aarch64" => "arm64",
        other => other,
    };
    format!("{}{release} ({arch})", platform_name())
}

fn platform_name() -> &'static str {
    match std::env::consts::OS {
        "macos" => "macOS",
        "windows" => "Windows",
        "linux" => "Linux",
        _ => "unknown",
    }
}

/// `describeTerminal()` (system.ts:15-19), verbatim semantics:
/// `TERM_PROGRAM || TERM || "unknown"`, plus ` <TERM_PROGRAM_VERSION>`
/// when set, plus `" in tmux"` when TMUX is set / `" in screen"` when
/// STY is set.
#[must_use]
pub fn describe_terminal() -> String {
    let program = std::env::var_os("TERM_PROGRAM")
        .filter(|value| !value.is_empty())
        .or_else(|| std::env::var_os("TERM").filter(|value| !value.is_empty()))
        .map(|value| value.to_string_lossy().into_owned())
        .unwrap_or_else(|| "unknown".to_owned());
    let version = std::env::var_os("TERM_PROGRAM_VERSION")
        .filter(|value| !value.is_empty())
        .map(|value| format!(" {}", value.to_string_lossy()))
        .unwrap_or_default();
    let multiplexer = if std::env::var_os("TMUX").is_some_and(|v| !v.is_empty()) {
        " in tmux"
    } else if std::env::var_os("STY").is_some_and(|v| !v.is_empty()) {
        " in screen"
    } else {
        ""
    };
    format!("{program}{version}{multiplexer}")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Serializes env mutation (parallel tests in one process).
    static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    #[test]
    fn describe_terminal_covers_every_branch() {
        let _guard = ENV_LOCK.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let names = ["TERM_PROGRAM", "TERM", "TERM_PROGRAM_VERSION", "TMUX", "STY"];
        let saved: Vec<(_, _)> = names.iter().map(|name| (*name, std::env::var_os(name))).collect();
        for name in names {
            std::env::remove_var(name);
        }
        let restore = || {
            for (name, value) in &saved {
                match value {
                    Some(value) => std::env::set_var(name, value),
                    None => std::env::remove_var(name),
                }
            }
        };

        // Unknown fallback: neither TERM_PROGRAM nor TERM.
        assert_eq!(describe_terminal(), "unknown");
        // TERM alone.
        std::env::set_var("TERM", "xterm-256color");
        assert_eq!(describe_terminal(), "xterm-256color");
        // TERM_PROGRAM wins over TERM.
        std::env::set_var("TERM_PROGRAM", "iTerm.app");
        assert_eq!(describe_terminal(), "iTerm.app");
        // G-388: TERM_PROGRAM_VERSION appends ` <version>`.
        std::env::set_var("TERM_PROGRAM_VERSION", "3.5.13");
        assert_eq!(describe_terminal(), "iTerm.app 3.5.13");
        // tmux suffix.
        std::env::set_var("TMUX", "/tmp/tmux-0/default,123,0");
        assert_eq!(describe_terminal(), "iTerm.app 3.5.13 in tmux");
        // screen wins when TMUX is unset.
        std::env::remove_var("TMUX");
        std::env::set_var("STY", "mysession");
        assert_eq!(describe_terminal(), "iTerm.app 3.5.13 in screen");
        restore();
    }

    #[test]
    fn describe_os_has_platform_and_arch() {
        let text = describe_os();
        assert!(text.contains(platform_name()), "platform name present: {text}");
        assert!(text.contains('('), "arch is parenthesized: {text}");
        assert!(text.ends_with(')'), "arch closes the string: {text}");
    }
}

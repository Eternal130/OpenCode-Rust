//! System dark/light detection (`tui/theme/system.ts terminalMode`).
//!
//! The reference classifies a terminal's detected background color with a
//! Rec.601 luma rule on 0-1 components: `0.299r + 0.587g + 0.114b > 0.5`
//! → light, else dark; a missing background is undecidable (`undefined`).
//!
//! Terminal-context divergence (documented): the actual probing of the
//! terminal — the OSC 11 background query with timeout, and the dark
//! fallback for headless pipes / timeouts / unparseable answers — lives in
//! oc-tui (`tui/src/theme.rs detect_system_mode`), which owns tty I/O and
//! the renderer (`renderer.themeMode` / `waitForThemeMode` in ref). This
//! module only hosts the pure classification rule.

use crate::types::Mode;

/// Classify a terminal background color (`system.ts terminalMode`):
/// Rec.601 luma over the 8-bit components; the ref's 0.5 threshold on 0-1
/// components equals 127.5 on the 0-255 scale.
#[must_use]
pub fn mode_for_background(r: u8, g: u8, b: u8) -> Mode {
    let luma = 0.299 * f64::from(r) + 0.587 * f64::from(g) + 0.114 * f64::from(b);
    if luma > 127.5 {
        Mode::Light
    } else {
        Mode::Dark
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_luminance_like_terminal_mode() {
        assert_eq!(mode_for_background(0, 0, 0), Mode::Dark);
        assert_eq!(mode_for_background(10, 10, 10), Mode::Dark);
        assert_eq!(mode_for_background(40, 40, 40), Mode::Dark); // #282828 gruvbox bg
        assert_eq!(mode_for_background(23, 23, 23), Mode::Dark); // #171717 tmux bg
        assert_eq!(mode_for_background(250, 250, 250), Mode::Light);
        assert_eq!(mode_for_background(255, 255, 0), Mode::Light); // 0.886*255
        assert_eq!(mode_for_background(0, 0, 255), Mode::Dark); // 0.114*255 = 29.1
    }
}

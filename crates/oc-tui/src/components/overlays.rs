//! The app-shell state overlays (SURF-2, auditB #2):
//!
//! * **Reconnecting** (`component/reconnecting.tsx`): the full-screen
//!   scrim `RGBA(0,0,0,150)` with a centered 48-wide raised box —
//!   "Connection lost…" + "Reconnecting to the server automatically."
//!   Shows 1 s after a RETRY transition, 5 s after the initial one
//!   (app.tsx:1294-1315).
//! * **`StartupLoading`** (`component/startup-loading.tsx`): the
//!   bottom-centered pill — "Loading plugins…" 500 ms after boot goes
//!   un-ready, "Finishing startup…" once ready with a 3 s hold, at
//!   `bottom=1` full-width centered, raised background, padding 1.

use crate::render::Frame;
use crate::store::Store;
use crate::theme::ThemeStyles;

/// Paint both overlays over the frame (the run loop's frame tail):
/// `StartupLoading` (`zIndex` 5000) under `Reconnecting` (`zIndex` 10000),
/// both above the dialog layer.
pub fn overlay_app_state(frame: &mut Frame, store: &Store, styles: &ThemeStyles) {
    let now = store.clock();
    if let Some(text) = store.startup_loading_text(now) {
        super::overlays_solid::overlay_startup_loading_solid(frame, styles, text, now);
    }
    if store.reconnecting_overlay_visible(now) {
        super::overlays_solid::overlay_reconnecting_solid(
            frame,
            styles,
            now.saturating_sub(store.connection_since()),
        );
    }
    // G-06 — the migration widget shares the 10_000 z-layer and paints
    // after (above) everything while the poll reports `running`.
    if let Some(progress) = store.migration_progress() {
        super::overlays_solid::overlay_migration_solid(frame, styles, &progress, now);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::components::overlays_solid::{
        overlay_migration_solid, overlay_reconnecting_solid, overlay_startup_loading_solid,
    };
    use crate::reactive::Runtime;
    use crate::render::{Cell, CellColor};
    use crate::store::Store;

    fn frame_with(width: usize, height: usize) -> Frame {
        Frame {
            cells: vec![
                vec![Cell { bg: CellColor::Rgb(200, 200, 200), ..Cell::default() }; width];
                height
            ],
            width,
            height,
        }
    }

    /// The scrim dims every background by 150/255 and paints the
    /// centered box copy (reconnecting.tsx:8-39).
    #[test]
    fn reconnecting_scrim_and_box() {
        let styles = crate::theme::ThemeState::v2_dark().styles();
        let mut frame = frame_with(60, 12);
        overlay_reconnecting_solid(&mut frame, &styles, 0);
        // The corner dims (200 * (1 - 150/255)) ≈ 82.
        assert_eq!(frame.cell(0, 0).bg, CellColor::Rgb(82, 82, 82));
        let text = frame.text_grid().join("\n");
        assert!(text.contains("Connection lost…"), "headline: {text}");
        assert!(text.contains("Reconnecting to the server automatically."), "copy: {text}");
    }

    /// The thresholds (app.tsx:1308-1314): retry shows after 1 s, the
    /// initial connect waits 5 s; recovery hides immediately.
    #[test]
    fn reconnecting_thresholds() {
        let store = Store::new(Runtime::new());
        store.advance_clock(10_000);
        store.set_connection(crate::data::ConnectionState::Reconnecting { attempt: 1 });
        assert!(!store.reconnecting_overlay_visible(10_500), "under 1 s");
        assert!(store.reconnecting_overlay_visible(11_000), "1 s retry threshold");
        // Initial connect: 5 s.
        store.set_connection(crate::data::ConnectionState::Connecting);
        assert!(!store.reconnecting_overlay_visible(12_000));
        assert!(!store.reconnecting_overlay_visible(14_999));
        assert!(store.reconnecting_overlay_visible(15_000), "5 s initial threshold");
        // Recovery clears instantly.
        store.set_connection(crate::data::ConnectionState::Connected);
        assert!(!store.reconnecting_overlay_visible(15_001));
    }

    /// The first-boot grace (app.tsx:1300-1314): a NEVER-transitioned
    /// `Connecting` state (first boot / hung handshake — the only
    /// anchor writer is the `set_connection` transition arm) must not
    /// surface the scrim from frame 0 off the 0-init anchor.
    /// Construction can't seed it (the clock signal starts at 0 and
    /// only becomes wall time in the run loop), so the read site
    /// treats 0 as "first observation": the anchor latches at the
    /// first look and the 5 s grace runs from there.
    #[test]
    fn reconnecting_initial_grace_from_first_observation() {
        let store = Store::new(Runtime::new());
        // No `set_connection` call: the state stays the constructed
        // `Connecting` and `connection_since` stays 0.
        assert_eq!(store.connection(), crate::data::ConnectionState::Connecting);
        assert_eq!(store.connection_since(), 0);
        assert!(
            !store.reconnecting_overlay_visible(20_000),
            "frame 0 stays clean — the wall-derived `now` against the 0 anchor must not read as \
             20 s elapsed"
        );
        assert_eq!(store.connection_since(), 20_000, "the first observation latches the anchor");
        assert!(!store.reconnecting_overlay_visible(24_999), "still inside the 5 s grace");
        assert!(store.reconnecting_overlay_visible(25_000), "5 s after the first observation");
    }

    /// `StartupLoading` (startup-loading.tsx:42-46): 500 ms wait, 3 s
    /// hold after ready with the copy flip.
    #[test]
    fn startup_loading_wait_and_hold() {
        let store = Store::new(Runtime::new());
        assert_eq!(store.startup_loading_text(0), None, "not yet armed");
        assert_eq!(store.startup_loading_text(499), None, "under 500 ms");
        assert_eq!(store.startup_loading_text(500), Some("Loading plugins…"), "shows at 500 ms");
        store.set_plugins_ready();
        assert_eq!(
            store.startup_loading_text(3_400),
            Some("Finishing startup…"),
            "the 3 s hold keeps the pill with the finishing copy"
        );
        assert_eq!(store.startup_loading_text(3_500), None, "hold expires");
        // A ready-before-showing boot never shows the pill.
        let fresh = Store::new(Runtime::new());
        fresh.set_plugins_ready();
        assert_eq!(fresh.startup_loading_text(10_000), None);
    }

    /// CONFIG-E3 — `OPENCODE_FAST_BOOT` / `skipInitialLoading`
    /// (app.tsx:351 + 1389-1391): the pill never mounts — neither the
    /// 500 ms wait nor the ready hold ever produces text — and the
    /// overlay painter leaves the frame clean.
    #[test]
    fn startup_loading_skipped_by_fast_boot() {
        let store = Store::new(Runtime::new());
        store.set_skip_initial_loading(true);
        assert_eq!(store.startup_loading_text(0), None, "wait never arms");
        assert_eq!(
            store.startup_loading_text(10_000),
            None,
            "the 500 ms wait never fires — the component is unmounted (app.tsx:1389-1391)"
        );
        store.set_plugins_ready();
        assert_eq!(
            store.startup_loading_text(20_000),
            None,
            "the ready hold is equally irrelevant — nothing was ever shown"
        );

        // The painter side: `overlay_app_state` leaves the pill row
        // empty (the clock advances past 500 ms so an armed pill would
        // show if the skip were ignored).
        let styles = crate::theme::ThemeState::v2_dark().styles();
        let mut frame = frame_with(40, 10);
        store.set_connection(crate::data::ConnectionState::Connected);
        overlay_app_state(&mut frame, &store, &styles);
        let row = String::from(frame.text_grid()[8].trim());
        assert!(row.is_empty(), "pill row must stay clean: {row}");

        // Without the flag the same clock position shows the pill.
        let plain = Store::new(Runtime::new());
        plain.set_connection(crate::data::ConnectionState::Connected);
        assert_eq!(plain.startup_loading_text(0), None, "arms the wait");
        assert_eq!(
            plain.startup_loading_text(600),
            Some("Loading plugins…"),
            "unskipped boot still shows the pill"
        );
    }

    /// CONFIG-E3 — the env parse mirrors JS
    /// `Boolean(process.env.OPENCODE_FAST_BOOT)` (app.tsx:351):
    /// unset and the empty string are false; ANY non-empty value —
    /// including `"0"` and `"false"` — is true.
    #[test]
    fn fast_boot_env_semantics() {
        assert!(!crate::app::fast_boot_from_env(None), "unset → false");
        assert!(!crate::app::fast_boot_from_env(Some("")), "empty string → false");
        assert!(crate::app::fast_boot_from_env(Some("1")));
        assert!(crate::app::fast_boot_from_env(Some("0")), "JS truthiness: \"0\" → true");
        assert!(crate::app::fast_boot_from_env(Some("false")), "\"false\" → true");
        assert!(crate::app::fast_boot_from_env(Some(" ")));
    }

    /// The pill renders above the frame content at bottom=1.
    #[test]
    fn startup_pill_geometry() {
        let styles = crate::theme::ThemeState::v2_dark().styles();
        let mut frame = frame_with(40, 10);
        overlay_startup_loading_solid(&mut frame, &styles, "Loading plugins…", 0);
        let row = frame.text_grid()[8].clone();
        assert!(row.contains("Loading plugins…"), "pill row: {row}");
        assert!(frame.text_grid()[9].trim().is_empty(), "bottom=1 leaves the last row");
    }
}

//! The app-shell state overlays (SURF-2, auditB #2):
//!
//! * **Reconnecting** (`component/reconnecting.tsx`): the full-screen
//!   scrim `RGBA(0,0,0,150)` with a centered 48-wide raised box —
//!   "Connection lost…" + "Reconnecting to the server automatically."
//!   Shows 1 s after a RETRY transition, 5 s after the initial one
//!   (app.tsx:1294-1315). D2.2 overlays lane: the scrim dims through
//!   a seam stamp + the box rides the tree as the family's LAST
//!   child (ref zIndex 10000).
//! * **`StartupLoading`** (`component/startup-loading.tsx`): the
//!   bottom-centered pill — "Loading plugins…" 500 ms after boot goes
//!   un-ready, "Finishing startup…" once ready with a 3 s hold, at
//!   `bottom=1` full-width centered, raised background, padding 1.
//!   D2.2 overlays lane: a family wrapper at skeleton index 8
//!   (today's frame-tail z, above every dialog pixel; ref zIndex
//!   5000).
//! * **The paste_frame overlays** (renderer console / which-key /
//!   toast): the overlays family's first three seats — mounted here
//!   from the live store, before `render_app`'s compositor seam.

use crate::components::footer::theme::FooterTheme;
use crate::render::Frame;
use crate::store::Store;
use crate::theme::ThemeStyles;

/// Mount the tree-riding state overlays (the pill + the migration
/// widget) — called from `render_app` BEFORE the chrome composite
/// seam. D2.2 overlays lane: both seat in the overlays family
/// (skeleton indices 8/9, above every dialog pixel — today's
/// frame-tail z, the ref's zIndex 5000-over-3000).
pub fn mount_state_overlays(store: &Store, styles: &ThemeStyles, width: usize, height: usize) {
    let now = store.clock();
    if let Some(text) = store.startup_loading_text(now) {
        super::overlays_solid::mount_startup_loading_solid(styles, text, now, width, height);
    }
    if let Some(progress) = store.migration_progress() {
        super::overlays_solid::mount_migration_solid(styles, &progress, now, width, height);
    }
}

/// Mount the overlays family's live surfaces (console / which-key /
/// toast / reconnecting) — `render_app` calls this AFTER the dialog
/// family's mount and BEFORE the compositor seam, so the family's
/// wrappers seat in the same walk/flush batch (skeleton indices 5+,
/// above the dialogs family's 4 — today's paste order frozen:
/// console < which-key < permission@3 < toast < pill < migration <
/// scrim). NOT mounting a surface this frame unmounts its wrapper at
/// the seam (the old painters' visibility branches).
pub fn mount_overlays_family(
    store: &Store,
    styles: &ThemeStyles,
    width: usize,
    height: usize,
    which_key_dock_top: Option<usize>,
) {
    // TUI-29 — the renderer console (`renderer.console`'s strip at
    // the top of the screen, over the app).
    if store.console_open() {
        super::console_solid::mount_console_overlay(
            &store.console_lines(),
            styles,
            u16::try_from(width).unwrap_or(u16::MAX),
        );
    }
    // The which-key panel: overlay seats at the screen bottom, dock
    // at the route's reduced height (`which_key_dock_top`), above
    // every route surface.
    if let Some(view) = store.which_key() {
        super::which_key::mount_which_key(&view, styles, width, height, which_key_dock_top);
    }
    // The toast layer (`ui/toast.tsx`): the queue's current toast
    // composites over whatever rendered underneath — the interior
    // rides the tree; the rails are the post-seam paint below (they
    // keep the underlying cell background).
    if let Some(toast) = store.toasts().current().cloned() {
        let pending = store.toasts().pending();
        let footer_theme = FooterTheme::from_resolved(&store.theme().resolved.tokens);
        super::toast_solid::mount_toast_tree(
            &toast,
            pending,
            width,
            &footer_theme,
            &store.theme().resolved.tokens,
        );
    }
    // The reconnecting scrim + box (the family's last child).
    let now = store.clock();
    if store.reconnecting_overlay_visible(now) {
        super::overlays_solid::mount_reconnecting_solid(
            styles,
            now.saturating_sub(store.connection_since()),
            width,
            height,
        );
    }
}

/// The overlays family's POST-SEAM paints (the dialogs lane's
/// two-phase paradigm: frame-reading strokes that must land over the
/// composited pixels): the toast's two rail columns (they keep the
/// UNDERLYING cell background — impossible for a tree cell) and the
/// reconnecting box's narrow-frame copy tail (ch/fg only over the
/// dimmed base). `render_app` calls this AFTER the dialog layer's
/// own post-seam paint, exactly the old `App::frame` tail order.
pub fn paint_overlays_post_seam(
    frame: &mut Frame,
    store: &Store,
    styles: &ThemeStyles,
    width: usize,
) {
    if let Some(toast) = store.toasts().current().cloned() {
        let pending = store.toasts().pending();
        let footer_theme = FooterTheme::from_resolved(&store.theme().resolved.tokens);
        super::toast_solid::paint_toast_rails(
            frame,
            &toast,
            pending,
            &footer_theme,
            &store.theme().resolved.tokens,
            None,
        );
    }
    let now = store.clock();
    if store.reconnecting_overlay_visible(now) {
        super::overlays_solid::paint_reconnecting_tail(frame, styles, width);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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
            clusters: None,
        }
    }

    /// The scrim dims every background by 150/255 and paints the
    /// centered box copy (reconnecting.tsx:8-39).
    #[test]
    fn reconnecting_scrim_and_box() {
        let styles = crate::theme::ThemeState::v2_dark().styles();
        let mut frame = frame_with(60, 12);
        super::super::overlays_solid::reconnecting_over_frame(&mut frame, &styles, 0);
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

        // The painter side: `mount_state_overlays` leaves the pill row
        // empty (the clock advances past 500 ms so an armed pill would
        // show if the skip were ignored).
        let styles = crate::theme::ThemeState::v2_dark().styles();
        let mut frame = frame_with(40, 10);
        store.set_connection(crate::data::ConnectionState::Connected);
        crate::components::overlays::mount_state_overlays(&store, &styles, 40, 10);
        crate::components::chrome_solid::composite_chrome_tree(&mut frame);
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
        crate::components::overlays_solid::mount_startup_loading_solid(
            &styles,
            "Loading plugins…",
            0,
            40,
            10,
        );
        crate::components::chrome_solid::composite_chrome_tree(&mut frame);
        let row = frame.text_grid()[8].clone();
        assert!(row.contains("Loading plugins…"), "pill row: {row}");
        assert!(frame.text_grid()[9].trim().is_empty(), "bottom=1 leaves the last row");
    }

    /// D2.2 overlays lane — the family's seating order is today's
    /// paste order: console (5) < which-key (6) < toast (7). The
    /// toast's opaque interior covers the console's blanked rows
    /// where they overlap (the toast mounts LAST), and the console's
    /// blank underlay erases the base outside the toast panel.
    #[test]
    fn overlays_family_order_is_the_old_paste_order() {
        let styles = crate::theme::ThemeState::v2_dark().styles();
        let store = Store::new(Runtime::new());
        store.set_connection(crate::data::ConnectionState::Connected);
        // The console strip (rows 0..12, blank-underlaid).
        crate::components::console_solid::mount_console_overlay(
            &[
                "console fixture line".to_owned(),
                "second line".to_owned(),
            ],
            &styles,
            80,
        );
        // The which-key panel (bottom rows).
        let view = crate::components::which_key::WhichKeyView {
            prefix: None,
            entries: vec![crate::components::which_key::WhichKeyEntry {
                key: "n".to_owned(),
                label: "New session".to_owned(),
                group: "Session".to_owned(),
                continues: false,
            }],
            layout: crate::components::which_key::WhichKeyLayout::Overlay,
            pending_preview: true,
            pinned: true,
            offset: 0,
            active_group: None,
            toggle_hint: "ctrl+alt+k".to_owned(),
            layout_hint: "ctrl+alt+shift+k".to_owned(),
        };
        crate::components::which_key::mount_which_key(&view, &styles, 80, 24, None);
        // The toast (top-right rows 1..5, mounted last).
        let toast = crate::components::toast::ToastOptions::info("Order fixture");
        let footer = crate::components::footer::theme::FooterTheme::from_resolved(
            &crate::theme::ThemeState::v2_dark().resolved.tokens,
        );
        let tokens = crate::theme::ThemeState::v2_dark().resolved.tokens;
        crate::components::toast_solid::mount_toast_tree(&toast, 0, 80, &footer, &tokens);

        let mut frame = frame_with(80, 24);
        crate::components::chrome_solid::composite_chrome_tree(&mut frame);
        // The post-seam half (render_app's tail): the rails.
        crate::components::toast_solid::paint_toast_rails(
            &mut frame,
            &toast,
            0,
            &footer,
            &tokens,
            None,
        );
        let grid = frame.text_grid();
        // The console title rides the top border (row 0, the family's
        // lowest seat).
        assert!(grid[0].contains(" console "), "console title: {:?}", grid[0]);
        // The which-key panel seals the bottom (the full port paints
        // a raised background fill, not a border).
        assert!(grid[23].contains("toggle ctrl+alt+k"), "which-key footer: {:?}", grid[23]);
        // The toast (mounted LAST — the family's highest seat below
        // the state overlays) lands over the console's blanked rows:
        // the message at the panel's second row, the rails at both
        // panel rows keeping the console blank's DEFAULT background
        // (nothing hand-painted under them).
        assert!(grid[2].contains("Order fixture"), "toast message: {:?}", grid[2]);
        let rail_row: String = grid[1].chars().skip(59).take(1).collect();
        assert!(rail_row.starts_with('┃'), "toast rail over the console blank: {rail_row:?}");
        assert!(grid[2].contains('┃'), "toast second rail row: {:?}", grid[2]);
    }
}

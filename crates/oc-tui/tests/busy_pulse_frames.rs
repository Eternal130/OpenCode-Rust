//! W6 症状⑨ (G-128 verification) — the busy session tab's breathing
//! animation as a frame sequence: the strip's busy face must MOVE on
//! the store clock, exactly like the ref's `TabPulseRenderable`
//! (tab-pulse.tsx):
//!
//! * the clockwise dot spinner cycles its glyph frames
//!   (`<spinner>` over the clock — spinner.tsx), and
//! * the busy sweep's background band crosses the tab left-to-right
//!   each 2.8 s cycle with a second pass half a cycle behind
//!   (`PulseState.fronts`, tab-pulse.tsx:404-417 — the G-128
//!   "breathing" the beta.15/16 users reported missing).
//!
//! The W6 audit found the port's sweep ALREADY animated in-tree
//! (W5's lane TAIL landed the clock-driven envelopes — the stale
//! "static sweep" premise in G-128's勘注 predates it); this sequence
//! test pins BOTH moving faces so a regression to a static strip
//! goes red. The frames sample the WALL-time clock the live run
//! loop advances every ~100 ms (app.rs's tick — the port's frame
//! pacer), so the sequence here is the sequence a real terminal
//! sees.

use oc_tui::app::App;
use oc_tui::store::Route;

/// One strip sample: the row text (spinner glyph) + the first 20
/// cells' background colors (the sweep band).
struct Sample {
    row: String,
    bgs: Vec<(u8, u8, u8)>,
}

fn sample(app: &App) -> Sample {
    let frame = app.frame();
    let row = frame.text_grid()[0].clone();
    let bgs = frame.cells[0]
        .iter()
        .take(20)
        .map(|cell| match cell.bg {
            oc_tui::render::CellColor::Rgb(r, g, b) => (r, g, b),
            _ => (0, 0, 0),
        })
        .collect();
    Sample { row, bgs }
}

#[test]
fn busy_tab_pulse_frame_sequence_moves() {
    let app = App::new((120, 30));
    app.store.open_tab("s1", Some("Alpha".to_owned()));
    app.store.navigate(Route::session("s1".to_owned()));
    app.store.hydrate_running_snapshot(["s1".to_owned()]);
    assert!(app.store.session_busy("s1"), "the session must read busy");

    // Sample the strip every 350 ms of wall-like clock across one
    // full sweep cycle (2.8 s) + margins.
    let mut samples = Vec::new();
    for step in 0..10u64 {
        app.store.advance_clock(10_000 + step * 350);
        samples.push(sample(&app));
    }

    // (a) the spinner glyph cycles: the row text changes across
    // samples (the dot frames ⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏).
    let spinner_frames: std::collections::HashSet<&str> =
        samples.iter().map(|s| s.row.trim_start()).collect();
    assert!(
        spinner_frames.len() >= 4,
        "the busy spinner must cycle its frames ({} distinct rows)",
        spinner_frames.len()
    );

    // (b) the sweep band MOVES: the background profile of the tab's
    // cells differs between samples 350 ms apart (the front crossed).
    let mut moving_pairs = 0;
    for pair in samples.windows(2) {
        if pair[0].bgs != pair[1].bgs {
            moving_pairs += 1;
        }
    }
    assert!(
        moving_pairs >= 6,
        "the busy sweep band must advance frame-to-frame ({moving_pairs}/9 pairs moved)"
    );

    // (c) the band is bounded: no cell leaves the resting-background
    // neighborhood by more than the sweep's 0.14 amplitude over the
    // ~30-level base (43 ≈ 30 + 0.14·(255−30) — the RUN_SWEEP_OPACITY
    // ceiling; a runaway pulse would blow past it).
    for sample in &samples {
        for (r, g, b) in &sample.bgs {
            let max_channel = (*r).max(*g).max(*b);
            assert!(
                max_channel <= 60,
                "the sweep amplitude must stay at the 0.14 band (saw {max_channel})"
            );
        }
    }

    // (d) idle contrast: a fresh app with the same tab NOT running
    // renders a static resting strip — the busy motion belongs to the
    // busy state alone (the running set only clears through lifecycle
    // events, so the idle face is pinned on a fresh app).
    let idle_app = App::new((120, 30));
    idle_app.store.open_tab("s1", Some("Alpha".to_owned()));
    idle_app.store.navigate(Route::session("s1".to_owned()));
    assert!(!idle_app.store.session_busy("s1"));
    idle_app.store.advance_clock(20_000);
    let idle_a = sample(&idle_app);
    idle_app.store.advance_clock(21_000);
    let idle_b = sample(&idle_app);
    assert_eq!(idle_a.bgs, idle_b.bgs, "the idle tab's background is static");
    assert!(!idle_b.row.contains('⠋'), "no spinner glyph once idle: {}", idle_b.row);
}

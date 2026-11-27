//! slowE — the harness RIDER target (ONE dedicated test binary).
//!
//! The rider tests used to live as `#[cfg(test)]` mods inside
//! `realchain_harness.rs`, which every realchain family target
//! includes via `#[path]` — each of the 16 target binaries compiled
//! AND RAN them (the five fixed-sleep quiescence riders alone are
//! ~26.8 s of wall: ~134 s of merge-gate duplication across the five
//! battery targets, ~429 s across the full family set). Deduped
//! here: the riders gate the same behaviors exactly once per gate.
//! Config consistency: the merge profile's default-filter excludes
//! only the realchain_v* anchor binaries (this target is NOT
//! excluded — it rides the gate's "+ harness" line); the CI nextest
//! job's filter does not exclude it either; no guard references the
//! rider names (checked 2026-11-28).
//!
//! The fixed-sleep riders stay fixed-sleep ON PURPOSE: each window
//! exercises a specific settle-arm behavior (the blink trap, the
//! spinner tolerance, the late completion, the paced-stream cadence)
//! — the sleeps are the stimulus, not padding.

#![allow(clippy::doc_markdown, clippy::similar_names)]

#[path = "realchain_harness.rs"]
mod realchain_harness;

#[cfg(test)]
mod slowe_rider_tests {
    use crate::realchain_harness::{
        capture_settled, capture_settled_pair, quiescence_names, TuiProcess,
    };
    use std::time::{Duration, Instant};

    /// slowE — the caret-blink quiescence trap, as a rider: a stream
    /// that NEVER goes byte-quiet (cursor-chatter escape bytes every
    /// 300 ms — the shape of the run loop's 100 ms render tick while
    /// the composer caret is live) over a TEXT grid that never
    /// changes. The retired stream-quiet gate burned its full 10 s cap
    /// on exactly this shape (slowB's ["settled","capped"] evidence);
    /// the grid-stability arm must certify it as settled, on the
    /// stable grid, well inside the cap.
    #[test]
    fn blink_noise_stream_settles_on_stable_grid() {
        let script = "printf 'stable-grid\\r\\n'; while true; do printf '\\033[?25l'; sleep 0.3; done";
        let mut command = std::process::Command::new("sh");
        command.arg("-c").arg(script);
        let mut proc = TuiProcess::spawn_command(command, 40, 10);
        let started = Instant::now();
        let grid = capture_settled(&proc);
        let elapsed = started.elapsed();
        let verdict = quiescence_names();
        assert_eq!(
            verdict,
            ["settled".to_owned()],
            "the chatter-over-stable-grid stream must certify settled, not cap"
        );
        assert!(
            grid.contains("stable-grid"),
            "the certified grid is the stable one: {grid:?}"
        );
        assert!(
            elapsed < Duration::from_secs(6),
            "the settle must not burn the 10 s cap (took {elapsed:?})"
        );
        proc.shutdown(Duration::from_secs(2));
    }

    /// slowE — slowA's cell classification, as a rider: a cell cycling
    /// a four-glyph alphabet at 80 ms (the spinner cadence) over
    /// otherwise-stable content is ANIMATION, not new content — its
    /// flips (fast, among seen values) must not re-arm the stability
    /// window, so the capture settles on the stable content grid
    /// instead of capping the way a naive identical-samples check
    /// would.
    #[test]
    fn spinner_cell_tolerance_settles_on_stable_content() {
        let script = "printf 'content-row\\r\\n'; while true; do \
                      for f in a b c d; do printf '\\033[3;1H%s' \"$f\"; sleep 0.08; done; done";
        let mut command = std::process::Command::new("sh");
        command.arg("-c").arg(script);
        let mut proc = TuiProcess::spawn_command(command, 40, 10);
        let started = Instant::now();
        let grid = capture_settled(&proc);
        let elapsed = started.elapsed();
        let verdict = quiescence_names();
        assert_eq!(
            verdict,
            ["settled".to_owned()],
            "the cycling spinner cell over stable content must settle, not cap"
        );
        assert!(
            grid.contains("content-row"),
            "the certified grid carries the stable content: {grid:?}"
        );
        assert!(
            elapsed < Duration::from_secs(6),
            "the settle must not burn the 10 s cap (took {elapsed:?})"
        );
        proc.shutdown(Duration::from_secs(2));
    }

    /// slowE — the pair-concurrent capture: both legs run as scope
    /// legs and the grids come back in (ref, rust) order (pinned by
    /// the distinct echo tails), with BOTH legs' quiescence remarks
    /// re-emitted on the CALLING thread (a dropped remark — the
    /// thread-local blocker slowB found — would drain fewer than two
    /// entries; a leg-thread mark could never reach this thread).
    #[test]
    fn pair_capture_remarks_replay_on_calling_thread() {
        let mut ref_leg = TuiProcess::spawn_command(std::process::Command::new("cat"), 40, 10);
        ref_leg.write(b"ref-tail");
        let mut rust_leg = TuiProcess::spawn_command(std::process::Command::new("cat"), 40, 10);
        rust_leg.write(b"rust-tail");
        let (ref_grid, rust_grid) = capture_settled_pair(&ref_leg, &rust_leg);
        let verdict = quiescence_names();
        assert_eq!(verdict, ["settled".to_owned(), "settled".to_owned()],
            "both legs' remarks must land on the calling thread, in leg order");
        assert!(ref_grid.contains("ref-tail"), "the first tuple slot is the ref leg: {ref_grid:?}");
        assert!(rust_grid.contains("rust-tail"), "the second tuple slot is the rust leg: {rust_grid:?}");
        ref_leg.shutdown(Duration::from_secs(2));
        rust_leg.shutdown(Duration::from_secs(2));
    }

    /// slowE — the shutdown chord RE-SEND: a child that only exits on
    /// its SECOND chord observation (the shape of a modal error toast
    /// swallowing the first one — app.rs `error_toast_live`). The
    /// re-send must land inside the grace so the teardown exits
    /// cleanly (status success — not a kill) well before the grace
    /// burns out.
    ///
    /// The child arms RAW mode FIRST and announces readiness on the
    /// stream (the chord's ctrl+c would otherwise arrive as SIGINT
    /// through the still-canonical line discipline — the real TUI
    /// arms raw mode itself at boot; the readiness needle pins the
    /// arm before the first chord is sent).
    #[test]
    fn shutdown_resends_the_swallowed_exit_chord() {
        // NOTE: explicit \n escapes on ONE source line — backslash
        // line-continuations strip the next line's leading whitespace
        // and would de-indent the python body.
        let script = "python3 -c 'import sys\nimport tty\ntty.setraw(0)\nsys.stdout.buffer.write(b\"CHORD-READY\\n\")\nsys.stdout.buffer.flush()\ndata = b\"\"\nwhile True:\n    chunk = sys.stdin.buffer.read(1)\n    if not chunk: break\n    data += chunk\n    if data.count(b\"\\x15\\x03\\x03\") >= 2: sys.exit(0)'";
        let mut command = std::process::Command::new("sh");
        command.arg("-c").arg(script);
        let mut proc = TuiProcess::spawn_command(command, 40, 10);
        assert!(
            proc.wait_for(b"CHORD-READY", 0, Duration::from_secs(10)),
            "the chord counter never armed its raw mode"
        );
        let started = Instant::now();
        proc.shutdown(Duration::from_secs(10));
        let elapsed = started.elapsed();
        let status = proc.child.wait().expect("wait after shutdown");
        assert!(
            status.success(),
            "the child must exit through the chord, not a kill (status {status:?})"
        );
        assert!(
            elapsed < Duration::from_secs(6),
            "the re-sent chord must land well inside the grace (took {elapsed:?})"
        );
    }
}

#[cfg(test)]
mod rider_payload_root_tests {
    use crate::realchain_harness::*;
    use std::path::Path;

    /// Rider (Lane D pitfall) — the git-ignored payload registry lives
    /// only in the main checkout: in a lane worktree the local path is
    /// absent and the root MUST fall back to the main checkout's copy
    /// (a silent skip once fake-greened a whole omo lane).
    #[test]
    fn payload_root_falls_back_to_main_checkout() {
        let local = worktree_dir("tools/diff/realchain/plugin-payload");
        let root = plugin_payload_root();
        if local.is_dir() {
            // Main-checkout shape: the local copy wins outright.
            assert_eq!(root, local);
        } else if !root.is_dir() {
            // No payload registry anywhere on this machine (CI does not
            // provision the git-ignored registry; a checkout that is its
            // own "main checkout" resolves the fallback to `local`).
            // Availability gap, not a contract breach — skip with the
            // reason (mirrors `ref_lane_skip_reason`); on a dev box the
            // registry exists and the fallback assertions below run.
            let ci_shape = root == local;
            eprintln!(
                "PAYLOAD-ROOT SKIP: no plugin payload registry available \
                 (local={}, fallback root={}; {})",
                local.display(),
                root.display(),
                if ci_shape {
                    "CI shape: checkout is its own main checkout, fallback == local"
                } else {
                    "worktree shape: fallback target missing"
                }
            );
            let log = worktree_dir(".test-logs");
            let _ = std::fs::create_dir_all(&log);
            let _ = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(log.join("skips.log"))
                .and_then(|mut file| {
                    use std::io::Write;
                    writeln!(file, "[payload-root] no plugin payload registry available")
                });
        } else {
            // Dev-box worktree shape: the fallback MUST be the main
            // checkout's copy, never the (absent) worktree-local path.
            assert_ne!(root, local);
        }
    }

    /// Skip semantics survive without a payload supply: an empty root
    /// yields a reason naming the missing tree (never a red).
    #[test]
    fn payload_skip_reason_names_missing_root() {
        let tmp = std::env::temp_dir().join(format!("t9-payload-empty-{}", std::process::id()));
        std::fs::create_dir_all(&tmp).expect("tmp payload root");
        let reason = plugin_lane_skip_reason_at(&tmp)
            .expect("empty payload root must produce a skip reason");
        assert!(reason.contains("oh-my-opencode-slim"), "reason names the spec dir: {reason}");
        let _ = std::fs::remove_dir_all(&tmp);
    }
}

#[cfg(test)]
mod rider_quiescence_tests {
    use crate::realchain_harness::*;
    use std::time::Duration;

    /// Rider (s3-palette settle race): the quiescence gate returns
    /// settled=true once the stream stops growing (a `cat` echo tail)
    /// and settled=false inside the cap while the stream never stops
    /// (a `yes` flood) — the annotated export-anyway path.
    #[test]
    fn quiescent_when_stream_stops_capped_under_flood() {
        let mut quiet = TuiProcess::spawn_command(std::process::Command::new("cat"), 40, 10);
        quiet.write(b"quiesce-probe");
        assert!(
            quiet.wait_quiescent(Duration::from_millis(250), Duration::from_secs(5)),
            "an echo tail never went quiescent"
        );
        quiet.shutdown(Duration::from_secs(2));

        let mut flood = TuiProcess::spawn_command(std::process::Command::new("yes"), 40, 10);
        std::thread::sleep(Duration::from_millis(300)); // let the flood fill the stream
        assert!(
            !flood.wait_quiescent(Duration::from_millis(250), Duration::from_millis(1200)),
            "a forever-growing stream reported quiescent"
        );
        flood.shutdown(Duration::from_secs(2));
    }

    /// W38 third arm — the settle needs THREE consecutive identical
    /// quiet-window grids. A stream that shows frame A for exactly
    /// TWO quiet windows then changes to B: the retired N=2 arm froze
    /// on the stale A grid; the settle must wait past the change and
    /// export B, annotated settled. (Fourth-arm note: windows are now
    /// 500 ms + a 1 s re-verification, so the same script settles
    /// through the wider gate.)
    #[test]
    fn grid_stability_requires_three_matching_quiet_windows() {
        let script = "printf 'frameA\r\n'; sleep 0.58; printf 'frameA2\r\n'; sleep 9";
        let mut command = std::process::Command::new("sh");
        command.arg("-c").arg(script);
        let mut proc = TuiProcess::spawn_command(command, 40, 10);
        let grid = capture_settled(&proc);
        let verdict = quiescence_names();
        assert_eq!(
            verdict,
            ["settled".to_owned()],
            "the two-window-then-change stream settled on the FINAL grid"
        );
        assert!(
            grid.contains("frameA2"),
            "the exported grid is the final frame, not the two-window stale one: {grid:?}"
        );
        proc.shutdown(Duration::from_secs(2));
    }

    /// W38 third arm — a grid that keeps CHANGING across quiet
    /// windows (a growing tick line each 0.4 s burst, the mid-state
    /// cadence shape) never produces three identical consecutive
    /// windows: capped, exported anyway, annotated. (Fourth-arm note:
    /// the cap is now 10 s — a forever-changing grid still caps and
    /// exports, just with the longer conservative budget.)
    #[test]
    fn changing_grid_caps_inside_three_second_window() {
        let script = "i=0; while true; do i=$((i+1)); printf 'tick %s\r\n' \"$i\"; sleep 0.4; done";
        let mut command = std::process::Command::new("sh");
        command.arg("-c").arg(script);
        let mut proc = TuiProcess::spawn_command(command, 40, 10);
        let _grid = capture_settled(&proc);
        let verdict = quiescence_names();
        assert_eq!(verdict, ["capped".to_owned()], "a forever-changing grid must cap, not settle");
        proc.shutdown(Duration::from_secs(2));
    }

    /// W38 fourth arm, mechanism A — a settlement chain SLOWER than
    /// the retired 3 s cap (one changed burst each 0.9 s for ~4 s,
    /// then the final frame + quiet): the 10 s / 500 ms gate settles
    /// on the FINAL frame instead of capping through the chain and
    /// exporting a mid-burst grid.
    #[test]
    fn slow_settlement_chain_settles_in_the_fourth_arm_gate() {
        let script = "printf 'chain1\r\n'; sleep 0.9; printf 'chain2\r\n'; sleep 0.9; \
                      printf 'chain3\r\n'; sleep 0.9; printf 'chain4\r\n'; sleep 0.9; \
                      printf 'final-frame\r\n'; sleep 9";
        let mut command = std::process::Command::new("sh");
        command.arg("-c").arg(script);
        let mut proc = TuiProcess::spawn_command(command, 40, 10);
        let grid = capture_settled(&proc);
        let verdict = quiescence_names();
        assert_eq!(
            verdict,
            ["settled".to_owned()],
            "the slow chain settles inside the 10 s gate (the 3 s cap would have exported mid-chain)"
        );
        assert!(
            grid.contains("final-frame"),
            "the exported grid is the settled final frame, not a mid-chain burst: {grid:?}"
        );
        proc.shutdown(Duration::from_secs(2));
    }

    /// W38 fourth arm, mechanism B — a grid that stays IDENTICAL
    /// across three quiet windows (the candidate settle) and THEN
    /// completes (frameX ~1.8 s → frameY): the 1 s post-settle
    /// re-verification catches the late completion, the history
    /// resets, and the export settles on the FINAL grid.
    #[test]
    fn post_settle_recheck_rewaits_on_late_completion() {
        let script = "printf 'frameX\r\n'; sleep 1.8; printf 'frameY\r\n'; sleep 9";
        let mut command = std::process::Command::new("sh");
        command.arg("-c").arg(script);
        let mut proc = TuiProcess::spawn_command(command, 40, 10);
        let grid = capture_settled(&proc);
        let verdict = quiescence_names();
        assert_eq!(
            verdict,
            ["settled".to_owned()],
            "the late completion re-settles instead of exporting the stale candidate"
        );
        assert!(
            grid.lines().any(|line| line.trim() == "frameY"),
            "the exported grid carries the completed frame: {grid:?}"
        );
        // The completion is the LAST content row (the printf rig
        // appends — like a transcript, the settled export ends on it).
        let last_content = grid.lines().rev().find(|line| !line.trim().is_empty());
        assert_eq!(
            last_content.map(str::trim),
            Some("frameY"),
            "the settled grid ends on the completed frame: {grid:?}"
        );
        proc.shutdown(Duration::from_secs(2));
    }
}

#[cfg(test)]
mod g461_ref_port_tests {
    use crate::realchain_harness::*;
    use std::path::Path;

    /// Mirror of the deterministic hashed base in `Sandbox::ref_service_port`.
    fn hashed_base(root: &Path) -> u16 {
        use std::hash::{Hash, Hasher};
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        root.hash(&mut hasher);
        50_000 + (hasher.finish() % 10_000) as u16
    }

    /// G-461 — an incumbent listener on a sandbox's hashed base port
    /// (the shape of a stray user-run `ref serve --service` DETACH
    /// child) must never be handed out: the probe walks past it, stays
    /// in the 50000-60000 band, and the pick is pinned so a re-call
    /// cannot silently re-point the sandbox at its own live daemon.
    ///
    /// Adaptive incumbent: we try to bind the hashed base ourselves.
    /// If the bind succeeds we own a dummy incumbent; if it fails with
    /// AddrInUse a real listener (e.g. a parallel test's live ref
    /// service in the same band) IS the incumbent — the walk must
    /// still clear it. No "band free by construction" assumption.
    #[test]
    fn ref_port_walks_past_incumbent_listener() {
        let sb = sandbox("g461incumbent", 0);
        let other = sandbox("g461other", 0);
        let base = hashed_base(&sb.root);
        // Distinct sandbox roots pin distinct bases (no cross-sandbox
        // fights inside one test process).
        assert_ne!(base, hashed_base(&other.root));
        let incumbent = match std::net::TcpListener::bind(("127.0.0.1", base)) {
            Ok(l) => Some(l),
            Err(e) if e.kind() == std::io::ErrorKind::AddrInUse => {
                // A real listener already owns the base port — that
                // occupant itself is the incumbent to walk past.
                None
            }
            Err(e) => panic!("unexpected bind error on base port {base}: {e}"),
        };
        let port = sb.ref_service_port();
        assert!((50_000..60_000).contains(&port), "port {port} outside the pinned band");
        assert_ne!(port, base, "must walk past the incumbent listener");
        drop(incumbent);
        // The pin is cached: even after the incumbent is gone, the
        // sandbox keeps its port (a live daemon owns it now).
        assert_eq!(sb.ref_service_port(), port);
        let _ = std::fs::remove_dir_all(&sb.root);
        let _ = std::fs::remove_dir_all(&other.root);
    }
}

#[cfg(test)]
mod w4_normalizer_tests {
    use crate::realchain_harness::*;

    #[test]
    fn seconds_durations_mask_but_words_survive() {
        assert_eq!(
            replace_timings("Build · claude-opus-4-7 · 3.6s · 5.1 tok/s"),
            "Build · claude-opus-4-7 · __SECS__ · __TOKS__"
        );
        assert_eq!(replace_timings("12ms later"), "__MS__ later");
        // `s` inside a word is not a duration.
        assert_eq!(replace_timings("38 tokens used"), "38 tokens used");
        assert_eq!(replace_timings("4 steps"), "4 steps");
    }

    #[test]
    fn id_shapes_use_the_schema_wire_prefix_list() {
        assert_eq!(replace_id_shapes("tab ses_0001abcdEFGhij"), "tab __ID__");
        assert_eq!(replace_id_shapes("row msg_1e64066090019u8zd"), "row __ID__");
        // `pty` is NOT a schema wire prefix (local attachment shape).
        assert_eq!(replace_id_shapes("attach pty_0001abcdEFGhij"), "attach pty_0001abcdEFGhij");
        // Short words that share a prefix stay intact.
        assert_eq!(replace_id_shapes("session message"), "session message");
    }

    /// va21g — the wordmark's inline v-prefixed version token (the
    /// mini splash row: `▪ oc mini vlocal · …` vs `v2.0.24 · …`).
    #[test]
    fn version_pill_collapses_the_inline_vtoken() {
        assert_eq!(
            replace_version_pill("▪ oc mini vlocal · __ROOT__/project"),
            "▪ oc mini __VERSION__ · __ROOT__/project"
        );
        assert_eq!(
            replace_version_pill("▪ oc mini v2.0.24 · __ROOT__/project"),
            "▪ oc mini __VERSION__ · __ROOT__/project"
        );
        // EOL-terminated token (the narrow form: the detail truncates).
        assert_eq!(replace_version_pill("▪ oc mini vlocal"), "▪ oc mini __VERSION__");
        // NOT a version: plain words with a v stay intact.
        assert_eq!(replace_version_pill("very good text"), "very good text");
        assert_eq!(replace_version_pill("value vlocal-ish and more"), "value vlocal-ish and more");
        // A mid-word v (no leading space) stays intact.
        assert_eq!(replace_version_pill("savedvlocal row"), "savedvlocal row");
    }
}


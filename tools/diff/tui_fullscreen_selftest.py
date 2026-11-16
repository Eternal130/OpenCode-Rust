#!/usr/bin/env python3
"""Selftest for channel-8 normalization/compare/gate (task 5).

Runs standalone:  python3 -m unittest tools.diff.tui_fullscreen_selftest -v
          or     python3 tools/diff/tui_fullscreen_selftest.py

Five synthetic `tui-diff-grid/1` cases pinning the compare contract:

  1. identical grids                              -> exact
  2. single-char diff at (3,5)                    -> diverge, first_diff pinpoints row/col/field
  3. RGB-only diff, every channel <= 8            -> structural (still a divergence, G-368 tolerance)
  4. one side attrs has "unknown:16", other not   -> diverge (never silent)
  5. one side fg=[-1,-1,-1] sentinel vs real RGB  -> diverge

plus verdict() gate semantics: exit 0 (all adjudicated), exit 2 (any
unadjudicated divergence) — gate scope = committed corpus only;
rec-* recorded-local unadjudicated divergences are advisory (exit 0),
while rec-* infra failures still hard-exit 1 in load_side.
"""
from __future__ import annotations

import os
import sys
import unittest

sys.path.insert(0, os.path.join(os.path.dirname(__file__), "..", ".."))

from tools.diff.tui_fullscreen import (  # noqa: E402
    CAUSES, adjudicate, compare, normalize, verdict,
)


def mkcell(ch: str = " ", attrs: list | None = None,
           fg: list | None = None, bg: list | None = None) -> dict:
    return {
        "ch": ch,
        "attrs": attrs if attrs is not None else [],
        "fg": fg if fg is not None else [255, 255, 255],
        "bg": bg if bg is not None else [0, 0, 0],
    }


def mkgrid(cells: list[list[dict]], width: int = 8, height: int = 4,
           grid_id: str = "synthetic") -> dict:
    return {
        "schema": "tui-diff-grid/1",
        "id": grid_id,
        "width": width,
        "height": height,
        "cells": cells,
        "stub_gaps": [],
        "request_census": {},
    }


def uniform(ch: str = " ") -> list[list[dict]]:
    return [[mkcell(ch) for _ in range(8)] for _ in range(4)]


class CompareTest(unittest.TestCase):
    def test_exact(self):
        res = compare(normalize(mkgrid(uniform())), normalize(mkgrid(uniform())))
        self.assertEqual(res["verdict"], "exact")
        self.assertIsNone(res["first_diff"])
        self.assertEqual(res["cells_diff_count"], 0)

    def test_single_char_diff_pinpoints_first_diff(self):
        ref_cells, rust_cells = uniform(), uniform()
        ref_cells[3][5] = mkcell("a")
        rust_cells[3][5] = mkcell("b")
        res = compare(normalize(mkgrid(ref_cells)), normalize(mkgrid(rust_cells)))
        self.assertEqual(res["verdict"], "diverge")
        self.assertEqual(res["first_diff"]["row"], 3)
        self.assertEqual(res["first_diff"]["col"], 5)
        self.assertEqual(res["first_diff"]["field"], "ch")
        self.assertEqual(res["first_diff"]["ref"], "a")
        self.assertEqual(res["first_diff"]["rust"], "b")
        self.assertEqual(res["cells_diff_count"], 1)

    def test_rgb_within_tolerance_is_structural(self):
        ref_cells, rust_cells = uniform("x"), uniform("x")
        ref_cells[0][0] = mkcell("x", fg=[100, 100, 100], bg=[10, 10, 10])
        rust_cells[0][0] = mkcell("x", fg=[107, 93, 108], bg=[3, 17, 2])
        res = compare(normalize(mkgrid(ref_cells)), normalize(mkgrid(rust_cells)))
        self.assertEqual(res["verdict"], "structural")
        self.assertEqual(res["cells_diff_count"], 1)
        # structural is still a divergence, not an exemption: verdict() must
        # not treat it as exact; with no CAUSES table it counts toward exit 2
        code = verdict([res])
        self.assertEqual(code, 2)

    def test_rgb_beyond_tolerance_is_diverge(self):
        ref_cells, rust_cells = uniform("x"), uniform("x")
        ref_cells[1][2] = mkcell("x", fg=[100, 100, 100])
        rust_cells[1][2] = mkcell("x", fg=[100, 100, 109])
        res = compare(normalize(mkgrid(ref_cells)), normalize(mkgrid(rust_cells)))
        self.assertEqual(res["verdict"], "diverge")
        self.assertEqual(res["first_diff"]["field"], "fg")

    def test_unknown_attr_diverges_not_silent(self):
        ref_cells, rust_cells = uniform("x"), uniform("x")
        ref_cells[2][2] = mkcell("x", attrs=["unknown:16"])
        rust_cells[2][2] = mkcell("x", attrs=[])
        res = compare(normalize(mkgrid(ref_cells)), normalize(mkgrid(rust_cells)))
        self.assertEqual(res["verdict"], "diverge")
        fd = res["first_diff"]
        self.assertEqual((fd["row"], fd["col"]), (2, 2))
        self.assertIn("unknown:", str(fd["ref"]) + str(fd["rust"]))

    def test_sentinel_fg_diverges(self):
        ref_cells, rust_cells = uniform("x"), uniform("x")
        ref_cells[0][7] = mkcell("x", fg=[-1, -1, -1])
        rust_cells[0][7] = mkcell("x", fg=[200, 200, 200])
        res = compare(normalize(mkgrid(ref_cells)), normalize(mkgrid(rust_cells)))
        self.assertEqual(res["verdict"], "diverge")
        self.assertEqual(res["first_diff"]["field"], "fg")


class VerdictGateTest(unittest.TestCase):
    def test_all_exact_exit_0(self):
        r = compare(normalize(mkgrid(uniform())), normalize(mkgrid(uniform())))
        self.assertEqual(verdict([r]), 0)

    def test_any_diverge_exit_2(self):
        cells_a, cells_b = uniform(), uniform()
        cells_b[0][0] = mkcell("z")
        r1 = compare(normalize(mkgrid(cells_a)), normalize(mkgrid(uniform())))
        r2 = compare(normalize(mkgrid(cells_a)), normalize(mkgrid(cells_b)))
        self.assertEqual(verdict([r1, r2]), 2)

    def test_structural_unadjudicated_exit_2(self):
        # covered in CompareTest too; here explicitly: no CAUSES table yet,
        # structural is NOT an exemption -> gate stays red
        ref_cells, rust_cells = uniform("x"), uniform("x")
        ref_cells[0][0] = mkcell("x", fg=[0, 0, 0])
        rust_cells[0][0] = mkcell("x", fg=[8, 8, 8])
        r = compare(normalize(mkgrid(ref_cells)), normalize(mkgrid(rust_cells)))
        self.assertEqual(r["verdict"], "structural")
        self.assertEqual(verdict([r]), 2)

    def test_recorded_unadjudicated_is_advisory_exit_0(self):
        # Controller ruling 2026-11-15 (C3): gate scope is the COMMITTED
        # corpus — a rec-* (recorded-local) unadjudicated divergence is
        # advisory and must not redden the release gate.
        cells_a, cells_b = uniform(), uniform()
        cells_b[0][0] = mkcell("z")
        committed = compare(normalize(mkgrid(cells_a)), normalize(mkgrid(uniform())))
        recorded = compare(normalize(mkgrid(cells_a)), normalize(mkgrid(cells_b)))
        self.assertEqual(
            verdict([{"id": "home-empty", **committed},
                     {"id": "rec-20261114-205531-bb7f1e74@120x30", **recorded}]),
            0, "rec-* unadjudicated divergence is advisory")

    def test_recorded_infra_failure_is_never_advisory_exit_1(self):
        # The advisory ruling covers ADJUDICATION only: a rec-* infra
        # failure (stub gap / malformed grid) means the replay mechanism
        # itself is broken and stays a hard exit 1 — enforced by
        # load_side before verdict() ever runs, regardless of the id.
        import json
        import tempfile
        from pathlib import Path
        import tools.diff.tui_fullscreen as tf
        with tempfile.TemporaryDirectory() as td:
            out = Path(td)
            # a rec- grid declaring 8x4 but carrying 3 rows -> infra red
            (out / "rec-x1.json").write_text(json.dumps(
                {"schema": "tui-diff-grid/1", "id": "rec-x1", "width": 8, "height": 4,
                 "cells": [[mkcell() for _ in range(8)] for _ in range(3)]}))
            cases = [{"id": "rec-x1", "scenario": "s", "width": 8, "height": 4}]
            with self.assertRaises(SystemExit) as cm:
                tf.load_side(out, cases, "test")
            self.assertEqual(cm.exception.code, 1)


class RecordingTokenTest(unittest.TestCase):
    """C3 review Important#1 — the paste/mouse recording token shapes:
    the compiler passes them through 1:1 (the recorder already put the
    shape-redacted paste literal in place), and the exit-chord trim
    never eats them."""

    def test_paste_and_mouse_tokens_pass_through_verbatim(self):
        from tools.diff.tui_record_compile import key_token, trim_exit_chord
        paste = "paste:xxxx xxxxx\nxxxxx xxxxx"
        mouse = "mouse:down-left:12,0"
        wheel = "mouse:wheel-up:4,9"
        self.assertEqual(key_token(paste), paste)
        self.assertEqual(key_token(mouse), mouse)
        self.assertEqual(key_token(wheel), wheel)
        # the exit-chord trim leaves recorded interaction intact (the
        # trailing mouse token is not a chord prefix — a truncated
        # [esc,esc,ctrl+u,ctrl+c] tail WOULD be trimmed, by design)
        keys = [paste, "esc", mouse, wheel]
        self.assertEqual(trim_exit_chord(keys), keys)
        # key vocabulary still maps as before (regression guard)
        self.assertEqual(key_token("return"), "enter")
        self.assertEqual(key_token("q"), "raw:q")


class BunGateTest(unittest.TestCase):
    """C3 final-review minor B2 — the TUI_DIFF_BUN gate's three
    negative shapes (unset / too old / unrunnable), with the version
    probe injected so no real bun process is spawned."""

    def test_unset_points_at_the_repo_local_default(self):
        import tools.diff.tui_fullscreen as tf
        with self.assertRaises(SystemExit) as cm:
            tf.check_bun_gate(None)
        msg = cm.exception.code
        self.assertIsInstance(msg, str)
        self.assertIn("TUI_DIFF_BUN is unset", msg)
        self.assertIn("tools/bin/bun-1.4.2", msg,
                      "the unset error names the persistent repo-local default")

    def test_older_than_floor_is_rejected(self):
        import tools.diff.tui_fullscreen as tf
        with self.assertRaises(SystemExit) as cm:
            tf.check_bun_gate("/fake/bun", version_fn=lambda _p: (1, 3, 10))
        msg = cm.exception.code
        self.assertIsInstance(msg, str)
        self.assertIn("(1, 3, 10)", msg)
        self.assertIn(">= 1.4.2", msg)

    def test_unrunnable_binary_is_rejected_as_none(self):
        import tools.diff.tui_fullscreen as tf
        # version_fn returns None exactly as _bun_version does for an
        # unrunnable/unparseable executable (OSError / bad exit / junk)
        with self.assertRaises(SystemExit) as cm:
            tf.check_bun_gate("/nonexistent/bun", version_fn=lambda _p: None)
        msg = cm.exception.code
        self.assertIsInstance(msg, str)
        self.assertIn("unrunnable", msg)

    def test_floor_version_passes(self):
        import tools.diff.tui_fullscreen as tf
        tf.check_bun_gate("/fake/bun", version_fn=lambda _p: (1, 4, 2))


class NormalizeTest(unittest.TestCase):
    def test_normalize_preserves_payload_and_strips_time_hook_fields(self):
        g = mkgrid(uniform())
        g2 = normalize(g)
        self.assertEqual(g2["cells"], g["cells"])
        # a driver-sent time field must not survive normalization
        g3 = dict(g, captured_at=12345, driver_time=1.0)
        g4 = normalize(g3)
        self.assertNotIn("captured_at", g4)
        self.assertNotIn("driver_time", g4)

    def test_load_side_get_defaults(self):
        # optional grid keys must default, not KeyError
        import json
        import tools.diff.tui_fullscreen as tf
        import tempfile
        from pathlib import Path
        with tempfile.TemporaryDirectory() as td:
            out = Path(td)
            (out / "c1.json").write_text(json.dumps(
                {"schema": "tui-diff-grid/1", "id": "c1", "width": 8, "height": 4,
                 "cells": [[None] * 8 for _ in range(4)]}))
            cases = [{"id": "c1", "scenario": "s", "width": 8, "height": 4}]
            grids, unavailable = tf.load_side(out, cases, "test")
            self.assertEqual(grids["c1"].get("stub_gaps", []), [])
            self.assertIsNone(grids["c1"].get("error"))
            self.assertEqual(unavailable, [])


class CausesTest(unittest.TestCase):
    """CAUSES matchers (T6): exact-shape hits and misses per cluster."""

    def _rec(self, **kw):
        base = {"id": "synthetic", "verdict": "diverge",
                "first_diff": None, "cells_diff_count": 1}
        base.update(kw)
        return base

    def _label(self, rec):
        return adjudicate(rec)

    def test_g368_structural_hits_quantize_cluster(self):
        ref_cells, rust_cells = uniform("x"), uniform("x")
        ref_cells[0][0] = mkcell("x", fg=[100, 100, 100])
        rust_cells[0][0] = mkcell("x", fg=[104, 96, 108])
        r = compare(normalize(mkgrid(ref_cells)), normalize(mkgrid(rust_cells)))
        self.assertEqual(r["verdict"], "structural")
        self.assertIn("G-368", self._label({"id": "any-scenario", **r}))

    def test_g368_out_of_tolerance_rgb_does_not_hit(self):
        # 255 vs 238 = delta 17 > tolerance 8 -> diverge, must stay
        # unadjudicated (the smoke baseline's shape)
        ref_cells, rust_cells = uniform("x"), uniform("x")
        ref_cells[0][0] = mkcell("x", fg=[255, 255, 255])
        rust_cells[0][0] = mkcell("x", fg=[238, 238, 238])
        r = compare(normalize(mkgrid(ref_cells)), normalize(mkgrid(rust_cells)))
        self.assertEqual(r["verdict"], "diverge")
        self.assertIsNone(self._label({"id": "session-text", **r}))

    def test_g28_plugin_prefix_diverge_hits(self):
        rec = self._rec(id="plugin-boot-basic")
        self.assertIn("G-28", self._label(rec))

    def test_g28_wrong_prefix_or_exact_shape_misses(self):
        self.assertIsNone(self._label(self._rec(id="home-empty")))
        # plugin- prefix but exact verdict is not a divergence at all
        self.assertIsNone(self._label(self._rec(id="plugin-x", verdict="exact")))

    def test_g332_markdown_prefix_ch_diff_hits(self):
        rec = self._rec(id="session-markdown-codeblock",
                        first_diff={"row": 0, "col": 1, "field": "ch",
                                    "ref": "a", "rust": "b"})
        self.assertIn("G-332", self._label(rec))

    def test_g332_fg_diff_or_bare_id_misses(self):
        # same scenario family but fg field (not ch) -> not this cluster
        rec_fg = self._rec(id="session-markdown-codeblock",
                           first_diff={"row": 0, "col": 0, "field": "fg",
                                       "ref": [1, 2, 3], "rust": [4, 5, 6]})
        self.assertIsNone(self._label(rec_fg))
        # the CURRENT corpus id has no trailing segment: prefix
        # "session-markdown-" must not swallow bare "session-markdown"
        rec_bare = self._rec(id="session-markdown",
                             first_diff={"row": 0, "col": 0, "field": "ch",
                                         "ref": "a", "rust": "b"})
        self.assertIsNone(self._label(rec_bare))

    def test_g239_panel_prefix_hits(self):
        rec = self._rec(id="panel-focused")
        self.assertIn("G-239", self._label(rec))

    def test_panel_at_sign_suffix_misses(self):
        # mirrored-tier ids like panel-x@120x30 still hit (prefix rule)
        self.assertIn("G-239", self._label(self._rec(id="panel-x@120x30")))
        # but the smoke baseline's ids must not
        self.assertIsNone(self._label(self._rec(id="home-empty@120x30")))

    def test_verdict_with_causes_adjudicated_passes(self):
        ref_cells, rust_cells = uniform("x"), uniform("x")
        ref_cells[0][0] = mkcell("x", fg=[100, 100, 100])
        rust_cells[0][0] = mkcell("x", fg=[107, 93, 108])
        r = compare(normalize(mkgrid(ref_cells)), normalize(mkgrid(rust_cells)))
        self.assertEqual(verdict([{"id": "any", **r}], CAUSES), 0)

    def test_verdict_with_causes_unknown_divergence_stays_red(self):
        rec = self._rec(id="home-empty",
                        first_diff={"row": 15, "col": 6, "field": "ch",
                                    "ref": " ", "rust": "B"})
        self.assertEqual(verdict([rec], CAUSES), 2)


class CausesTableTest(unittest.TestCase):
    def test_every_entry_is_label_matcher_judgment_triple(self):
        for entry in CAUSES:
            label, match, judgment = entry
            self.assertIsInstance(label, str)
            self.assertTrue(callable(match))
            self.assertEqual(len(judgment), 3)
            # both sides' file:line judgments must be present
            self.assertIn("ref", judgment[1])
            self.assertTrue("rust" in judgment[2] or "crates/" in judgment[2])


class DefensiveCompareTest(unittest.TestCase):
    """T5-review follow-ups: loud divergence instead of silent zip
    truncation, and load_side cells-shape validation."""

    def test_ragged_rows_diverge_loudly(self):
        ref_cells, rust_cells = uniform(), uniform()
        rust_cells[2].append(mkcell("x"))  # one row longer
        res = compare(normalize(mkgrid(ref_cells)), normalize(mkgrid(rust_cells)))
        self.assertEqual(res["verdict"], "diverge")
        self.assertEqual(res["first_diff"]["field"], "cells_shape")

    def test_fewer_rows_diverge_loudly(self):
        ref_cells = uniform()
        rust_cells = uniform()[:-1]  # a row short
        res = compare(normalize(mkgrid(ref_cells)), normalize(mkgrid(rust_cells)))
        self.assertEqual(res["verdict"], "diverge")
        self.assertEqual(res["first_diff"]["field"], "cells_shape")

    def test_load_side_rejects_cells_shape_mismatch(self):
        import json
        import tempfile
        from pathlib import Path
        import tools.diff.tui_fullscreen as tf
        with tempfile.TemporaryDirectory() as td:
            out = Path(td)
            # declares 8x4 but cells is 3 rows -> infra red (SystemExit 1)
            (out / "c1.json").write_text(json.dumps(
                {"schema": "tui-diff-grid/1", "id": "c1", "width": 8, "height": 4,
                 "cells": [[mkcell() for _ in range(8)] for _ in range(3)]}))
            cases = [{"id": "c1", "scenario": "s", "width": 8, "height": 4}]
            with self.assertRaises(SystemExit) as cm:
                tf.load_side(out, cases, "test")
            self.assertEqual(cm.exception.code, 1)


if __name__ == "__main__":
    unittest.main(verbosity=2)


class FramesInfraTest(unittest.TestCase):
    """A2 review fixes: missing frames on either side is infra-red,
    never a silent exact."""

    def _tmp_out(self, grid_extra: dict | None) -> "Path":
        import json
        import tempfile
        from pathlib import Path
        import tools.diff.tui_fullscreen as tf
        td = tempfile.TemporaryDirectory()
        out = Path(td.name)
        payload = {"schema": "tui-diff-grid/1", "id": "anim-x", "width": 8,
                   "height": 4, "cells": [[None] * 8 for _ in range(4)]}
        if grid_extra is not None:
            payload.update(grid_extra)
        (out / "anim-x.json").write_text(json.dumps(payload))
        self.addCleanup(td.cleanup)
        return out

    def test_anim_case_missing_frames_on_one_side_is_infra_red(self):
        import tools.diff.tui_fullscreen as tf
        # the side WITHOUT frames (rust) must be flagged: an anim
        # scenario (anim_ticks declared) that emits no frames is an
        # infra failure, not a comparable result
        out = self._tmp_out(None)  # no frames key at all
        cases = [{"id": "anim-x", "scenario": "s", "width": 8, "height": 4,
                  "anim_ticks": 8}]
        with self.assertRaises(SystemExit) as cm:
            tf.load_side(out, cases, "rust")
        self.assertEqual(cm.exception.code, 1)

    def test_anim_case_short_frames_is_infra_red(self):
        import tools.diff.tui_fullscreen as tf
        frames = [[[None] * 8 for _ in range(4)] for _ in range(3)]  # != K+1
        out = self._tmp_out({"frames": frames})
        cases = [{"id": "anim-x", "scenario": "s", "width": 8, "height": 4,
                  "anim_ticks": 8}]
        with self.assertRaises(SystemExit):
            tf.load_side(out, cases, "ref")

    def test_resting_case_with_frames_is_infra_red(self):
        import tools.diff.tui_fullscreen as tf
        frames = [[[None] * 8 for _ in range(4)] for _ in range(9)]
        out = self._tmp_out({"frames": frames})
        cases = [{"id": "anim-x", "scenario": "s", "width": 8, "height": 4}]
        with self.assertRaises(SystemExit):
            tf.load_side(out, cases, "ref")

    def test_compare_frames_empty_side_never_exact(self):
        import tools.diff.tui_fullscreen as tf
        g = mkgrid(uniform())
        res = tf.compare_frames({}, dict(g))  # ref side: no frames
        self.assertEqual(res["verdict"], "diverge")
        self.assertEqual(res["per_tick"], [])
        self.assertEqual(res["first_tick_diff"]["field"], "frames_length")
        # length asymmetry (zip truncation) is diverge too
        g2 = dict(g, frames=[uniform(), uniform()])
        g3 = dict(g, frames=[uniform()])
        res2 = tf.compare_frames(g3, g2)
        self.assertEqual(res2["verdict"], "diverge")
        self.assertEqual(res2["first_tick_diff"]["field"], "frames_length")


class ArtifactFamilyTest(unittest.TestCase):
    """The A2 animation-gate artifact family is RETIRED (gb17,
    2026-11-16): the animations gate is ported and anim-tool-progress
    is exact in both dimensions — the former glyph-pair cluster no
    longer matches ANY record (it was removed from CAUSES)."""

    def _rec(self, cid: str, field: str, ref, rust) -> dict:
        fd = {"row": 7, "col": 5, "field": field, "ref": ref, "rust": rust}
        return {"id": cid, "dimension": "frames", "verdict": "diverge",
                "first_tick_diff": {"tick": 0, **fd}, "per_tick": ["diverge"]}

    def _label(self, rec: dict) -> str | None:
        return adjudicate(rec)

    def test_retired_tool_progress_glyph_pair_no_longer_hits(self):
        # retired (gb17): the running-tool glyph pair (⠋ vs ⋯) of
        # anim-tool-progress used to hit the A2 artifact cluster; the
        # gate is ported now, so the shape is unadjudicated if it ever
        # reappears (a re-land regression must FAIL the lane, not be
        # silently waived).
        self.assertIsNone(self._label(self._rec("anim-tool-progress", "ch", "⋯", "⠋")))

    def test_non_tool_progress_anim_diverge_does_not_hit(self):
        # review finding: the old anim-* prefix swallowed ANY anim
        # divergence — e.g. a caret-blink diff must stay unadjudicated
        # (anim-toast-* is covered by the separate G-424 family; the
        # generic artifact cluster itself must not match it either)
        self.assertIsNone(self._label(self._rec("anim-spinner-running", "ch", "a", " ")))
        self.assertIsNone(self._label(self._rec("anim-stream-delta", "fg",
                                                [255, 255, 255], [128, 128, 128])))
        g424 = self._label(self._rec("anim-toast-window", "ch", "┃", " "))
        self.assertIsNotNone(g424)  # G-424's OWN family, not the artifact one
        self.assertIn("G-424", g424)

    def test_tool_progress_fg_diff_does_not_hit(self):
        # same scenario but a non-ch divergence: no match
        self.assertIsNone(self._label(
            self._rec("anim-tool-progress", "fg", [1, 2, 3], [4, 5, 6])))

    def test_tool_progress_other_glyph_pair_does_not_hit(self):
        # a NEW glyph shape needs a NEW evidenced entry, not a silent
        # widening of this cluster
        self.assertIsNone(self._label(self._rec("anim-tool-progress", "ch", "⠙", "⋯")))


class VersionSkewFamilyTest(unittest.TestCase):
    """The B2 version-skew cluster is evidence-gated: a plugin-* case
    matches ONLY when the record carries the ref-side 'Plugin failed'
    screen flag (the ref 2.0.24 harness could not boot the payload at
    all). Without the flag, G-28 owns the shape divergences."""

    def _rec(self, cid: str, failed: bool | None) -> dict:
        rec = {"id": cid, "dimension": "resting", "verdict": "diverge",
               "first_diff": {"row": 1, "col": 18, "field": "ch",
                              "ref": "┃", "rust": " "},
               "cells_diff_count": 445}
        if failed is not None:
            rec["ref_plugin_failed"] = failed
        return rec

    def test_plugin_case_with_ref_failure_hits_version_skew(self):
        label = adjudicate(self._rec("plugin-boot-idle", True))
        self.assertIsNotNone(label)
        self.assertIn("version-skew", label)

    def test_plugin_case_without_evidence_falls_to_g28(self):
        # no flag (a future corpus where ref boots the plugin): the
        # version-skew cluster must NOT match; G-28's bounded-shim
        # family takes the shape divergence instead
        label = adjudicate(self._rec("plugin-boot-idle", None))
        self.assertIsNotNone(label)
        self.assertIn("G-28", label)
        self.assertIsNotNone(adjudicate(self._rec("plugin-boot-idle", False)))

    def test_non_plugin_case_with_flag_never_hits(self):
        # the flag alone is not enough: a non-plugin-* id stays
        # unadjudicated (or hits its own family) — never this cluster
        rec = self._rec("session-text", True)
        self.assertTrue(all(not m(rec) for _, m, _ in CAUSES)
                        or adjudicate(rec) is None
                        or "version-skew" not in (adjudicate(rec) or ""))


class PluginSkipReconciliationTest(unittest.TestCase):
    """B2 plugins_unavailable consistency: both sides marked ⇒ skip;
    one side ⇒ infra (SystemExit); neither ⇒ survives."""

    def _grids(self, marked: bool) -> dict:
        grid = {"schema": "tui-diff-grid/1", "plugins_unavailable": marked}
        return {"case": grid}

    def test_both_sides_marked_skips_the_case(self):
        from tools.diff.tui_fullscreen import reconcile_plugin_skips
        cases = [{"id": "case"}]
        surviving, skipped = reconcile_plugin_skips(
            self._grids(True), self._grids(True), cases)
        self.assertEqual(surviving, [])
        self.assertEqual(skipped, ["case"])

    def test_one_side_marked_is_infra_red(self):
        from tools.diff.tui_fullscreen import reconcile_plugin_skips
        with self.assertRaises(SystemExit):
            reconcile_plugin_skips(self._grids(True), self._grids(False),
                                   [{"id": "case"}])
        with self.assertRaises(SystemExit):
            reconcile_plugin_skips(self._grids(False), self._grids(True),
                                   [{"id": "case"}])

    def test_unmarked_case_survives(self):
        from tools.diff.tui_fullscreen import reconcile_plugin_skips
        surviving, skipped = reconcile_plugin_skips(
            self._grids(False), self._grids(False), [{"id": "case"}])
        self.assertEqual([c["id"] for c in surviving], ["case"])
        self.assertEqual(skipped, [])

    def test_plugin_failed_predicate_reads_the_screen(self):
        from tools.diff.tui_fullscreen import _plugin_failed_on_ref
        blank = {"cells": [[{"ch": " "} for _ in range(4)] for _ in range(2)]}
        failed = {"cells": [[{"ch": c} for c in "Plug"]
                            for _ in range(1)]}
        failed = {"cells": [[{"ch": c} for c in "Plugin failed: /x"]
                            for _ in range(1)]}
        self.assertFalse(_plugin_failed_on_ref(blank))
        self.assertTrue(_plugin_failed_on_ref(failed))


class B2FixFamilyTest(unittest.TestCase):
    """The B2-fix adjudication split: plugin-fallback-* (deliberate
    broken payload) belongs to the G-425 failure-surface registration
    — NEVER the version-skew cluster, even with the ref failure flag;
    plugin-panel-open-* belongs to the loading-model cluster
    (unified-process server-entry projection) regardless of the
    flag; version-skew stays reserved for genuinely un-bootable
    payloads on other plugin-* ids."""

    def _rec(self, cid: str, failed: bool | None) -> dict:
        rec = {"id": cid, "dimension": "resting", "verdict": "diverge",
               "first_diff": {"row": 1, "col": 18, "field": "ch",
                              "ref": "┃", "rust": " "},
               "cells_diff_count": 500}
        if failed is not None:
            rec["ref_plugin_failed"] = failed
        return rec

    def test_fallback_with_ref_failure_flag_hits_g425_not_version_skew(self):
        label = adjudicate(self._rec("plugin-fallback", True))
        self.assertIsNotNone(label)
        self.assertIn("G-425", label)
        self.assertNotIn("version-skew", label)

    def test_panel_open_hits_loading_model_cluster(self):
        # without the ref-failure flag the loading-model cluster owns
        # it; WITH a hypothetical ref failure the earlier version-skew
        # entry takes precedence (whole-screen un-bootability)
        for failed in (False, None):
            label = adjudicate(self._rec("plugin-panel-open", failed))
            self.assertIsNotNone(label)
            self.assertIn("注册投影", label)
        self.assertIn("version-skew", adjudicate(self._rec("plugin-panel-open", True)))

    def test_version_skew_still_guards_other_plugin_ids(self):
        label = adjudicate(self._rec("plugin-boot-idle", True))
        self.assertIsNotNone(label)
        self.assertIn("version-skew", label)
        # without the ref-failure evidence it falls to G-28
        g28 = adjudicate(self._rec("plugin-boot-idle", None))
        self.assertIn("G-28", g28)

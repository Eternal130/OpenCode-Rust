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
unadjudicated divergence).
"""
from __future__ import annotations

import os
import sys
import unittest

sys.path.insert(0, os.path.join(os.path.dirname(__file__), "..", ".."))

from tools.diff.tui_fullscreen import compare, normalize, verdict  # noqa: E402


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
                {"schema": "tui-diff-grid/1", "id": "c1", "width": 8, "height": 4, "cells": []}))
            cases = [{"id": "c1", "scenario": "s", "width": 8, "height": 4}]
            grids = tf.load_side(out, cases, "test")
            self.assertEqual(grids["c1"].get("stub_gaps", []), [])
            self.assertIsNone(grids["c1"].get("error"))


if __name__ == "__main__":
    unittest.main(verbosity=2)

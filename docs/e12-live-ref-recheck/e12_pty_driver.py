#!/usr/bin/env python3
"""E12 lane PTY driver — boots a TUI binary ONLY inside a PTY this process
owns (pty.fork), drives the session-list dialog through the E12 corpus
queries, and captures settled screen snapshots via a pyte screen model.

Terminal-isolation rules honored:
  * every TUI child is forkpty-owned by THIS driver (never raw boots,
    never /dev/tty); probes write only to the owned PTY fd;
  * teardown kills only OUR OWN child by pid handle (waitpid), never by
    bare name/number scanning.

Usage:
  e12_pty_driver.py boot <label> --cwd DIR --argv-json JSON --env-json JSON \
      [--cols N] [--rows N] [--out DIR]
      ... interactive observe mode: dumps screens to OUT, waits for stdin cmd
  e12_pty_driver.py corpus <label> ... [--queries JSON]
      ... boots, opens the session list (leader l = ctrl+x l), runs the
          corpus queries, writes one JSON record per query + snapshots.

Output record fields: query, class, present (title->rowcount), no_match,
screen (final settled screen text).
"""
from __future__ import annotations

import argparse
import fcntl
import json
import os
import pty
import re
import select
import signal
import struct
import sys
import termios
import time

sys.path.insert(0, "/tmp/opencode/e12ref-pylibs")
import pyte  # noqa: E402

CTRL = {"ctrl+x": b"\x18", "esc": b"\x1b", "enter": b"\r", "ctrl+c": b"\x03", "ctrl+d": b"\x04"}
BACKSPACE = b"\x7f"

# Terminal capability auto-responder: the ref TUI (opentui renderer)
# probes DA1 / kitty-keyboard / CPR and leaves its key pipeline idle
# until the terminal answers — a real terminal replies, so the driver
# must too (writes go only to OUR OWN PTY master fd).
DA1_REPLY = b"\x1b[?62;22c"  # xterm-like VT220 + color
KITTY_REPLY = b"\x1b[?0u"  # kitty keyboard: no flags (legacy input)


class PtySession:
    """One TUI process inside a PTY owned by this driver."""

    def __init__(self, argv, env, cwd, cols=120, rows=40):
        self.cols, self.rows = cols, rows
        self.screen = pyte.Screen(cols, rows)
        self.stream = pyte.ByteStream(self.screen)
        self.dead = False
        self.tail = b""  # carry for split probe sequences
        pid, fd = pty.fork()
        if pid == 0:  # child — becomes the TUI, nothing else ever runs here
            try:
                os.chdir(cwd)
                for key in list(os.environ):
                    if key not in env:
                        os.environ.pop(key, None)
                os.environ.update(env)
                os.execvpe(argv[0], argv, env)
            except Exception:  # pragma: no cover
                os._exit(127)
        self.pid, self.fd = pid, fd
        fcntl.ioctl(fd, termios.TIOCSWINSZ, struct.pack("HHHH", rows, cols, 0, 0))

    # -- raw plumbing ------------------------------------------------------
    def _answer_probes(self, chunk: bytes):
        data = self.tail + chunk
        replies = b""
        idx = 0
        while True:
            esc = data.find(b"\x1b[", idx)
            if esc < 0:
                break
            rest = data[esc + 2 :]
            # params until a final byte 0x40-0x7e
            m = re.match(rb"[0-9;?<>]*[\x40-\x7e]", rest)
            if not m:
                break  # incomplete sequence — keep for next chunk
            seq = m.group(0)
            final = seq[-1:]
            params = seq[:-1]
            if final == b"c" and params in (b"", b"?62;22"):
                replies += DA1_REPLY
            elif final == b"u" and params.startswith(b"?"):
                replies += KITTY_REPLY
            elif final == b"n" and params == b"6":
                replies += b"\x1b[%d;%dR" % (self.screen.cursor.y + 1, self.screen.cursor.x + 1)
            idx = esc + 2 + len(seq)
        self.tail = data[max(idx - 16, len(data) - 16) :] if idx else data[-16:]
        if replies:
            try:
                os.write(self.fd, replies)
            except OSError:
                pass

    def pump(self, seconds: float):
        end = time.monotonic() + seconds
        while True:
            remain = end - time.monotonic()
            if remain <= 0:
                return
            try:
                ready, _, _ = select.select([self.fd], [], [], min(remain, 0.25))
            except InterruptedError:
                continue
            if not ready:
                continue
            try:
                chunk = os.read(self.fd, 65536)
            except OSError:
                self.dead = True
                return
            if not chunk:
                self.dead = True
                return
            self.stream.feed(chunk)
            self._answer_probes(chunk)


    def wait_quiesce(self, timeout: float = 25.0, quiet: float = 1.3) -> float:
        """Pump until `quiet` seconds with no output (settled screen)."""
        start = time.monotonic()
        last = time.monotonic()
        while time.monotonic() - start < timeout:
            try:
                ready, _, _ = select.select([self.fd], [], [], 0.2)
            except InterruptedError:
                continue
            if ready:
                try:
                    chunk = os.read(self.fd, 65536)
                except OSError:
                    self.dead = True
                    break
                if not chunk:
                    self.dead = True
                    break
                self.stream.feed(chunk)
                self._answer_probes(chunk)
                last = time.monotonic()
            elif time.monotonic() - last >= quiet:
                break
        return time.monotonic() - start

    def send(self, data: bytes):
        if not self.dead:
            os.write(self.fd, data)

    def snapshot(self) -> list[str]:
        return [line.rstrip() for line in self.screen.display]

    def close(self):
        """Kill ONLY our own child by pid handle, then reap it."""
        if not self.dead:
            try:
                os.kill(self.pid, signal.SIGTERM)
            except ProcessLookupError:
                pass
            deadline = time.monotonic() + 4.0
            while time.monotonic() < deadline:
                try:
                    if os.waitpid(self.pid, os.WNOHANG) == (self.pid, 0) or self._reaped:
                        break
                except ChildProcessError:
                    break
                try:
                    if os.waitpid(self.pid, os.WNOHANG)[0] != 0:
                        break
                except ChildProcessError:
                    break
                time.sleep(0.1)
            else:
                try:
                    os.kill(self.pid, signal.SIGKILL)
                except ProcessLookupError:
                    pass
        try:
            os.close(self.fd)
        except OSError:
            pass
        try:
            os.waitpid(self.pid, 0)
        except (ChildProcessError, OSError):
            pass

    _reaped = False


# -- corpus ------------------------------------------------------------------
TITLES = {
    "ses_alpha": "Fix login redirect loop",
    "ses_beta": "refactor: database migrations",
    "ses_gamma": "CJK 支付宝接入",
    "ses_delta": "café menu parser",
    "ses_epsilon": "CAFE overhaul",
    "ses_zeta": "Zalando API client",
    "ses_child": "Fix login redirect loop",  # same title as parent — count rows
    "ses_eta": "Wide  spaces  title",  # surrounding spaces may be trimmed by row render
    "ses_theta": "naïve UTF-8 benches",
}
# distinctive, screen-safe search keys (full strings; the input echo never
# contains these because queries are never the full title)
KEYS = {
    "ses_alpha": "Fix login redirect loop",
    "ses_beta": "refactor: database migrations",
    "ses_gamma": "支付宝接入",
    "ses_delta": "café menu parser",
    "ses_epsilon": "CAFE overhaul",
    "ses_zeta": "Zalando API client",
    "ses_eta": "Wide  spaces  title",
    "ses_theta": "naïve UTF-8 benches",
}
NO_MATCH_MARKS = ["No sessions found", "No sessions available", "No results found"]
SEARCHING_MARK = "Searching sessions"


def count_titles(screen: list[str]) -> dict:
    text_lines = screen
    present = {}
    for sid, key in KEYS.items():
        n = sum(1 for line in text_lines if key in line)
        if n:
            present[sid] = n
    marks = [m for m in NO_MATCH_MARKS if any(m in line for line in text_lines)]
    return {"present": present, "no_match_marks": marks}


def run_corpus(sess: PtySession, queries: list[dict], out_dir: str, label: str):
    os.makedirs(out_dir, exist_ok=True)
    records = []
    # settle fully (the first-run modal mounts late in boot), then
    # dismiss it (the old rig's invariant: esc before any other key)
    sess.wait_quiesce(timeout=40.0, quiet=2.0)
    sess.pump(4.0)
    sess.send(CTRL["esc"])
    sess.wait_quiesce(timeout=12.0, quiet=1.5)
    # open the session-list dialog: leader (ctrl+x) then l
    sess.send(CTRL["ctrl+x"])
    sess.pump(0.4)
    sess.send(b"l")
    sess.wait_quiesce(timeout=20.0, quiet=1.5)
    opened = sess.snapshot()
    if not any("Sessions" in line for line in opened):
        raise SystemExit(
            f"[e12-driver] session-list dialog did not open for {label}; "
            f"aborting before queries (see boot state in output dir)"
        )
    base = sess.snapshot()
    with open(os.path.join(out_dir, f"{label}-dialog-baseline.txt"), "w") as fh:
        fh.write("\n".join(base) + "\n")
    records.append(
        {
            "query": "",
            "class": "dialog-baseline(empty)",
            **count_titles(base),
            "screen": base,
        }
    )
    prev_len = 0
    for q in queries:
        query = q["query"]
        # clear the input: over-send backspaces (grapheme deletes; extra
        # backspaces on an empty input are consumed harmlessly by the field)
        n_bs = len(query) + 6 if query else prev_len + 6
        sess.send(BACKSPACE * max(n_bs, prev_len + 6))
        sess.pump(0.35)
        sess.send(query.encode("utf-8"))
        sess.wait_quiesce(timeout=15.0, quiet=1.5)  # > 150ms debounce + round trip
        settle1 = sess.snapshot()
        sess.pump(0.5)  # settle check: capture twice, keep the later one
        settle2 = sess.snapshot()
        snap = settle2 if settle2 != settle1 else settle1
        rec = {
            "query": query,
            "class": q.get("class", "?"),
            "query_codepoints": [f"U+{ord(c):04X}" for c in query],
            **count_titles(snap),
            "screen": snap,
        }
        records.append(rec)
        prev_len = len(query)
        with open(os.path.join(out_dir, f"{label}-q{len(records)-1:02d}.txt"), "w") as fh:
            fh.write("\n".join(snap) + "\n")
    # close dialog + exit TUI
    sess.send(CTRL["esc"])
    sess.pump(0.4)
    sess.send(CTRL["ctrl+c"])
    sess.pump(1.0)
    sess.send(CTRL["ctrl+d"])
    sess.pump(1.0)
    with open(os.path.join(out_dir, f"{label}-corpus.json"), "w") as fh:
        json.dump(records, fh, ensure_ascii=False, indent=1)
    return records


def run_perquery(make_session, queries: list[dict], out_dir: str, label: str):
    """One fresh TUI boot PER QUERY — no cross-query state, no input
    clearing. Each boot: settle, esc (first-run modal), leader+l, type
    the query, settle, capture."""
    os.makedirs(out_dir, exist_ok=True)
    records = []
    # baseline boot: the empty dialog row set (child-leak reference)
    sess = make_session()
    try:
        sess.wait_quiesce(timeout=40.0, quiet=2.0)
        sess.pump(4.0)
        sess.send(CTRL["esc"])
        sess.wait_quiesce(timeout=12.0, quiet=1.5)
        sess.send(CTRL["ctrl+x"])
        sess.pump(0.4)
        sess.send(b"l")
        sess.wait_quiesce(timeout=20.0, quiet=1.5)
        base = sess.snapshot()
        with open(os.path.join(out_dir, f"{label}-dialog-baseline.txt"), "w") as fh:
            fh.write("\n".join(base) + "\n")
        records.append({"query": "", "class": "dialog-baseline(empty)", **count_titles(base), "screen": base})
    finally:
        sess.close()
    for idx, q in enumerate(queries):
        query = q["query"]
        sess = make_session()
        try:
            sess.wait_quiesce(timeout=40.0, quiet=2.0)
            sess.pump(4.0)
            sess.send(CTRL["esc"])
            sess.wait_quiesce(timeout=12.0, quiet=1.5)
            sess.send(CTRL["ctrl+x"])
            sess.pump(0.4)
            sess.send(b"l")
            sess.wait_quiesce(timeout=20.0, quiet=1.5)
            if not any("Sessions" in line for line in sess.snapshot()):
                records.append(
                    {
                        "query": query,
                        "class": q.get("class", "?"),
                        "error": "dialog did not open",
                        **count_titles(sess.snapshot()),
                        "screen": sess.snapshot(),
                    }
                )
                continue
            sess.send(query.encode("utf-8"))
            sess.wait_quiesce(timeout=15.0, quiet=1.6)  # > 150ms debounce + round trip
            settle1 = sess.snapshot()
            sess.pump(0.6)
            settle2 = sess.snapshot()
            snap = settle2
            rec = {
                "query": query,
                "class": q.get("class", "?"),
                "query_codepoints": [f"U+{ord(c):04X}" for c in query],
                **count_titles(snap),
                "input_line": next(
                    (line.strip() for line in snap if line.strip() and line == line.strip()),
                    "",
                ),
                "screen": snap,
            }
            records.append(rec)
            with open(os.path.join(out_dir, f"{label}-q{idx:02d}.txt"), "w") as fh:
                fh.write("\n".join(snap) + "\n")
        finally:
            sess.close()
    with open(os.path.join(out_dir, f"{label}-corpus.json"), "w") as fh:
        json.dump(records, fh, ensure_ascii=False, indent=1)
    return records


def run_boot(sess: PtySession, out_dir: str, label: str, seconds: float):
    os.makedirs(out_dir, exist_ok=True)
    sess.wait_quiesce(timeout=30.0, quiet=2.0)
    snap = sess.snapshot()
    with open(os.path.join(out_dir, f"{label}-boot.txt"), "w") as fh:
        fh.write("\n".join(snap) + "\n")
    print("\n".join(snap))
    sess.pump(seconds)
    snap2 = sess.snapshot()
    with open(os.path.join(out_dir, f"{label}-boot-late.txt"), "w") as fh:
        fh.write("\n".join(snap2) + "\n")
    return snap, snap2


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("mode", choices=["boot", "corpus", "perquery"])
    ap.add_argument("label")
    ap.add_argument("--cwd", required=True)
    ap.add_argument("--argv-json", required=True)
    ap.add_argument("--env-json", required=True)
    ap.add_argument("--cols", type=int, default=120)
    ap.add_argument("--rows", type=int, default=40)
    ap.add_argument("--out", required=True)
    ap.add_argument("--queries", default="/opt/opencode-rust/tools/parity/spec-canaries/e12_search_corpus.json")
    ap.add_argument("--observe-seconds", type=float, default=3.0)
    args = ap.parse_args()

    argv = json.loads(args.argv_json)
    env = json.loads(args.env_json)
    sess = PtySession(argv, env, args.cwd, args.cols, args.rows)
    try:
        if args.mode == "boot":
            run_boot(sess, args.out, args.label, args.observe_seconds)
        else:
            queries = []
            with open(args.queries) as fh:
                queries = json.load(fh)["queries"]
            if args.mode == "corpus":
                run_corpus(sess, queries, args.out, args.label)
            else:
                def make_session():
                    return PtySession(argv, env, args.cwd, args.cols, args.rows)
                sess.close()
                sess = None  # type: ignore[assignment]
                run_perquery(make_session, queries, args.out, args.label)
    finally:
        if sess is not None:
            sess.close()


if __name__ == "__main__":
    main()

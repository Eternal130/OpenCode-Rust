#!/usr/bin/env python3
"""SCAN-EF2 F7 boot-trace probe (negative-space parity, ours-extra).

Terminal-isolation compliance: every TUI boot here runs under a PTY we
own (pty.fork), the app's stderr is redirected to a file, strace writes
to a file, and we SIGKILL the whole process group at window end. No
/dev/tty access, no raw terminal writes, small batches (one boot per
invocation; the lane sleeps between invocations).

Trace scope (spec F7): files written (openat O_WRONLY/O_CREAT/O_TRUNC,
creat, mkdir, rename, unlink), sockets opened (socket+connect+bind+
listen), with args. Env reads + events emitted are censused statically
elsewhere (see the lane log); this probe owns the fs/network window.

Usage:
  f7_boot_trace.py --label ours1 --binary <path> --window 18
  f7_boot_trace.py --label canary --binary <path> --canary
"""
import argparse
import os
import pty
import select
import signal
import subprocess
import sys
import time

BASE = "/tmp/opencode/scanef2/f7"


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--label", required=True)
    ap.add_argument("--binary", required=True)
    ap.add_argument("--window", type=float, default=18.0)
    ap.add_argument("--channel-env", default=None, help="extra KEY=VAL appended to the child env")
    ap.add_argument("--extra-arg", default=None, help="one extra CLI arg (e.g. --standalone)")
    ap.add_argument("--canary", action="store_true",
                    help="inject ONE extra fs write + ONE extra loopback "
                         "connect from a sibling process during the window "
                         "(the I8 F7 canary; product untouched)")
    args = ap.parse_args()

    os.makedirs(BASE, exist_ok=True)
    home = os.path.join(BASE, f"home-{args.label}-{int(time.time())}")
    cwd = os.path.join(BASE, f"proj-{args.label}-{int(time.time())}")
    os.makedirs(home, exist_ok=True)
    os.makedirs(cwd, exist_ok=True)

    env = {
        "HOME": home,
        "TERM": "xterm-256color",
        "TMPDIR": BASE,
        "PATH": "/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin",
        "LANG": "en_US.UTF-8",
    }
    if args.channel_env:
        for kv in args.channel_env.split(";"):
            if "=" in kv:
                k, v = kv.split("=", 1)
                env[k] = v
    strace_out = os.path.join(BASE, f"{args.label}.strace")
    stderr_out = os.path.join(BASE, f"{args.label}.stderr")
    ansi_out = os.path.join(BASE, f"{args.label}.ansi")

    cmd = [
        "/usr/bin/strace", "-f", "-qq",
        "-e", "trace=openat,creat,mkdir,mkdirat,rename,renameat,renameat2,unlink,unlinkat,connect,bind,listen",
        "-o", strace_out,
        args.binary,
    ]
    if args.extra_arg:
        cmd.append(args.extra_arg)
    if args.canary:
        sidecar = os.path.join(os.path.dirname(os.path.abspath(__file__)),
                               "f7_canary_sidecar.py")
        env["EF2_SIDECAR"] = sidecar
        cmd = ["/usr/bin/strace", "-f", "-qq",
               "-e", "trace=openat,creat,mkdir,mkdirat,rename,renameat,renameat2,unlink,unlinkat,connect,bind,listen",
               "-o", strace_out,
               "/bin/sh", "-c",
               'python3 "$EF2_SIDECAR" & exec "$EF2_BINARY"']
        env["EF2_BINARY"] = args.binary

    pid, fd = pty.fork()
    if pid == 0:
        os.chdir(cwd)
        # stderr to a file: keeps the owned-PTY stream clean and
        # guarantees no escape leakage outside the harness.
    # NOTE: pty.fork child continues below; stderr redirect there.
        with open(stderr_out, "wb") as err:
            os.dup2(err.fileno(), 2)
        os.execve(cmd[0], cmd, env)
        os._exit(127)

    t0 = time.time()
    buf = b""
    sent_key = False
    while time.time() - t0 < args.window:
        r, _, _ = select.select([fd], [], [], 0.2)
        if r:
            try:
                d = os.read(fd, 65536)
                if not d:
                    break
                buf += d
            except OSError:
                break
        # one keystroke mid-window: exercises the composer path
        if not sent_key and time.time() - t0 > 6.0:
            try:
                os.write(fd, b"z")
                sent_key = True
            except OSError:
                pass
    try:
        os.killpg(os.getpgid(pid), signal.SIGKILL)
    except Exception:
        pass
    try:
        os.kill(pid, signal.SIGKILL)
        os.waitpid(pid, 0)
    except Exception:
        pass
    # strace child may lag writing -o output; give it a beat
    time.sleep(1.0)
    with open(ansi_out, "wb") as f:
        f.write(buf)
    print(json_summary(args.label, strace_out, len(buf)))
    return 0


def json_summary(label: str, strace_out: str, bytes_captured: int) -> str:
    import json
    n_syscalls = 0
    try:
        with open(strace_out, "r", errors="replace") as f:
            n_syscalls = sum(1 for _ in f)
    except FileNotFoundError:
        pass
    return json.dumps({"label": label, "strace_lines": n_syscalls,
                       "pty_bytes": bytes_captured})


if __name__ == "__main__":
    sys.exit(main())

import os, pty, sys, time, signal, select, re, json, subprocess

ANSI_CSI = re.compile(rb'\x1b\[[0-9;?]*[a-zA-Z]')
ANSI_OSC = re.compile(rb'\x1b\][^\x07]*\x07')
ANSI_ELSE = re.compile(rb'\x1b[^[]')

def strip_ansi(b):
    b = ANSI_OSC.sub(b'', b)
    b = ANSI_CSI.sub(b'', b)
    b = ANSI_ELSE.sub(b'', b)
    return b.decode('utf-8', 'replace')

def run_once(idx, total_window=90):
    env = dict(os.environ)
    env["HOME"] = f"/tmp/opencode/spec-r7/ours-home-tti{idx}_{int(time.time())}"
    env["TERM"] = "xterm-256color"
    os.makedirs(env["HOME"], exist_ok=True)
    cwd = "/tmp/opencode/spec-r7/proj-ours"
    os.makedirs(cwd, exist_ok=True)
    pid, fd = pty.fork()
    if pid == 0:
        os.chdir(cwd)

        b = os.environ.get("EF2_BIN", "/root/.opencode/bin/opencode-rust"); a = [b] + ([x for x in os.environ.get("EF2_ARGS","").split(",") if x]); os.execvpe(b, a, env)
    import fcntl, struct, termios
    fcntl.ioctl(fd, termios.TIOCSWINSZ, struct.pack("HHHH", 24, 80, 0, 0))
    t0 = time.time()
    buf = b""
    state = {"echo": None, "session": None}
    probed = False
    last_probe = 0.0
    while time.time() - t0 < total_window:
        r, _, _ = select.select([fd], [], [], 0.2)
        if r:
            try:
                d = os.read(fd, 65536)
                if not d:
                    break
                buf += d
            except OSError:
                break
        now = time.time() - t0
        v = strip_ansi(buf)
        if state["session"] is None and re.search(r'opencode|Ctrl|session|Start', v, re.I):
            state["session"] = now
        if state["session"] is not None and not probed and now - last_probe >= 2.0:
            try:
                os.write(fd, b"\x10")  # ctrl+p opens palette (our home screen first input)
                time.sleep(0.1)
                os.write(fd, b"z")
            except OSError:
                break
            last_probe = now
            time.sleep(0.2)
            try:
                r2, _, _ = select.select([fd], [], [], 0.1)
                if r2:
                    d2 = os.read(fd, 65536)
                    if d2:
                        buf += d2
            except OSError:
                pass
            v2 = strip_ansi(buf)
            if v2.count('z') > v.count('z'):
                state["echo"] = time.time() - t0
                probed = True
        if probed:
            break
    try:
        os.killpg(os.getpgid(pid), signal.SIGKILL)  # own pty.fork() child (own session) — allowed pattern
    except Exception:
        pass
    try:
        os.kill(pid, signal.SIGKILL)
        os.waitpid(pid, 0)
    except Exception:
        pass
    open(f"/tmp/opencode/spec-r7/ours_tti_{idx}.ansi", "wb").write(buf)
    return {"run": idx,
            "t_session": round(state["session"], 1) if state["session"] else None,
            "t_echo": round(state["echo"], 1) if state["echo"] else None,
            "bytes": len(buf)}

if __name__ == "__main__":
    n = int(sys.argv[1]) if len(sys.argv) > 1 else 3
    results = []
    for i in range(n):
        time.sleep(1.0)
        r = run_once(i)
        results.append(r)
        print(json.dumps(r), flush=True)
    json.dump(results, open("/tmp/opencode/spec-r7/ours_tti_results.json", "w"), indent=1)

import os, pty, sys, time, signal, select
env = dict(os.environ)
env["HOME"] = "/tmp/opencode/spec-r7/ref-home-src"
env["TERM"] = "xterm-256color"
os.makedirs(env["HOME"], exist_ok=True)
cmd = ["bun", "run", "dev"]
cwd = "/tmp/opencode/ref/packages/cli"
pid, fd = pty.fork()
if pid == 0:
    os.chdir(cwd); os.execvpe(cmd[0], cmd, env)
start = time.time(); buf=b""; deadline=start+30; first_byte=None
while time.time() < deadline:
    try:
        r,_,_ = select.select([fd],[],[],0.5)
        if r:
            d = os.read(fd, 65536)
            if not d: break
            if first_byte is None: first_byte = time.time()-start
            buf += d
    except OSError: break
elapsed=time.time()-start
try: os.kill(pid, signal.SIGKILL)
except Exception: pass
open("/tmp/opencode/spec-r7/boot_probe_src3.ansi","wb").write(buf)
txt = buf.decode("utf-8","replace")
print(f"elapsed={elapsed:.1f}s bytes={len(buf)} first_byte={first_byte}")
print("module_err:", "Cannot find module" in txt)
print("wordmark:", "opencode" in txt.lower())
import re
print("session_ui:", bool(re.search(r"Context|Session|Untitled|composer|Ctrl", txt, re.I)))
print("spinner:", bool(re.search(r"[⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏]", txt)))
for line in txt.splitlines():
    if "error" in line.lower() or "panic" in line.lower():
        print("ERR-LINE:", line[:160]); break

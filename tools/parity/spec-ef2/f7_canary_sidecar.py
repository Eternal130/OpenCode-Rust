#!/usr/bin/env python3
"""F7 I8 canary sidecar: performs exactly ONE extra fs write + ONE extra
loopback connect at t+4s inside the traced window. The product binary is
never modified — this simulates 'ours doing an extra write + extra
network call' for the negative-space differ."""
import socket
import time

time.sleep(4.0)
with open("/tmp/opencode/scanef2/f7/canary-extra-write.txt", "w") as f:
    f.write("canary")
try:
    s = socket.socket()
    s.settimeout(1.0)
    s.connect(("127.0.0.1", 9))  # discard port: connect() observable, no server needed
except OSError:
    pass
time.sleep(30)

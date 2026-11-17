#!/usr/bin/env python3
"""Channel 9 recording proxy for real opencode v2 daily traffic.

Sits between the real opencode v2 client and the real provider. The
user flips each provider's baseURL in their opencode config to point
at this proxy (`http://127.0.0.1:PORT/r/<route>/...`); the proxy
forwards to the real upstream (`--routes routes.json` maps route name
-> base URL) and accumulates every exchange as a cassette-v2 file in
a local corpus dir. The future channel-9 replay gateway replays those
cassettes with TTFT/TPS timing fidelity.

Cassette v2 format (spec frozen by the controller; the W5 gateway
lane owns merging playback):

  { "version": 2,
    "metadata": { "recordedAt": "...", "proxy": "record_proxy/1",
                  "route": "anthropic", "model": "<from body>",
                  "transport": "http" },
    "interactions": [ { "transport": "http",
        "request":  { "method", "path", "headers_redacted", "body" },
        "response": { "status", "headers_redacted",
                      "frames": [ { "blob", "delay_ms" } ] } } ] }

  frames: one entry per forwarded chunk for SSE streams (delay_ms =
  ms since the previous chunk; the first frame's delay_ms IS the
  TTFT); non-SSE bodies collapse to a single frame.

Privacy iron rules:
  * Authorization / x-api-key / api-key (and cookie) headers are
    forwarded upstream but NEVER written to a cassette -- they are
    replaced by "<redacted>" in headers_redacted.
  * Cassettes only ever land in the user-designated --corpus-dir on
    the local machine.

Fidelity:
  * SSE is forwarded chunk-by-chunk with zero added latency (first
    upstream byte is flushed to the client immediately).
  * Status and headers are passed through verbatim except
    hop-by-hop headers.
  * Upstream TLS uses the python standard library.

Crash safety: each finished interaction is written atomically
(tmp file + os.replace); a proxy restart never damages existing
cassettes.

Usage:
  record_proxy.py --routes routes.json --corpus-dir DIR [--port N]
                  [--host 127.0.0.1]
"""

from __future__ import annotations

import argparse
import http.client
import json
import os
import re
import secrets
import socketserver
import sys
import threading
import time
import urllib.parse
from datetime import datetime, timezone
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

PROXY_ID = "record_proxy/1"
CASSETTE_VERSION = 2

# Hop-by-hop headers never forwarded in either direction (RFC 2616
# 13.5.1 + connection-specific tokens).
HOP_BY_HOP = {
    "connection", "keep-alive", "proxy-authenticate",
    "proxy-authorization", "te", "trailers", "transfer-encoding",
    "upgrade", "host", "content-length",
}

# Headers forwarded upstream but never persisted to a cassette.
REDACTED_HEADERS = {
    "authorization", "proxy-authorization", "x-api-key", "api-key",
    "cookie",
}

REDACTED = "<redacted>"

ROUTE_PREFIX_RE = re.compile(r"^/r/([^/]+)(/.*)?$")


def redact_headers(headers: dict[str, str]) -> dict[str, str]:
    out = {}
    for name, value in headers.items():
        out[name] = REDACTED if name.lower() in REDACTED_HEADERS else value
    return out


def pick_headers(raw) -> dict[str, str]:
    """Flatten handler headers into a plain dict (last value wins)."""
    return {k: v for k, v in raw.items()}


class CassetteWriter:
    """Atomically writes one cassette file per finished interaction."""

    def __init__(self, corpus_dir: str):
        self.corpus_dir = os.path.abspath(corpus_dir)
        os.makedirs(self.corpus_dir, exist_ok=True)
        self._lock = threading.Lock()

    def write(self, metadata: dict, interaction: dict) -> str:
        cassette = {
            "version": CASSETTE_VERSION,
            "metadata": metadata,
            "interactions": [interaction],
        }
        recorded = datetime.now(timezone.utc)
        name = "rec-{}-{}.json".format(
            recorded.strftime("%Y%m%dT%H%M%S"), secrets.token_hex(4))
        path = os.path.join(self.corpus_dir, name)
        blob = json.dumps(cassette, ensure_ascii=False, indent=1)
        tmp = path + ".tmp"
        with self._lock:
            with open(tmp, "w", encoding="utf-8") as fh:
                fh.write(blob)
            os.replace(tmp, path)  # atomic on POSIX
        return path


class RecordingProxyHandler(BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"
    server_version = "record-proxy/1"

    # Injected by serve().
    routes: dict[str, str] = {}
    cassette_writer: CassetteWriter | None = None

    # ------------------------------------------------------------------
    def log_message(self, fmt, *args):  # keep stdout tidy
        sys.stderr.write("[proxy] %s - %s\n" % (self.address_string(),
                                                fmt % args))

    def do_GET(self):
        self._handle("GET")

    def do_POST(self):
        self._handle("POST")

    def do_DELETE(self):
        self._handle("DELETE")

    def do_PUT(self):
        self._handle("PUT")

    # ------------------------------------------------------------------
    def _handle(self, method: str):
        match = ROUTE_PREFIX_RE.match(self.path)
        if not match:
            self._fail(400, "bad proxy path; expected /r/<route>/<path>")
            return
        route, rest = match.group(1), match.group(2) or "/"
        base = self.routes.get(route)
        if base is None:
            self._fail(404, "unknown route %r; known: %s"
                       % (route, ", ".join(sorted(self.routes)) or "<none>"))
            return
        parsed = urllib.parse.urlsplit(base)
        if parsed.scheme not in ("http", "https"):
            self._fail(502, "routes.json base URL must be http(s)")
            return
        upstream_path = rest
        if parsed.path:
            upstream_path = parsed.path.rstrip("/") + rest

        try:
            body = self._read_request_body()
        except Exception as exc:  # bad content-length etc.
            self._fail(400, "bad request body: %s" % exc)
            return

        out_headers = {}
        for name, value in self.headers.items():
            if name.lower() in HOP_BY_HOP:
                continue
            out_headers[name] = value
        out_headers["Host"] = parsed.netloc

        conn_cls = (http.client.HTTPSConnection if parsed.scheme == "https"
                    else http.client.HTTPConnection)
        upstream = conn_cls(parsed.hostname, parsed.port, timeout=600)
        sent_at = time.monotonic()
        try:
            upstream.request(method, upstream_path, body=body,
                             headers=out_headers)
            resp = upstream.getresponse()
        except Exception as exc:
            upstream.close()
            self._fail(502, "upstream error: %s" % exc, record=False)
            return

        try:
            self._relay_and_record(resp, method, route, rest, body,
                                   out_headers, sent_at)
        finally:
            upstream.close()

    # ------------------------------------------------------------------
    def _read_request_body(self) -> bytes:
        length = self.headers.get("Content-Length")
        if length is None:
            te = (self.headers.get("Transfer-Encoding") or "").lower()
            if "chunked" in te:
                chunks = []
                while True:
                    line = self.rfile.readline(65536).strip()
                    size = int(line.split(b";")[0], 16)
                    if size == 0:
                        # trailing headers until blank line
                        while True:
                            trailer = self.rfile.readline(65536)
                            if trailer in (b"\r\n", b"\n", b""):
                                break
                        return b"".join(chunks)
                    chunks.append(self.rfile.read(size))
                    self.rfile.read(2)  # CRLF
                # not reached
            return b""
        n = int(length)
        if n <= 0:
            return b""
        data = b""
        while len(data) < n:
            piece = self.rfile.read(n - len(data))
            if not piece:
                break
            data += piece
        return data

    # ------------------------------------------------------------------
    def _relay_and_record(self, resp: http.client.HTTPResponse, method,
                          route, path, req_body: bytes, req_headers,
                          sent_at: float):
        is_sse = "text/event-stream" in (
            resp.getheader("Content-Type") or "").lower()

        resp_headers = [(k, v) for k, v in resp.getheaders()
                        if k.lower() not in HOP_BY_HOP]

        self.send_response_only(resp.status, resp.reason)
        for name, value in resp_headers:
            self.send_header(name, value)
        frames = []
        if is_sse:
            # Stream chunk-by-chunk: first upstream byte goes straight
            # to the client (zero added TTFT latency).
            self.send_header("Transfer-Encoding", "chunked")
            self.end_headers()
            last = sent_at
            while True:
                chunk = resp.read1(65536)
                if not chunk:
                    break
                now = time.monotonic()
                frames.append({"blob": chunk.decode("utf-8", "replace"),
                               "delay_ms": round((now - last) * 1000, 3)})
                last = now
                self.wfile.write(b"%x\r\n" % len(chunk) + chunk + b"\r\n")
                self.wfile.flush()
            self.wfile.write(b"0\r\n\r\n")
            self.wfile.flush()
        else:
            # Buffer the full body: single cassette frame, exact
            # content-length relay.
            body = resp.read()
            now = time.monotonic()
            frames.append({"blob": body.decode("utf-8", "replace"),
                           "delay_ms": round((now - sent_at) * 1000, 3)})
            self.send_header("Content-Length", str(len(body)))
            self.end_headers()
            self.wfile.write(body)
            self.wfile.flush()

        interaction = {
            "transport": "http",
            "request": {
                "method": method,
                "path": path,
                "headers_redacted": redact_headers(
                    {k: v for k, v in req_headers.items()}),
                "body": req_body.decode("utf-8", "replace"),
            },
            "response": {
                "status": resp.status,
                "headers_redacted": redact_headers(dict(resp_headers)),
                "frames": frames,
            },
        }
        metadata = {
            "recordedAt": datetime.now(timezone.utc).isoformat(),
            "proxy": PROXY_ID,
            "route": route,
            "model": _model_from_body(req_body),
            "transport": "http",
        }
        if self.cassette_writer is not None:
            try:
                self.cassette_writer.write(metadata, interaction)
            except Exception as exc:
                sys.stderr.write("[proxy] cassette write failed: %s\n" % exc)

    def _fail(self, status: int, message: str, record: bool = False):
        try:
            blob = json.dumps({"error": message}).encode()
            self.send_response(status)
            self.send_header("Content-Type", "application/json")
            self.send_header("Content-Length", str(len(blob)))
            self.end_headers()
            self.wfile.write(blob)
        except Exception:
            pass


def _model_from_body(body: bytes) -> str:
    try:
        parsed = json.loads(body)
        if isinstance(parsed, dict) and isinstance(parsed.get("model"),
                                                    str):
            return parsed["model"]
    except (ValueError, UnicodeDecodeError):
        pass
    return ""


class ThreadingProxyServer(ThreadingHTTPServer):
    daemon_threads = True
    allow_reuse_address = True


def serve(host: str, port: int, routes: dict[str, str],
          corpus_dir: str) -> ThreadingProxyServer:
    handler = type("BoundHandler", (RecordingProxyHandler,), {
        "routes": routes,
        "cassette_writer": CassetteWriter(corpus_dir),
    })
    server = ThreadingProxyServer((host, port), handler)
    return server


def main(argv=None) -> int:
    parser = argparse.ArgumentParser(
        prog="record_proxy.py",
        description=("Record real opencode v2 provider traffic into "
                     "cassette-v2 files for channel-9 replay. Point "
                     "each provider baseURL at "
                     "http://HOST:PORT/r/<route>/ and pass --routes "
                     "mapping <route> to the real upstream base URL."))
    parser.add_argument("--routes", required=True,
                        help="JSON file mapping route name to upstream "
                             "base URL, e.g. "
                             '{"anthropic": "https://api.anthropic.com",'
                             ' "openai": "https://api.openai.com"}')
    parser.add_argument("--corpus-dir", required=True,
                        help="Local directory for recorded cassettes "
                             "(created if missing; secrets never land "
                             "here).")
    parser.add_argument("--host", default="127.0.0.1",
                        help="Listen host (default 127.0.0.1).")
    parser.add_argument("--port", type=int, default=8099,
                        help="Listen port (default 8099; 0 = pick one).")
    args = parser.parse_args(argv)

    try:
        with open(args.routes, "r", encoding="utf-8") as fh:
            routes = json.load(fh)
    except (OSError, ValueError) as exc:
        parser.error("cannot load --routes %s: %s" % (args.routes, exc))
        return 2
    if not isinstance(routes, dict) or not all(
            isinstance(k, str) and isinstance(v, str)
            for k, v in routes.items()):
        parser.error("--routes must be a JSON object of string -> string")
        return 2

    server = serve(args.host, args.port, routes, args.corpus_dir)
    host, port = server.server_address[:2]
    print("[proxy] listening on http://%s:%d" % (host, port), flush=True)
    print("[proxy] routes: %s" % ", ".join(
        "%s -> %s" % (k, v) for k, v in sorted(routes.items())), flush=True)
    print("[proxy] corpus dir: %s" % os.path.abspath(args.corpus_dir),
          flush=True)
    try:
        server.serve_forever()
    except KeyboardInterrupt:
        pass
    finally:
        server.server_close()
    return 0


if __name__ == "__main__":
    sys.exit(main())

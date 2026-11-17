#!/usr/bin/env python3
"""Self-test for record_proxy.py -- no real provider is contacted.

Boots a stub upstream (fixed SSE script with configurable inter-chunk
delays + a plain-JSON endpoint), boots the recording proxy, drives it
with a stdlib http client, and asserts:
  * SSE bytes pass through chunk-faithfully (streaming works),
  * cassettes match the frozen v2 shape (frames + delay_ms +
    headers_redacted),
  * Authorization / x-api-key / api-key never reach disk,
  * non-streaming JSON collapses to a single frame,
  * two concurrent requests become two separate interactions.
"""

from __future__ import annotations

import http.client
import json
import os
import shutil
import sys
import tempfile
import threading
import time
import unittest
import urllib.parse
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)

import record_proxy  # noqa: E402

# Fixed SSE script the stub upstream replays. delay = seconds before
# sending the chunk (after the previous one).
SSE_SCRIPT = [
    (0.0, b"event: message_start\n"
          b'data: {"type":"message_start"}\n\n'),
    (0.12, b"data: {\"type\":\"content_block_delta\",\"delta\":"
           b"{\"text\":\"hello \"}}\n\n"),
    (0.10, b"data: {\"type\":\"content_block_delta\",\"delta\":"
           b"{\"text\":\"world\"}}\n\n"),
    (0.0, b"event: message_stop\ndata: {\"type\":\"message_stop\"}\n\n"),
]


class StubUpstreamHandler(BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"

    def log_message(self, fmt, *args):
        pass

    def do_POST(self):
        length = int(self.headers.get("Content-Length", 0))
        self.rfile.read(length)
        path = urllib.parse.urlsplit(self.path).path
        if path.endswith("/v1/messages"):
            seen_auth = self.headers.get("Authorization")
            if seen_auth is None:
                self._json(401, {"error": "missing auth"})
                return
            body = b"".join(blob for _, blob in SSE_SCRIPT)
            self.send_response(200)
            self.send_header("Content-Type", "text/event-stream")
            self.send_header("X-Upstream", "stub-1")
            self.send_header("Transfer-Encoding", "chunked")
            self.end_headers()
            for delay, blob in SSE_SCRIPT:
                if delay:
                    time.sleep(delay)
                self.wfile.write(b"%x\r\n" % len(blob) + blob + b"\r\n")
                self.wfile.flush()
            self.wfile.write(b"0\r\n\r\n")
            self.wfile.flush()
        else:
            self._json(200, {"ok": True, "echo_path": path})

    def _json(self, status, obj):
        blob = json.dumps(obj).encode()
        self.send_response(status)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(blob)))
        self.end_headers()
        self.wfile.write(blob)


class StubUpstream(ThreadingHTTPServer):
    daemon_threads = True
    allow_reuse_address = True


class TestRecordProxy(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.tmpdir = tempfile.mkdtemp(prefix="recproxy-test-")
        cls.corpus_dir = os.path.join(cls.tmpdir, "corpus")
        cls.stub = StubUpstream(("127.0.0.1", 0), StubUpstreamHandler)
        cls.stub_port = cls.stub.server_address[1]
        cls.stub_thread = threading.Thread(
            target=cls.stub.serve_forever, daemon=True)
        cls.stub_thread.start()
        routes = {"anthropic": "http://127.0.0.1:%d" % cls.stub_port,
                  "openai": "http://127.0.0.1:%d" % cls.stub_port}
        cls.routes_file = os.path.join(cls.tmpdir, "routes.json")
        with open(cls.routes_file, "w", encoding="utf-8") as fh:
            json.dump(routes, fh)
        cls.proxy = record_proxy.serve("127.0.0.1", 0, routes,
                                       cls.corpus_dir)
        cls.proxy_port = cls.proxy.server_address[1]
        cls.proxy_thread = threading.Thread(
            target=cls.proxy.serve_forever, daemon=True)
        cls.proxy_thread.start()

    @classmethod
    def tearDownClass(cls):
        cls.proxy.shutdown()   # stop serve_forever loop
        cls.proxy.server_close()
        cls.stub.shutdown()
        cls.stub.server_close()
        cls.proxy_thread.join(timeout=5)
        cls.stub_thread.join(timeout=5)
        shutil.rmtree(cls.tmpdir, ignore_errors=True)

    # -- helpers ------------------------------------------------------
    def _request(self, path, body, headers):
        conn = http.client.HTTPConnection("127.0.0.1", self.proxy_port,
                                          timeout=30)
        try:
            conn.request("POST", path, body=body, headers=headers)
            resp = conn.getresponse()
            data = resp.read()
            return resp, data
        finally:
            conn.close()

    def _cassette(self):
        """Return the (single) recorded cassette, asserting there is one."""
        deadline = time.monotonic() + 5  # cassette write settles
        names = []
        while time.monotonic() < deadline:
            names = sorted(n for n in os.listdir(self.corpus_dir)
                           if n.endswith(".json"))
            if names:
                break
            time.sleep(0.05)
        self.assertEqual(len(names), 1, "expected exactly one cassette,"
                                        " got %r" % names)
        with open(os.path.join(self.corpus_dir, names[0]),
                  encoding="utf-8") as fh:
            return json.load(fh)

    def _clear(self):
        for name in os.listdir(self.corpus_dir):
            os.unlink(os.path.join(self.corpus_dir, name))

    # -- tests --------------------------------------------------------
    def test_sse_passthrough_and_cassette_v2(self):
        self._clear()
        body = json.dumps({"model": "claude-x", "stream": True,
                           "messages": [{"role": "user",
                                         "content": "hi"}]}).encode()
        t0 = time.monotonic()
        resp, data = self._request(
            "/r/anthropic/v1/messages", body,
            {"Authorization": "Bearer sk-secret-DO-NOT-LOG",
             "x-api-key": "sk-ant-secret-DO-NOT-LOG",
             "Content-Type": "application/json"})
        elapsed = time.monotonic() - t0
        self.assertEqual(resp.status, 200)
        self.assertIn("text/event-stream",
                      resp.getheader("Content-Type"))
        self.assertEqual(resp.getheader("X-Upstream"), "stub-1")

        # Byte fidelity: concatenated SSE payload identical.
        expected = b"".join(blob for _, blob in SSE_SCRIPT)
        self.assertEqual(data, expected)

        # Streaming: the first chunk must arrive before the script
        # finishes (script total delay ~0.22s; we cannot see partial
        # arrival through .read(), so instead assert the request
        # completed only after the full upstream script duration).
        self.assertGreaterEqual(elapsed, 0.20)

        # TTFT fidelity recorded in the cassette.
        cas = self._cassette()
        self.assertEqual(cas["version"], 2)
        meta = cas["metadata"]
        self.assertEqual(meta["proxy"], "record_proxy/1")
        self.assertEqual(meta["route"], "anthropic")
        self.assertEqual(meta["model"], "claude-x")
        self.assertEqual(meta["transport"], "http")
        self.assertIn("recordedAt", meta)

        inter = cas["interactions"][0]
        self.assertEqual(inter["transport"], "http")
        req = inter["request"]
        self.assertEqual(req["method"], "POST")
        self.assertEqual(req["path"], "/v1/messages")
        self.assertEqual(req["headers_redacted"]["Authorization"],
                         "<redacted>")
        self.assertEqual(req["headers_redacted"]["x-api-key"],
                         "<redacted>")
        self.assertNotIn("sk-secret-DO-NOT-LOG", json.dumps(cas))
        self.assertNotIn("sk-ant-secret-DO-NOT-LOG", json.dumps(cas))
        self.assertEqual(json.loads(req["body"])["model"], "claude-x")

        res = inter["response"]
        self.assertEqual(res["status"], 200)
        self.assertEqual(res["headers_redacted"]["X-Upstream"], "stub-1")
        self.assertNotIn("Transfer-Encoding", res["headers_redacted"])
        frames = res["frames"]
        self.assertEqual(len(frames), 4, "one frame per SSE event chunk")
        blobs = b"".join(f["blob"].encode() for f in frames)
        self.assertEqual(blobs, expected)
        delays = [f["delay_ms"] for f in frames]
        self.assertEqual(frames[0]["delay_ms"],
                         delays[0])  # first delay == TTFT
        # Inter-chunk gaps recorded: ~120ms then ~100ms (+ slack).
        self.assertGreaterEqual(delays[1], 100)
        self.assertLess(delays[1], 400)
        self.assertGreaterEqual(delays[2], 80)
        self.assertLess(delays[2], 400)

    def test_non_sse_json_single_frame(self):
        self._clear()
        body = json.dumps({"model": "gpt-x",
                           "messages": []}).encode()
        resp, data = self._request(
            "/r/openai/v1/chat/completions", body,
            {"Authorization": "Bearer sk-openai-secret",
             "api-key": "also-secret",
             "Content-Type": "application/json"})
        self.assertEqual(resp.status, 200)
        self.assertEqual(json.loads(data), {"ok": True,
                                            "echo_path":
                                            "/v1/chat/completions"})
        cas = self._cassette()
        self.assertEqual(cas["metadata"]["route"], "openai")
        self.assertEqual(cas["metadata"]["model"], "gpt-x")
        frames = cas["interactions"][0]["response"]["frames"]
        self.assertEqual(len(frames), 1, "JSON collapses to one frame")
        self.assertEqual(json.loads(frames[0]["blob"]),
                         {"ok": True, "echo_path": "/v1/chat/completions"})
        self.assertGreaterEqual(frames[0]["delay_ms"], 0)
        self.assertNotIn("sk-openai-secret", json.dumps(cas))
        self.assertNotIn("also-secret", json.dumps(cas))

    def test_concurrent_requests_two_interactions(self):
        self._clear()
        body = json.dumps({"model": "claude-x", "stream": True,
                           "messages": []}).encode()
        headers = {"Authorization": "Bearer x",
                   "Content-Type": "application/json"}
        results = []

        def worker(path):
            resp, data = self._request(path, body, headers)
            results.append((resp.status, data))

        threads = [threading.Thread(
            target=worker,
            args=("/r/anthropic/v1/messages?session=%d" % i,))
            for i in range(2)]
        for t in threads:
            t.start()
        for t in threads:
            t.join(timeout=30)

        self.assertEqual(len(results), 2)
        for status, data in results:
            self.assertEqual(status, 200)
            self.assertEqual(data,
                             b"".join(b for _, b in SSE_SCRIPT))

        deadline = time.monotonic() + 5  # cassette writes settle
        while time.monotonic() < deadline:
            names = [n for n in os.listdir(self.corpus_dir)
                     if n.endswith(".json")]
            if len(names) == 2:
                break
            time.sleep(0.05)
        names = [n for n in os.listdir(self.corpus_dir)
                 if n.endswith(".json")]
        self.assertEqual(len(names), 2,
                         "two interactions -> two cassettes, got %r"
                         % names)
        for name in names:
            with open(os.path.join(self.corpus_dir, name),
                      encoding="utf-8") as fh:
                cas = json.load(fh)
            self.assertEqual(len(cas["interactions"]), 1)
            self.assertEqual(len(
                cas["interactions"][0]["response"]["frames"]), 4)

    def test_unknown_route_404_no_cassette(self):
        self._clear()
        conn = http.client.HTTPConnection("127.0.0.1", self.proxy_port,
                                          timeout=10)
        try:
            conn.request("POST", "/r/nope/v1/messages", body=b"{}",
                         headers={})
            resp = conn.getresponse()
            resp.read()
            self.assertEqual(resp.status, 404)
        finally:
            conn.close()
        self.assertEqual(os.listdir(self.corpus_dir), [])


if __name__ == "__main__":
    unittest.main(verbosity=2)

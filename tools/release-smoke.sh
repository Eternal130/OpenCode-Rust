#!/usr/bin/env bash
# Release smoke (Phase 10 packaging): verifies a RELEASE-built binary
# end-to-end before it ships. Run as the repo user:
#
#   tools/release-smoke.sh [path-to-release-binary]
#
# Checks (all sandboxed under /tmp/opencode; NEVER binds 49374):
#   1. `opencode-rust --version` reports exactly `opencode-rust v2.0.24`
#      (service version negotiation is exact-string; MIGRATION §2.4).
#   2. `opencode-rust --version-rust` reports the build stamp (git hash).
#   3. `opencode-rust serve` boots in a sandbox home on a RANDOM port
#      (--port 0), answers /api/info with the pinned version over
#      Basic auth, and exits cleanly on SIGTERM.
#   4. The full FakeLlm e2e battery (oc-cli tests/run_e2e.rs — the
#      standalone run/stdio-lease flows) runs against THIS binary via
#      OPENCODE_SMOKE_BIN.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
BIN="${1:-${OPENCODE_RELEASE_BIN:-$ROOT/target/release/opencode-rust}}"
EXPECTED_VERSION="opencode-rust v2.0.24"

if [ ! -x "$BIN" ]; then
    echo "release smoke: building release binary first ($BIN)"
    (cd "$ROOT" && cargo build --release -p oc-cli)
fi

fail() { echo "release smoke: FAIL: $*" >&2; exit 1; }
pass() { echo "release smoke: PASS: $*"; }

# --- 1+2. version banners ---------------------------------------------------
got=$("$BIN" --version)
[ "$got" = "$EXPECTED_VERSION" ] || fail "--version printed '$got', expected '$EXPECTED_VERSION'"
pass "--version == '$EXPECTED_VERSION'"

got=$("$BIN" --version-rust)
echo "$got" | grep -Eq 'opencode-rust v2\.0\.24 \(rust port build [0-9a-f]+, (debug|release) profile\)$' \
    || fail "--version-rust malformed: '$got'"
pass "--version-rust stamp: $got"

# --- 3. serve boot on a random port -----------------------------------------
SANDBOX=$(mktemp -d /tmp/opencode/release-smoke-XXXXXX)
trap 'rm -rf "$SANDBOX"' EXIT
mkdir -p "$SANDBOX/home"
PORT_PASSWORD="smoke-password"
export OPENCODE_TEST_HOME="$SANDBOX/home"
export OPENCODE_PASSWORD="$PORT_PASSWORD"

"$BIN" serve --hostname 127.0.0.1 --port 0 >"$SANDBOX/serve.log" 2>"$SANDBOX/serve.err" &
SERVE_PID=$!
trap 'if command -v ours >/dev/null 2>&1; then ours "$SERVE_PID" && kill "$SERVE_PID" 2>/dev/null || true; else kill "$SERVE_PID" 2>/dev/null || true; fi; rm -rf "$SANDBOX"' EXIT

URL=""
for _ in $(seq 1 100); do
    URL=$(sed -n 's/^server listening on //p' "$SANDBOX/serve.log" | head -n 1)
    [ -n "$URL" ] && break
    kill -0 "$SERVE_PID" 2>/dev/null || { cat "$SANDBOX/serve.err" >&2; fail "serve exited before listening"; }
    sleep 0.2
done
[ -n "$URL" ] || { cat "$SANDBOX/serve.err" >&2; fail "serve never printed its listening line"; }

PORT=$(printf '%s' "$URL" | sed -n 's#.*:\([0-9][0-9]*\)/*$#\1#p')
[ -n "$PORT" ] || fail "could not parse port from '$URL'"
[ "$PORT" != "49374" ] || fail "serve bound the managed-service default port 49374 (forbidden in smoke)"

INFO=$(curl -fsS -u "opencode:$PORT_PASSWORD" "$URL/api/info") || fail "/api/info unreachable at $URL"
echo "$INFO" | grep -q '"version":"2.0.24"' || fail "/api/info version mismatch: $INFO"
pass "serve booted on random port $PORT; /api/info reports 2.0.24"

# Stop OUR serve child safely: $! of a direct spawn is our own child, but a
# dead child's PID can be recycled between the kill -0 checks above and the
# TERM below — guard the signal with a cmdline basename check (safekill's
# Expectation::Foreign semantics, shell form) so we never signal a stranger.
ours() {
    [ -r "/proc/$1/cmdline" ] || return 1
    case "$(tr '\0' ' ' < "/proc/$1/cmdline")" in
        *opencode-rust*|*oc-server*) return 0 ;;
        *) return 1 ;;
    esac
}
if ours "$SERVE_PID"; then
    kill "$SERVE_PID"
else
    fail "serve pid $SERVE_PID no longer looks like our binary — refusing to signal"
fi
wait "$SERVE_PID" || fail "serve did not exit cleanly on SIGTERM"
pass "serve exited cleanly on SIGTERM"
unset OPENCODE_TEST_HOME OPENCODE_PASSWORD

# --- 4. FakeLlm e2e battery against the release binary ----------------------
echo "release smoke: running the oc-cli run_e2e battery against $BIN"
(cd "$ROOT" && OPENCODE_SMOKE_BIN="$BIN" cargo nextest run -p oc-cli --test run_e2e) \
    || fail "run_e2e battery failed against the release binary"
pass "run_e2e (FakeLlm standalone flows) green against the release binary"

echo "release smoke: ALL GREEN ($BIN)"

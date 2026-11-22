#!/usr/bin/env bash
# Provision the channel-9 real-plugin payload registry (W8 persistence:
# the payload moved from /tmp staging to this repo-local, git-ignored
# directory — it survives reboots and is visible to every worktree lane
# via the harness default tools/diff/realchain/plugin-payload).
#
# Layout produced (consumed by realchain_harness::plugin_payload_root,
# override via T9_PLUGIN_PAYLOAD_ROOT):
#   tools/diff/realchain/plugin-payload/<name>/<stamp>/node_modules/<pkg>
#
# Sources, in order: the host's REAL install cache (entry layout
# `~/.cache/opencode/npm/<name>@<key>/<stamp>/node_modules/<pkg>` —
# `<key>` is the install-time key, e.g. `latest` or a git sha, NOT the
# version pin; any key matches) then (legacy W5 staging, if present)
# /tmp/opencode/plugin-payload/<name>/<stamp>/node_modules/<pkg>
#
# The payloads are published npm packages (oh-my-opencode-slim@3.0.3,
# superpowers@7.0.0), not user data — the pin is the privacy boundary.
set -euo pipefail

REPO=$(cd "$(dirname "$0")/../../.." && pwd)
DEST=${T9_PLUGIN_PAYLOAD_ROOT:-$REPO/tools/diff/realchain/plugin-payload}
HOST_CACHE=${HOME}/.cache/opencode/npm
LEGACY=/tmp/opencode/plugin-payload

# (registry name, npm spec)
NAME_SPECS="oh-my-opencode-slim:oh-my-opencode-slim@3.0.3 superpowers:superpowers@7.0.0"

newest_stamp() { # <dir-with-stamp-dirs> -> stdout: newest stamp dir carrying node_modules
  local best="" d
  for d in "$1"/*/; do
    [[ -d "${d}node_modules" ]] || continue
    d=${d%/}
    if [[ -z "$best" || "${d##*/}" > "${best##*/}" ]]; then best=$d; fi
  done
  [[ -n "$best" ]] && printf '%s\n' "$best"
  return 0
}

mkdir -p "$DEST"
for pair in $NAME_SPECS; do
  name=${pair%%:*}
  spec=${pair##*:}
  target=$DEST/$name
  src=""
  # Host cache: any install key for the package name (npm and git
  # install keys); newest stamp wins.
  for cand in "$HOST_CACHE/$name@"*/ "$HOST_CACHE/git-$name-"*/; do
    [[ -d $cand ]] || continue
    s=$(newest_stamp "$cand")
    if [[ -n $s && ( -z $src || "${s##*/}" > "${src##*/}" ) ]]; then src=$s; fi
  done
  if [[ -z "$src" && -d "$LEGACY/$name" ]]; then
    src=$(newest_stamp "$LEGACY/$name")
  fi
  if [[ -z "$src" ]]; then
    echo "error: no payload found for $name (checked $HOST_CACHE/$name@* and $LEGACY/$name)" >&2
    exit 1
  fi
  rm -rf "$target"
  mkdir -p "$target"
  cp -a "$src" "$target/"
  echo "staged $spec <- $src"
done
echo "plugin payload registry ready at $DEST (git-ignored; never commit)"

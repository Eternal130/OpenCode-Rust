#!/usr/bin/env bash
# Channel-9 corpus age pipeline (W8): encrypt/decrypt REAL recorded
# corpus bundles for the CI lane (realchain-diff.yml).
#
#   tools/diff/realchain/corpus_age.sh encrypt <dir> <bundle.tar.age>
#   tools/diff/realchain/corpus_age.sh decrypt <bundle.tar.age> <dir>
#
# Mode: age PUBLIC-KEY (asymmetric — the CI holds only the public key
# `CORPUS_AGE_PUBKEY` (an age recipient, "age1...") to ENCRYPT result
# artifacts; the private identity `CORPUS_AGE_KEY` (an age identity
# file path, or the "AGE-SECRET-KEY-1..." string itself) to DECRYPT
# corpus bundles. A leaked CI environment can therefore encrypt but
# never decrypt.
#
# Missing the age binary? One-liner (linux-amd64):
#   curl -sL https://github.com/FiloSottile/age/releases/download/v1.2.1/age-v1.2.1-linux-amd64.tar.gz \
#     | tar xz -C /tmp/opencode --strip-components=1 age/age age/age-keygen
# (see README-CI.md for other platforms)
#
# Bundle layout convention (see corpus/README-CI.md): the plaintext dir
# may carry corpus dirs (corpus/, corpus-ref/, corpus-w5/, corpus-tools/,
# corpus-tools-ref/) and an optional plugin-payload/ registry.
set -euo pipefail

if ! command -v age >/dev/null 2>&1; then
  echo "error: age binary not on PATH — install hint:" >&2
  echo "  curl -sL https://github.com/FiloSottile/age/releases/download/v1.2.1/age-v1.2.1-linux-amd64.tar.gz | tar xz -C /tmp/opencode --strip-components=1 age/age age/age-keygen" >&2
  exit 3
fi

usage() {
  sed -n '2,12p' "$0" >&2
  exit 2
}

[[ $# -eq 3 ]] || usage
mode=$1; a=$2; b=$3

case $mode in
encrypt)
  [[ -d $a ]] || { echo "error: plaintext dir not found: $a" >&2; exit 2; }
  [[ -n ${CORPUS_AGE_PUBKEY:-} ]] || {
    echo "error: CORPUS_AGE_PUBKEY (age recipient, age1...) not set" >&2; exit 3; }
  mkdir -p "$(dirname "$b")"
  tar -C "$a" -cf - . | age -r "$CORPUS_AGE_PUBKEY" > "$b"
  echo "encrypted $a -> $b ($(du -h "$b" | cut -f1))"
  ;;
decrypt)
  [[ -f $a ]] || { echo "error: bundle not found: $a" >&2; exit 2; }
  key=${CORPUS_AGE_KEY:-}
  [[ -n $key ]] || { echo "error: CORPUS_AGE_KEY (identity file path or AGE-SECRET-KEY-1... string) not set" >&2; exit 3; }
  identity=$key
  cleanup=""
  # Accept the bare secret string OR a pasted age-keygen file (the
  # AGE-SECRET-KEY-1 line is extracted; "# comment" lines dropped).
  secret=$(printf '%s\n' "$key" | grep -o "AGE-SECRET-KEY-1[A-Z0-9]*" | head -1 || true)
  if [[ -n $secret ]]; then
    identity=$(mktemp /tmp/opencode/age-identity.XXXXXX)
    cleanup=$identity
    printf '%s\n' "$secret" > "$identity"
    chmod 600 "$identity"
  fi
  mkdir -p "$b"
  age -d -i "$identity" "$a" | tar -C "$b" -xf -
  [[ -z $cleanup ]] || rm -f "$cleanup"
  echo "decrypted $a -> $b"
  ;;
*)
  usage
  ;;
esac

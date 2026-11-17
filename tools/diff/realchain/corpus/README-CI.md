# CI corpus pipeline (channel 9, W8)

Real recorded traffic may ONLY travel encrypted. This directory tree
(`tools/diff/realchain/corpus/encrypted/`) is the one place recorded
corpus enters git — always as `*.tar.age` bundles produced by
`tools/diff/realchain/corpus_age.sh`. Plaintext JSON cassettes are
FORBIDDEN here (guard `recorded_local` reds on any non-`.age` tracked
file under this directory).

## Pipeline (record → CI)

1. **Record** on a lane-carrying machine via the zcode-proxy /
   `tools/diff/realchain/record_proxy.py` flow (RECORD-PROXY.md).
   Output = request-shape cassettes per engine side.
2. **Filter**: drop every interaction with `truncated != null`, failed
   interactions, and any cassette whose scenario is not part of the
   target CI battery. Keep the set minimal — every cassette is attack
   surface for a leak.
3. **De-sensitization review**: the gateway redaction set
   (`--redact` sandbox-root substitution, `__TOOLRESULT__` blanking,
   `__SECS__` wall-clock normalization) already strips the known red
   lines (sandbox paths, tool payloads, timings). BEYOND that, a human
   MUST spot-check content: open each cassette and grep for API keys,
   tokens, account ids, hostnames, and anything user-typed that is not
   part of the scripted scenario. The scripted seeds
   (`t9x_*_bootstrap_seed`) contain only synthetic traffic by
   construction — recorded `corpus-w5`-style additions are the ones to
   re-review every time.
4. **Encrypt**:
   `CORPUS_AGE_PUBKEY=age1... tools/diff/realchain/corpus_age.sh encrypt <filtered-dir> tools/diff/realchain/corpus/encrypted/<name>.tar.age`
   The public key lives in the CI env (`CORPUS_AGE_PUBKEY`); the
   private identity (`CORPUS_AGE_KEY`) NEVER enters CI — CI only
   *writes* artifacts to this key, it cannot read corpus.
5. **Commit** the `.tar.age` bundle (git-tracked; this directory is
   deliberately NOT git-ignored). Optionally include a
   `plugin-payload/` registry inside the bundle (layout of
   `fetch_plugin_payload.sh` output) so the CI T9-7 plugin scenario
   runs with the real payloads.

## CI consumption (realchain-diff.yml)

`workflow_dispatch` with `corpus_bundle = <bundle name>`: the job
requires the `CORPUS_AGE_KEY` secret (missing secret + named bundle =
red), decrypts into a scratch dir OUTSIDE the repo (`/tmp/opencode/
realchain-corpus`), then overlays the carried corpus dirs onto the
committed synthetic ones (untracked adds; the run never commits) and
stages `plugin-payload/` into the repo-local registry path. Without a
`corpus_bundle` the job runs the full SYNTHETIC battery with zero
secrets — the default dispatch path.

Result artifacts (grid dumps, gateway logs, misses) are encrypted to
the `CORPUS_AGE_PUBKEY` recipient before upload; download and
`CORPUS_AGE_KEY=... age -d` locally to read.

## Keys

One keypair per corpus generation. Generate:
`age-keygen -o key.txt` (public key printed on stdout). CI env:
`CORPUS_AGE_PUBKEY` (repo-level variable, public) and secret
`CORPUS_AGE_KEY` (holder machines only). Rotate by re-encrypting the
directory and deleting the old bundles in the same commit.

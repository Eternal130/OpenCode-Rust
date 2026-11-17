# Encrypted corpus bundles (channel 9 CI)

Only `*.tar.age` bundles produced by `tools/diff/realchain/corpus_age.sh
encrypt` may live here (see ../README-CI.md). Anything else tracked in
this directory reds the `recorded_local` guard. The directory is
deliberately git-tracked (NOT ignored): CI fetches bundles from the
repo. There are no bundles yet — the first real recorded corpus lands
with the controller's initial key generation.

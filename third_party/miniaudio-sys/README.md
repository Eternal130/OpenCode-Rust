# miniaudio-sys (vendored)

Raw FFI bindings for [miniaudio](https://miniaud.io) **0.11.22**, vendored
byte-for-byte from opentui's native engine
(`third_party/opentui/packages/native/src/vendor/miniaudio/miniaudio.h`,
same `MA_ENABLE_ONLY_SPECIFIC_BACKENDS` + `MA_ENABLE_ALSA` +
`MA_ENABLE_PULSEAUDIO` shim configuration) so the Rust TUI's in-process
sound channel runs **the same audio engine as v2** (`@opentui/core`
`Audio` → Zig native → miniaudio).

## Why vendored (not crates.io)

- `cpal`/`rodio` need `-lasound` at link time (`alsa`/`alsa-sys`, plus
  `jack`/`pipewire`/`pulseaudio` sys crates in cpal 0.18). Neither the dev
  box nor CI (`ubuntu-latest`, no `apt-get`) carries the ALSA dev symlink,
  and every one of those is a hard link-time native dependency.
- The `miniaudio` crates.io crate (0.10.0 → `ep-miniaudio-sys` 2.4.0) builds
  with **bindgen 0.54 at build time**, which panics on modern glibc headers
  and requires libclang discovery during the build.

This crate instead compiles the single miniaudio translation unit with `cc`
and ships **pre-generated bindings** (`src/bindings.rs`), so a clean build
needs neither bindgen, libclang, nor any system audio dev headers. On Linux
miniaudio resolves ALSA/PulseAudio at **runtime via `dlopen`** (the crate
links only `dl`/`pthread`/`m`): on machines without them, device
initialization fails and the caller degrades to a silent no-op — exactly v2's
`Audio.create` → catch → `null` behavior.

## Layout

- `src/miniaudio/miniaudio.h` — miniaudio 0.11.22, unmodified.
- `src/shim.c` — the compile unit: `#define MINIAUDIO_IMPLEMENTATION` plus
  the backend selection (Linux: ALSA + PulseAudio only, runtime linking;
  no null backend, mirroring opentui's `miniaudio_shim.c`).
- `src/bindings.rs` — pre-generated with bindgen 0.70.
- `build.rs` — `cc` only.

## Regenerating bindings (maintainers only)

```sh
cd /tmp && cargo new ma-bindgen --bin && cd ma-bindgen
cargo add bindgen@0.70
# main.rs: Builder::default().header("<repo>/third_party/miniaudio-sys/src/shim.c")
#   .allowlist_type/function/var per src/bindings.rs coverage
#   .size_t_is_usize(true).layout_tests(false)
LIBCLANG_PATH=/usr/lib/llvm-18/lib cargo run --release -- \
  <repo>/third_party/miniaudio-sys/src/shim.c \
  <repo>/third_party/miniaudio-sys/src/bindings.rs
```

The allow-list intentionally covers only the public surface used by
`crates/oc-tui/src/sound/` (engine/device/decoder/data-source/sound APIs);
adding a symbol means regenerating with a wider list.

## License

miniaudio is MIT-0 OR Apache-2.0 (see the header's license section). The
shim mirrors opentui (MIT).

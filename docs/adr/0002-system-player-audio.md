# ADR-0002: 提示音播放——Linux 走进程内 vendored miniaudio（v2 parity 既成决定），其余平台走系统播放器 spawn

- 状态：accepted（2026-10-18 用户裁决：接受 inproc miniaudio 为既成决定；原 §11.3 "非进程内 miniaudio" 措辞就此修订）
- 日期：2026-10-18（原记录）；2026-10-18 用户裁决落地
- 上游版本：2.0.24 (ref e7a34f09bf)
- 背景：v2 的 TUI 在进程内解码并播放内置 MP3 音频包（opentui native 的 vendored miniaudio，经 `packages/tui/src/audio.ts` 暴露）。Rust 移植最初（R-06，commit `bff58df`，2026-10-11）没有移植原生音频引擎，而是把播放委托给系统播放器：detached `spawn`、stdio 全 null、按 `pw-play → paplay → ffplay → mpv → canberra-gtk-play` 顺序探测、各自做音量刻度映射。PARITY-LOOP §11.3 据此记录了"声音用系统播放器，非进程内 miniaudio"的旧措辞。此后的 P2-sound lane（commits `b430891`/`0fabd7d`，2026-10-16，已在 main）在 Linux target 上落地了 vendored miniaudio 的进程内播放路径以对齐 v2，系统播放器 spawn 保留为非 Linux 回退。
- 决定：**用户裁决（2026-10-18）：Linux 的进程内 vendored miniaudio 路径是 v2 parity 的既成决定，予以接受。** 声音播放的平台分流为最终形态：Linux = 进程内 vendored miniaudio（`sound/inproc.rs` + `third_party/miniaudio-sys`）；非 Linux = 系统播放器进程（detached spawn、stdio null、候选循环首个成功者胜、无播放器即静默 no-op），播放 argv/音量映射经 `SoundSpawner` seam 可测。PARITY-LOOP §11.3 的"非进程内 miniaudio"措辞视为被本裁决取代（历史措辞保留于本 ADR 背景以供追溯）。
- 后果：代价是非 Linux 平台依赖外部播放器存在（缺失即无声，与 v2 的"无设备静默 no-op"可观察近似但不位等同）、音量刻度映射按播放器各异、spawn 路径无法报告播放完成；Linux 的进程内路径则引入 vendored `miniaudio-sys` FFI 依赖（含 unsafe）。收益是 Linux 与 v2 行为/资产位等同，TUI 不绑定 v2 的 opentui/js 音频栈，回退路径零设备依赖。文档残留：`README.md:20` 仍把 "sounds" 列在 PORT_REPORT §3 deferred 尾单里，属文档滞后（实现已落地，release 前随 §12.4 README 一致性清理）。
- 证据（path:line / 测试）：
  - `crates/oc-tui/src/sound.rs:1-28`（模块文档：v2 进程内 miniaudio、本移植的平台分流说明与播放器表）
  - `crates/oc-tui/src/sound.rs:30-52`（`#[cfg(target_os = "linux")] pub mod inproc`；`SoundHalf` 在 Linux = `InprocSoundPlayer`，非 Linux = `ProcessSoundPlayer`）
  - `crates/oc-tui/src/sound.rs:106-169`（`PlayerKind` 探测顺序与各播放器 argv/音量映射）、`sound.rs:174-185`（PATH 探测 `detect_player`，不执行被探测程序）
  - `crates/oc-tui/src/sound.rs:196-208`（`ProcessSpawner`：detached spawn、stdio 全 null）、`sound.rs:214-226`（`play_file`/`play_with` 候选循环）
  - `crates/oc-tui/src/sound/inproc.rs:1-28`（Linux-only 进程内 miniaudio 端口与 v2 parity 对照表）
  - `crates/oc-tui/Cargo.toml:42-47`（vendored `miniaudio-sys` path 依赖）
  - 测试：`crates/oc-tui/src/sound.rs:289`（`player_argv_maps_volume`）、`:331`（`play_file_requires_a_player`）、`:347`（`candidate_loop_falls_through`）、`:368`（`extracts_builtin_pack_byte_for_byte`）
  - 历史：`bff58df`（R-06 系统播放器移植）、`b430891`/`0fabd7d`（P2-sound Linux 进程内 miniaudio）
  - ref 对照：`/opt/opencode-ref/packages/tui/src/audio.ts`、`attention-sounds.node.ts`（v2 进程内解码播放 + 资产落盘）
- 复审日期：Batch 2 结束

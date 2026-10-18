# ADR-0002: 提示音播放走系统播放器（spawn 外部播放器进程），不引入进程内音频栈作为通用形态

- 状态：accepted（记录既有决定；依据 PARITY-LOOP §11.3）
- 日期：2026-10-18
- 上游版本：2.0.24 (ref e7a34f09bf)
- 背景：v2 的 TUI 在进程内解码并播放内置 MP3 音频包（opentui native 的 vendored miniaudio，经 `packages/tui/src/audio.ts` 暴露）。Rust 移植最初（R-06，commit `ed00a20`，2026-10-11）没有移植原生音频引擎，而是把播放委托给系统播放器：detached `spawn`、stdio 全 null、按 `pw-play → paplay → ffplay → mpv → canberra-gtk-play` 顺序探测、各自做音量刻度映射。PARITY-LOOP §11.3 据此记录本决定："声音用系统播放器，非进程内 miniaudio"。此后的 P2-sound lane（commits `109bc80`/`eab1362`，2026-10-16，已在 main）又在 Linux target 上落地了 vendored miniaudio 的进程内播放路径以对齐 v2，系统播放器 spawn 保留为非 Linux 回退——这与 §11.3 的措辞存在张力，见下方 OPEN。
- 决定：声音播放默认形态是系统播放器进程（detached spawn、stdio null、候选循环首个成功者胜、无播放器即静默 no-op），不把进程内音频栈作为跨平台通用形态引入；播放_argv/音量映射经 `SoundSpawner` seam 可测。当前既成实现按平台分流：Linux 用 vendored miniaudio 进程内播放（P2-sound，v2 可观察等价），其余平台用系统播放器 spawn 镜像。
- 后果：代价是非 Linux 平台依赖外部播放器存在（缺失即无声，与 v2 的"无设备静默 no-op"可观察近似但不位等同）、音量刻度映射按播放器各异、spawn 路径无法报告播放完成；Linux 的进程内路径则引入 vendored `miniaudio-sys` FFI 依赖（`third_party/miniaudio-sys`，含 unsafe）。收益是 TUI 不绑定 v2 的 opentui/js 音频栈，回退路径零设备依赖。
- OPEN（未决，非本 ADR 决定）：PARITY-LOOP §11.3 措辞为"非进程内 miniaudio"，但 main 自 2026-10-16 起在 Linux target 含进程内 vendored miniaudio 路径（`crates/oc-tui/src/sound/inproc.rs` + `third_party/miniaudio-sys`）。需裁决：修订本 ADR 措辞以接受 Linux 进程内路径为 v2 parity 的既成决定，或按 §11.3 原意退役 inproc 路径。归属簇：TUI / P2-sound 残留。另：`README.md:20` 仍把 "sounds" 列在 PORT_REPORT §3 deferred 尾单里，属文档滞后（实现已落地）。
- 证据（path:line / 测试）：
  - `crates/oc-tui/src/sound.rs:1-28`（模块文档：v2 进程内 miniaudio、本移植的平台分流说明与播放器表）
  - `crates/oc-tui/src/sound.rs:30-52`（`#[cfg(target_os = "linux")] pub mod inproc`；`SoundHalf` 在 Linux = `InprocSoundPlayer`，非 Linux = `ProcessSoundPlayer`）
  - `crates/oc-tui/src/sound.rs:106-169`（`PlayerKind` 探测顺序与各播放器 argv/音量映射）、`sound.rs:174-185`（PATH 探测 `detect_player`，不执行被探测程序）
  - `crates/oc-tui/src/sound.rs:196-208`（`ProcessSpawner`：detached spawn、stdio 全 null）、`sound.rs:214-226`（`play_file`/`play_with` 候选循环）
  - `crates/oc-tui/src/sound/inproc.rs:1-28`（Linux-only 进程内 miniaudio 端口与 v2 parity 对照表）
  - `crates/oc-tui/Cargo.toml:42-47`（vendored `miniaudio-sys` path 依赖）
  - 测试：`crates/oc-tui/src/sound.rs:289`（`player_argv_maps_volume`）、`:331`（`play_file_requires_a_player`）、`:347`（`candidate_loop_falls_through`）、`:368`（`extracts_builtin_pack_byte_for_byte`）
  - 历史：`ed00a20`（R-06 系统播放器移植）、`109bc80`/`eab1362`（P2-sound Linux 进程内 miniaudio）
  - ref 对照：`/opt/opencode-ref/packages/tui/src/audio.ts`、`attention-sounds.node.ts`（v2 进程内解码播放 + 资产落盘）
- 复审日期：Batch 2 结束

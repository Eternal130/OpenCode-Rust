# Task 4 Report — 编排器骨架（manifest、双侧调度、infra 语义）

## 交付

`tools/diff/tui_fullscreen.py`（python3 单文件，通道 7 先例风格）：

- **docstring 规格书**（:1-56，照 provider_recordings.py :1-43）：corpus 来源、
  双侧驱动与 staging 映射（本通道无 node_modules symlink —— T2 driver 用绝对路径
  preload）、尺寸档位镜像规则、infra 语义、request_census 不比对声明、子进程纪律
  （无 killpg —— 控制者裁定 http_full.py 的 killpg allowlist 路径专属，不得复制）、
  范围外（判门 exit 2 属 T5，CAUSES 属 T6）。
- **build_manifest**：STAGE（/tmp/opencode/diff-tuifull）全量重建，场景拷入
  `STAGE/scenarios/`，主档（文件 width/height，无后缀 id）+ 镜像档 120×30
  （`<id>@120x30`），按 id 排序，`tui-diff-manifest/1`。manifest 内 scenario 路径
  为 **STAGE 绝对路径**——rust driver 经 nextest 跑在 crate cwd，无法解析 ref 运行
  cwd 相对路径；ref driver 的 `path.resolve(cwd, p)` 对绝对路径等价成立。
- **run_ref**：driver 拷 STAGE，`bun --preload $DIFF_REF_ROOT/packages/tui/node_modules/
  @opentui/solid/scripts/preload.js`（绝对路径），env `DIFF_REF_ROOT`。
- **run_rust**：`tools/dev-gate.sh 'TUI_DIFF_MANIFEST=… TUI_DIFF_OUT=… cargo nextest run
  -p oc-tui --test tui_fullscreen_diff_driver'`（env 前缀折进同一命令串）。
- **infra 语义**：驱动 rc≠0 → SystemExit(1)（双侧输出尾部透传 stderr）；任一 case
  `stub_gaps` 非空 / grid 文件缺失 / grid 带 error → load_side 汇总 infra failure 后
  SystemExit(1)。request_census 只落报告不比对。exit 0 = 报告产出（T4 范围）。
- 驱动输出日志落 `/tmp/opencode/logs/t4-ref-driver.log` / `t4-rust-driver.log`（追加式）。

## dev-gate 身份适配（实现中发现）

dev-gate 以 eternal130 执行 rust 腿；root 建的 STAGE 目录对其不可写
（`create out dir: PermissionDenied`）。修复：STAGE/scenarios 及双侧 out 目录
chmod 0o777（代码内注释说明）。

## 验证（全部通过）

1. `--manifest-only`：6 cases（3 场景 × 2 档）按 id 排序输出 → rc 0。
2. smoke 端到端：双侧各 6 个 `tui-diff-grid/1` 网格落到 STAGE（ref-out / rust-out），
   gaps=0，报告写出 → rc 0。
3. 故意断桩（home-empty 去掉 `/api/config` 后还原）：ref driver rc=1（STUB GAPS
   PRESENT）→ 编排器 SystemExit(1) → exit=1。corpus 已还原（git status 干净）。
4. 还原后复跑 clean：exit 0。

## Concerns

- ref driver 把渲染帧文本打到 stdout，断桩时编排器 stderr 尾部透传较长（-4000 截断，通道 7 同款）。
- 断桩情形下 ref driver 自身以 rc=1 先退，编排器走 "driver failed" 分支而非 load_side 的 gap 汇总——语义同为 exit 1，T5 若要 gap 明细可考虑先读 partial 输出再退。

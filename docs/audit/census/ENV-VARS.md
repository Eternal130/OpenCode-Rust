# ENV-VARS Census（缺口普查 · 通道 3/∞）

> 双侧进程边界环境变量全集机械提取（字面量）；census 非门禁，exit 0。

> 生成命令：`python3 tools/census/env_vars.py --write docs/audit/census/ENV-VARS.md`
> 权威源：rust `crates/`（strip_tests 后的 var/var_os/env!/set_var/remove_var 字面量）vs ref `packages/`（process.env.X / Bun.env.X / process.env["X"]，测试与脚本目录排除）。

## 归一化与范围声明

- 名称按 **大写** 归一比对；原始拼写差异以 CASE_DIFF 标注（不静默合并语义）。
- ref 排除 node_modules、/test/、/e2e/、__tests__、/dist/、/scripts/、/script/、*.test.*、*.d.ts、*.config.*（构建配置）；rust 剥 `#[cfg(test)]`/`#[test]` 区、跳过 *.gen.rs 与 /tests//benches//examples/ 集成测试目录与 tests.rs 模块文件。
- rust `env!`/`option_env!` 编译期字面计入读取面（标注 compile）；`set_var`/`remove_var` 计为写（标注）。
- 动态读取不可枚举：rust `envs()/vars()` 6 处、ref `process.env[expr]` 10 处——以计数声明，不进差异清单。
- **BOTH 但用途不同**的变量不做行为判定（超范围；行为对拍属对应功能通道）。
- CI/构建期变量（rust `env!("CARGO_PKG_*")` 族、ref scripts 目录）多数非运行时旋钮，保留在清单中并以 compile/scripts 标注辅助判读。

## Census

- rust 读取面：120 个字面名（另动态 6 处）；ref 读取面：135 个字面名（另动态 10 处）
- **BOTH：76** · REF_ONLY：**59** · RUST_ONLY：**44**（全集 179，逐项归属，覆盖率 100%）

### REF_ONLY（ref 读取、rust 不读 → 缺口候选）

- 🔴 `AICORE_DEPLOYMENT_ID` — ref[core]: read /opt/opencode-ref/packages/core/src/plugin/provider/sap-ai-core.ts:26
  - 裁定【注册表族·待集成通道】：同上（sap-ai-core.ts:26）。
- 🔴 `AICORE_RESOURCE_GROUP` — ref[core]: read /opt/opencode-ref/packages/core/src/plugin/provider/sap-ai-core.ts:26
  - 裁定【注册表族·待集成通道】：同上（sap-ai-core.ts:26）。
- 🔴 `AICORE_SERVICE_KEY` — ref[core]: read /opt/opencode-ref/packages/core/src/plugin/provider/sap-ai-core.ts:16; read /opt/opencode-ref/packages/core/src/plugin/provider/sap-ai-core.ts:18; write /opt/opencode-ref/packages/core/src/plugin/provider/sap-ai-core.ts:18
  - 裁定【注册表族·待集成通道】：ref `core/src/plugin/provider/sap-ai-core.ts:16-18` 写读兼有；rust 经 integration/provider_catalog 注册表读取（Method::Env 表已比对，此名不在表内）——归 provider/integration 通道裁定。
- 🔴 `AZURE_COGNITIVE_SERVICES_RESOURCE_NAME` — ref[core]: read /opt/opencode-ref/packages/core/src/plugin/provider/azure.ts:476
  - 裁定【注册表族·待集成通道】：ref `core/src/plugin/provider/azure.ts:476`；rust `provider_catalog/azure_sync.rs:66` 有同名常量 AZURE_LEGACY_RESOURCE_NAME_ENV、`integration/providers_enterprise.rs:444` 有名表——提取器未并表，归集成通道复核。
- 🔴 `CLOUDFLARE_ACCOUNT_ID` — ref[core]: read /opt/opencode-ref/packages/core/src/plugin/provider/cloudflare-ai-gateway.ts:29; read /opt/opencode-ref/packages/core/src/plugin/provider/cloudflare-workers-ai.ts:53
  - 裁定【注册表族·待集成通道】：ref cloudflare-ai-gateway.ts:29 / cloudflare-workers-ai.ts:53；rust 门面注释（facade.rs:1242）提及经凭据链而非裸 env——归集成通道。
- 🔴 `CLOUDFLARE_GATEWAY_ID` — ref[core]: read /opt/opencode-ref/packages/core/src/plugin/provider/cloudflare-ai-gateway.ts:30
  - 裁定【注册表族·待集成通道】：ref cloudflare-ai-gateway.ts:30；同上。
- 🔴 `GITLAB_TOKEN` — ref[core]: read /opt/opencode-ref/packages/core/src/plugin/provider/gitlab.ts:304
  - 裁定【注册表族·待集成通道】：ref core/src/plugin/provider/gitlab.ts:304；rust integration 表无此名。
- 🔴 `OPENCODE_DIRECT_TRACE` — ref[cli]: read /opt/opencode-ref/packages/cli/src/mini-host.ts:50
  - 裁定【演示/诊断类·低优先】：ref cli/src/mini-host.ts:50 mini 宿主直连 trace；rust mini 通道待定。
- 🔴 `OPENCODE_DISABLE_FILEWATCHER` — ref[cli]: read /opt/opencode-ref/packages/cli/src/server-process.ts:124
  - 裁定【已文档化缺口】：同上（同名旧拼写，ref server-process.ts:124 双名兼容）。
- 🔴 `OPENCODE_DRIVE` — ref[tui]: read /opt/opencode-ref/packages/tui/src/app.tsx:253
  - GAPS-AND-PITFALLS.md 提及行：[1309]
  - 裁定【有据分歧】：ref tui/src/app.tsx:253 drive 集成；GAPS:1300 段提及。
- 🔴 `OPENCODE_FILEWATCHER_DISABLE` — ref[cli]: read /opt/opencode-ref/packages/cli/src/server-process.ts:124
  - 裁定【已文档化缺口】：rust `crates/oc-server/src/server_process_env.rs:22-24` 注释明示 "Not claimable in this port … flagged, not faked"（v2 `Watcher.configured`）；ref `packages/cli/src/server-process.ts:124`。
- 🔴 `OPENCODE_NODE_ASSETS_DIR` — ref[tui]: read /opt/opencode-ref/packages/tui/src/attention-sounds.node.ts:7; read /opt/opencode-ref/packages/tui/src/attention-sounds.node.ts:8
  - GAPS-AND-PITFALLS.md 提及行：[1309, 2732]
  - 裁定【生态资产类·不适用】：node 资产目录（tui/attention-sounds.node.ts:7-8）；rust 资产经 OPENCODE_APP_ASSETS（rust-only）。
- 🔴 `OPENCODE_NODE_PTY_PATH` — ref[core]: read /opt/opencode-ref/packages/core/src/pty/pty.node.ts:8
  - 裁定【生态资产类·不适用】：node 侧 pty 原生模块路径覆盖（core/src/pty/pty.node.ts:8）；rust 自带 oc-pty 进程，无 node 模块加载。
- 🔴 `OPENCODE_PARCEL_WATCHER_PATH` — ref[core]: read /opt/opencode-ref/packages/core/src/filesystem/watcher-binding.ts:11
  - 裁定【生态资产类·不适用】：node 文件监视原生模块（watcher-binding.ts:11）；rust 用原生 notify。
- 🔴 `OPENCODE_PHOTON_WASM_PATH` — ref[core]: read /opt/opencode-ref/packages/core/src/image/photon-wasm.node.ts:3
  - 裁定【生态资产类·不适用】：WASM 图像处理模块路径（image/photon-wasm.node.ts:3）；rust 原生图像路径。
- 🔴 `OPENCODE_ROUTE` — ref[tui]: read /opt/opencode-ref/packages/tui/src/app.tsx:348; read /opt/opencode-ref/packages/tui/src/app.tsx:349
  - GAPS-AND-PITFALLS.md 提及行：[1309]
  - 裁定【有据分歧】：ref tui/src/app.tsx:348-349 演示路由；GAPS-AND-PITFALLS.md:1300 段已提及同族。
- 🔴 `OPENCODE_SHOW_TTFD` — ref[cli]: read /opt/opencode-ref/packages/cli/src/mini-host.ts:164
  - 裁定【演示/诊断类·低优先】：ref cli/src/mini-host.ts:164 TTFD 显示；同上。
- 🔴 `OPENCODE_STORY` — ref[tui]: read /opt/opencode-ref/packages/tui/src/app.tsx:337; read /opt/opencode-ref/packages/tui/src/app.tsx:344
  - GAPS-AND-PITFALLS.md 提及行：[1309]
  - 裁定【有据分歧】：ref tui/src/app.tsx:337/344 storybook 演示模式；GAPS:1300 段提及。
- 🔴 `OPENCODE_TREE_SITTER_BASH_WASM_PATH` — ref[core]: read /opt/opencode-ref/packages/core/src/shell/parser-wasm.node.ts:7
  - 裁定【生态资产类·不适用】：同上（:7）。
- 🔴 `OPENCODE_TREE_SITTER_POWERSHELL_WASM_PATH` — ref[core]: read /opt/opencode-ref/packages/core/src/shell/parser-wasm.node.ts:9
  - 裁定【生态资产类·不适用】：同上（:9）。
- 🔴 `OPENCODE_TREE_SITTER_WASM_PATH` — ref[core]: read /opt/opencode-ref/packages/core/src/shell/parser-wasm.node.ts:6
  - 裁定【生态资产类·不适用】：WASM 语法分析模块（shell/parser-wasm.node.ts:6）；rust 内置 tree-sitter。
- 🔴 `PATHEXT` — ref[core]: read /opt/opencode-ref/packages/core/src/util/which.ts:10
  - GAPS-AND-PITFALLS.md 提及行：[632]
  - 裁定【半缺口候选】：ref `packages/core/src/util/which.ts:10` Windows 可执行扩展解析；rust `oc-core/src/environment/local.rs:23` 注释提及 PATHEXT 解析但 census 未见生产读取（GAPS:623 提及）——windows which 通道复核。
- 🔴 `SNOWFLAKE_ACCOUNT` — ref[core]: read /opt/opencode-ref/packages/core/src/plugin/provider/snowflake-cortex.ts:176
  - 裁定【注册表族·待集成通道】：ref core/src/plugin/provider/snowflake-cortex.ts:176；rust providers_enterprise.rs 只列 SNOWFLAKE_CORTEX_TOKEN/PAT。

#### REF_ONLY·生态胶水（仅 ref 独有包读取：desktop/app/session-ui/enterprise/stats/…，36 项）

- ⚪ `AUTH_FRONTEND_URL` — ref[console]（ref 独有包，非本移植进程边界）
- ⚪ `DATABASE_URL` — ref[stats]（ref 独有包，非本移植进程边界）
- ⚪ `DISPLAY` — ref[gui-extensions]（ref 独有包，非本移植进程边界）
- ⚪ `ELECTRON_RENDERER_URL` — ref[desktop]（ref 独有包，非本移植进程边界）
- ⚪ `HIGHLIGHT_BUNDLE` — ref[session-ui]（ref 独有包，非本移植进程边界）
- ⚪ `HIGHLIGHT_PORT` — ref[session-ui]（ref 独有包，非本移植进程边界）
- ⚪ `HIGHLIGHT_RETENTION` — ref[session-ui]（ref 独有包，非本移植进程边界）
- ⚪ `HIGHLIGHT_SCREENSHOTS` — ref[session-ui]（ref 独有包，非本移植进程边界）
- ⚪ `MARKDOWN_ASSERT_DISPOSAL` — ref[session-ui]（ref 独有包，非本移植进程边界）
- ⚪ `MARKDOWN_BUILD_DIR` — ref[session-ui]（ref 独有包，非本移植进程边界）
- ⚪ `MARKDOWN_RETAINED` — ref[session-ui]（ref 独有包，非本移植进程边界）
- ⚪ `MARKDOWN_REVISION` — ref[session-ui]（ref 独有包，非本移植进程边界）
- ⚪ `MARKDOWN_SCREENSHOT` — ref[session-ui]（ref 独有包，非本移植进程边界）
- ⚪ `NODE_ENV` — ref[enterprise]（ref 独有包，非本移植进程边界）
- ⚪ `OPENCODE_DESKTOP_CLI_DEV` — ref[desktop]（ref 独有包，非本移植进程边界）
- ⚪ `OPENCODE_DESKTOP_DISABLE_PROTOCOL_REGISTRATION` — ref[desktop]（ref 独有包，非本移植进程边界）
- ⚪ `OPENCODE_DESKTOP_ISOLATED_SERVER` — ref[desktop]（ref 独有包，非本移植进程边界）
- ⚪ `OPENCODE_DESKTOP_REMOTE_DEBUGGING_PORT` — ref[desktop]（ref 独有包，非本移植进程边界）
- ⚪ `OPENCODE_DESKTOP_SERVER_CHANNEL` — ref[desktop]（ref 独有包，非本移植进程边界）
- ⚪ `OPENCODE_DESKTOP_TEST_ROOT` — ref[desktop]（ref 独有包，非本移植进程边界）
- ⚪ `OPENCODE_DESKTOP_WSL_CLI_BUILD` — ref[gui-extensions]（ref 独有包，非本移植进程边界）
- ⚪ `OPENCODE_DESKTOP_WSL_CLI_OUTPUT` — ref[gui-extensions]（ref 独有包，非本移植进程边界）
- ⚪ `OPENCODE_STORAGE_ACCESS_KEY_ID` — ref[enterprise]（ref 独有包，非本移植进程边界）
- ⚪ `OPENCODE_STORAGE_ACCOUNT_ID` — ref[enterprise]（ref 独有包，非本移植进程边界）
- ⚪ `OPENCODE_STORAGE_ADAPTER` — ref[enterprise]（ref 独有包，非本移植进程边界）
- ⚪ `OPENCODE_STORAGE_BUCKET` — ref[enterprise]（ref 独有包，非本移植进程边界）
- ⚪ `OPENCODE_STORAGE_REGION` — ref[enterprise]（ref 独有包，非本移植进程边界）
- ⚪ `OPENCODE_STORAGE_SECRET_ACCESS_KEY` — ref[enterprise]（ref 独有包，非本移植进程边界）
- ⚪ `OPENCODE_TEST_ONBOARDING` — ref[desktop]（ref 独有包，非本移植进程边界）
- ⚪ `OPENCODE_VERSION` — ref[desktop]（ref 独有包，非本移植进程边界）
- ⚪ `PLANETSCALE_DATABASE` — ref[stats]（ref 独有包，非本移植进程边界）
- ⚪ `PLANETSCALE_HOST` — ref[stats]（ref 独有包，非本移植进程边界）
- ⚪ `PLANETSCALE_PASSWORD` — ref[stats]（ref 独有包，非本移植进程边界）
- ⚪ `PLANETSCALE_USERNAME` — ref[stats]（ref 独有包，非本移植进程边界）
- ⚪ `PLAYWRIGHT_STORYBOOK_PORT` — ref[storybook]（ref 独有包，非本移植进程边界）
- ⚪ `PLAYWRIGHT_STORYBOOK_URL` — ref[storybook]（ref 独有包，非本移植进程边界）

### RUST_ONLY（rust 读取、ref 不读 → rust 额外旋钮）

- 🔗 `AWS_ACCESS_KEY_ID` — rust: indirect crates/oc-ai/src/protocols/bedrock/sigv4.rs:102
- 🔗 `AWS_SECRET_ACCESS_KEY` — rust: indirect crates/oc-ai/src/protocols/bedrock/sigv4.rs:104
- 🔗 `AWS_SESSION_TOKEN` — rust: indirect crates/oc-ai/src/protocols/bedrock/sigv4.rs:109
- 🔗 `CARGO_MANIFEST_DIR` — rust: compile crates/oc-ai/src/test_support.rs:29; compile crates/oc-cli/build.rs:26
- 🔗 `CARGO_PKG_VERSION` — rust: compile crates/oc-cli/src/cli.rs:22; compile crates/oc-cli/src/util.rs:8
- 🔗 `CLOUDFLARE_API_KEY` — rust: table crates/oc-core/src/integration/providers.rs:73
- 🔗 `CLOUDFLARE_API_TOKEN` — rust: table crates/oc-core/src/integration/providers.rs:73
  - 有据分歧：GAPS-AND-PITFALLS.md 提及行 [2389]
- 🔗 `CLOUDFLARE_WORKERS_AI_TOKEN` — rust: table crates/oc-core/src/integration/providers.rs:73
- 🔗 `GOOGLE_APPLICATION_CREDENTIALS` — rust: read crates/oc-core/src/integration/vertex.rs:34
- 🔗 `HERDR_ENV` — rust: read crates/oc-tui/src/config.rs:128; read crates/oc-tui/src/config.rs:331
- 🔗 `NODE_EXTRA_CA_CERTS` — rust: indirect crates/oc-http/src/lib.rs:0
  - 有据分歧：GAPS-AND-PITFALLS.md 提及行 [1929, 1930, 1933, 2083, 2253]
- 🔗 `OC_BUILD_GIT_HASH` — rust: compile crates/oc-cli/src/main.rs:53
- 🔗 `OC_BUILD_PROFILE` — rust: compile crates/oc-cli/src/main.rs:54
- 🔗 `OC_BUILD_STAMP` — rust: compile crates/oc-server/src/lifecycle.rs:35
- 🔗 `OC_FRAME_BUDGET_SCALE` — rust: read crates/oc-tui/src/frame_budget_tests.rs:52
- 🔗 `OC_PLUGIN_BUN` — rust: indirect crates/oc-plugin-host/src/bun.rs:0; indirect crates/oc-util/src/bunman.rs:0
  - 有据分歧：GAPS-AND-PITFALLS.md 提及行 [2233, 2799, 2811]
- 🔗 `OC_PLUGIN_NODE` — rust: indirect crates/oc-plugin-host/src/bun.rs:0
  - 有据分歧：GAPS-AND-PITFALLS.md 提及行 [2233, 2799, 2804]
- 🔗 `OC_PTY_DAEMON` — rust: read crates/oc-core/src/persistent_pty/frontend.rs:54
- 🔗 `OC_RELEASE_TAG` — rust: read crates/oc-cli/build.rs:18; read crates/oc-server/build.rs:67; compile crates/oc-cli/src/services/updater.rs:46
  - 有据分歧：GAPS-AND-PITFALLS.md 提及行 [2279]
- 🔗 `OC_TUI_DEBUG` — rust: read crates/oc-tui/src/app.rs:1347; read crates/oc-tui/src/app.rs:1442
- 🔗 `OC_TUI_TEST_RENDER_PANIC` — rust: read crates/oc-tui/src/app.rs:3424
- 🔗 `OPENCODE_ACP_DEBUG` — rust: read crates/oc-cli/src/acp/service.rs:335; read crates/oc-cli/src/acp/transport.rs:65
- 🔗 `OPENCODE_APP_ARCHIVE` — rust: read crates/oc-server/build.rs:42
- 🔗 `OPENCODE_APP_ASSETS` — rust: read crates/oc-server/src/app_archive.rs:354
- 🔗 `OPENCODE_CATALOG_FETCH` — rust: read crates/oc-server/src/boot.rs:347; read crates/oc-server/src/boot.rs:648
- 🔗 `OPENCODE_DEBUG_ERRORS` — rust: read crates/oc-server/src/errors.rs:706
- 🔗 `OPENCODE_DISABLE_PLUGINS` — rust: read crates/oc-server/src/plugins.rs:340
- 🔗 `OPENCODE_INSTALLER_URL` — rust: indirect crates/oc-cli/src/services/updater.rs:0
  - 有据分歧：GAPS-AND-PITFALLS.md 提及行 [767]
- 🔗 `OPENCODE_LOCAL_DISCOVERY` — rust: read crates/oc-server/src/boot.rs:642
- 🔗 `OPENCODE_LOG_FILE` — rust: read crates/oc-server/src/boot.rs:1093; read crates/oc-util/src/logging.rs:120; write crates/oc-server/src/boot.rs:1094
  - 有据分歧：GAPS-AND-PITFALLS.md 提及行 [2703]
- 🔗 `OPENCODE_MODELS_DEV_URL` — rust: indirect crates/oc-server/src/server_process_env.rs:76
- 🔗 `OPENCODE_NO_TTY_PROMPT` — rust: read crates/oc-cli/src/run/noninteractive.rs:400
- 🔗 `OPENCODE_PACKAGE` — rust: indirect crates/oc-cli/src/services/updater.rs:0
- 🔗 `OPENCODE_RELEASE_URL` — rust: indirect crates/oc-cli/src/services/updater.rs:0
- 🔗 `OPENCODE_RG_PATH` — rust: indirect crates/oc-core/src/tool/ripgrep_runner.rs:0
- 🔗 `OPENCODE_TRACE_BOOT` — rust: read crates/oc-server/src/boot.rs:250
- 🔗 `OPENTUI_NOTIFICATIONS` — rust: read crates/oc-tui/src/os_notify.rs:86
- 🔗 `OPENTUI_NOTIFICATION_PROTOCOL` — rust: read crates/oc-tui/src/os_notify.rs:161
- 🔗 `OUT_DIR` — rust: read crates/oc-server/build.rs:39; compile crates/oc-server/src/app_archive.rs:44
- 🔗 `PARITY_UPDATE` — rust: read crates/oc-tui/src/components/chrome_golden_tests.rs:65; read crates/oc-tui/src/components/footer/golden_tests.rs:59
- 🔗 `PROFILE` — rust: read crates/oc-cli/build.rs:38
- 🔗 `TZ` — rust: read crates/oc-cli/src/handlers/session.rs:123; read crates/oc-cli/src/handlers/stats.rs:137
  - 有据分歧：GAPS-AND-PITFALLS.md 提及行 [999]
- 🔗 `USERPROFILE` — rust: read crates/oc-core/src/filesystem/protected.rs:65
- 🔗 `WT_SESSION` — rust: read crates/oc-tui/src/render/color_mode.rs:97

### BOTH（双方读取；用途异同不在本通道判定）

- ✅ `AWS_BEARER_TOKEN_BEDROCK` — rust: read crates/oc-ai/src/protocols/bedrock/mantle_messages.rs:207; indirect crates/oc-ai/src/protocols/bedrock/sigv4.rs:204; table crates/oc-core/src/integration/providers.rs:82 | ref: read /opt/opencode-ref/packages/ai/src/protocols/utils/bedrock-auth.ts:140; read /opt/opencode-ref/packages/ai/src/providers/amazon-bedrock-mantle.ts:111
- ✅ `AWS_DEFAULT_REGION` — rust: read crates/oc-ai/src/protocols/bedrock/sigv4.rs:94 | ref: read /opt/opencode-ref/packages/ai/src/protocols/utils/bedrock-auth.ts:119; read /opt/opencode-ref/packages/core/src/plugin/provider/amazon-bedrock.ts:42
- ✅ `AWS_REGION` — rust: read crates/oc-ai/src/protocols/bedrock/sigv4.rs:93 | ref: read /opt/opencode-ref/packages/ai/src/protocols/utils/bedrock-auth.ts:119; read /opt/opencode-ref/packages/core/src/plugin/provider/amazon-bedrock.ts:42
- ✅ `AZURE_RESOURCE_NAME` — rust: read crates/oc-core/src/provider_catalog/facade.rs:1483 | ref: read /opt/opencode-ref/packages/core/src/plugin/provider/azure.ts:476
- ✅ `CI` — rust: read crates/oc-cli/src/run/noninteractive.rs:397 | ref: read /opt/opencode-ref/packages/http-recorder/src/replay/state.ts:6; read /opt/opencode-ref/packages/storybook/playwright/config.ts:13
- ✅ `CLAUDE_CODE_SSE_PORT` — rust: read crates/oc-tui/src/editor_ide.rs:160 | ref: read /opt/opencode-ref/packages/tui/src/context/editor.ts:117
- ✅ `COLORFGBG` — rust: read crates/oc-cli/src/handlers/stats.rs:305 | ref: read /opt/opencode-ref/packages/cli/src/commands/handlers/stats.ts:331
- ✅ `COLORTERM` — rust: read crates/oc-core/src/engine/skills.rs:203; read crates/oc-tui/src/render/color_mode.rs:95 | ref: read /opt/opencode-ref/packages/core/src/plugin/skill.ts:84; read /opt/opencode-ref/packages/core/src/plugin/skill.ts:84
- ✅ `COMSPEC` — rust: read crates/oc-core/src/engine/skills.rs:221; read crates/oc-core/src/shell/select.rs:192 | ref: read /opt/opencode-ref/packages/core/src/plugin/skill.ts:95; read /opt/opencode-ref/packages/core/src/shell/select.ts:102
- ✅ `EDITOR` — rust: read crates/oc-tui/src/editor.rs:45 | ref: read /opt/opencode-ref/packages/tui/src/editor.ts:15
- ✅ `GCLOUD_PROJECT` — rust: indirect crates/oc-ai/src/protocols/vertex.rs:52 | ref: read /opt/opencode-ref/packages/ai/src/providers/google-vertex-shared.ts:22; read /opt/opencode-ref/packages/core/src/plugin/provider/google-vertex.ts:13
- ✅ `GCP_PROJECT` — rust: indirect crates/oc-ai/src/protocols/vertex.rs:51 | ref: read /opt/opencode-ref/packages/ai/src/providers/google-vertex-shared.ts:21; read /opt/opencode-ref/packages/core/src/plugin/provider/google-vertex.ts:12
- ✅ `GITLAB_INSTANCE_URL` — rust: read crates/oc-core/src/integration/providers_enterprise.rs:91; read crates/oc-core/src/integration/providers_enterprise.rs:127 | ref: read /opt/opencode-ref/packages/core/src/plugin/provider/gitlab.ts:303; read /opt/opencode-ref/packages/core/src/plugin/provider/gitlab.ts:351
- ✅ `GITLAB_OAUTH_CLIENT_ID` — rust: read crates/oc-core/src/integration/providers_enterprise.rs:168; read crates/oc-core/src/integration/providers_enterprise.rs:241 | ref: read /opt/opencode-ref/packages/core/src/plugin/provider/gitlab.ts:151; read /opt/opencode-ref/packages/core/src/plugin/provider/gitlab.ts:168
- ✅ `GOOGLE_CLOUD_LOCATION` — rust: indirect crates/oc-ai/src/protocols/vertex.rs:63 | ref: read /opt/opencode-ref/packages/ai/src/providers/google-vertex-shared.ts:27; read /opt/opencode-ref/packages/core/src/plugin/provider/google-vertex.ts:21
- ✅ `GOOGLE_CLOUD_PROJECT` — rust: indirect crates/oc-ai/src/protocols/vertex.rs:50 | ref: read /opt/opencode-ref/packages/ai/src/providers/google-vertex-shared.ts:20; read /opt/opencode-ref/packages/core/src/plugin/provider/google-vertex.ts:11
- ✅ `GOOGLE_VERTEX_API_KEY` — rust: table crates/oc-core/src/integration/providers.rs:72 | ref: read /opt/opencode-ref/packages/ai/src/providers/google-vertex-shared.ts:53
- ✅ `GOOGLE_VERTEX_LOCATION` — rust: indirect crates/oc-ai/src/protocols/vertex.rs:62 | ref: read /opt/opencode-ref/packages/ai/src/providers/google-vertex-shared.ts:26; read /opt/opencode-ref/packages/core/src/plugin/provider/google-vertex.ts:20
- ✅ `GOOGLE_VERTEX_PROJECT` — rust: indirect crates/oc-ai/src/protocols/vertex.rs:49 | ref: read /opt/opencode-ref/packages/ai/src/providers/google-vertex-shared.ts:19; read /opt/opencode-ref/packages/core/src/plugin/provider/google-vertex.ts:10
- ✅ `HOME` — rust: read crates/oc-core/src/config/variable.rs:142; read crates/oc-core/src/filesystem/protected.rs:67 | ref: read /opt/opencode-ref/packages/desktop/src/main/files/apps.ts:25
- ✅ `MODAL_PROXY_TOKEN` — rust: indirect crates/oc-core/src/engine/mod.rs:0 | ref: read /opt/opencode-ref/packages/core/src/plugin/provider/modal.ts:30
- ✅ `NO_COLOR` — rust: read crates/oc-cli/src/handlers/stats.rs:109 | ref: read /opt/opencode-ref/packages/cli/src/commands/handlers/stats.ts:55
- ✅ `OPENCODE_API_KEY` — rust: read crates/oc-core/src/engine/resolver.rs:591 | ref: read /opt/opencode-ref/packages/core/src/plugin/provider/opencode.ts:319; read /opt/opencode-ref/packages/core/src/plugin/provider/opencode.ts:331
- ✅ `OPENCODE_CHANNEL` — rust: read crates/oc-server/src/observability.rs:139; read crates/oc-tui/src/storage.rs:74; compile crates/oc-core/src/engine/skills.rs:105 | ref: read /opt/opencode-ref/packages/app/vite.js:46; read /opt/opencode-ref/packages/app/vite.js:50
- ✅ `OPENCODE_CLIENT` — rust: read crates/oc-core/src/session/runner/model_request.rs:75; write crates/oc-cli/src/acp/mod.rs:114 | ref: read /opt/opencode-ref/packages/cli/src/commands/handlers/default.ts:98; read /opt/opencode-ref/packages/cli/src/index.ts:129; write /opt/opencode-ref/packages/cli/src/commands/handlers/acp.ts:16
- ✅ `OPENCODE_CLI_CONFIG_CONTENT` — rust: read crates/oc-core/src/cli_config/mod.rs:472 | ref: read /opt/opencode-ref/packages/cli/src/config/config.ts:32; read /opt/opencode-ref/packages/cli/src/config/config.ts:33
- ✅ `OPENCODE_CONFIG` — rust: read crates/oc-core/src/config.rs:438; read crates/oc-tui/src/plugin_boot.rs:162 | ref: read /opt/opencode-ref/packages/cli/src/server-process.ts:117
- ✅ `OPENCODE_CONFIG_CONTENT` — rust: read crates/oc-core/src/config.rs:442 | ref: read /opt/opencode-ref/packages/cli/src/server-process.ts:118
- ✅ `OPENCODE_CONFIG_DIR` — rust: indirect crates/oc-util/src/global_roots.rs:98 | ref: read /opt/opencode-ref/packages/cli/src/index.ts:120; read /opt/opencode-ref/packages/cli/src/index.ts:120
- ✅ `OPENCODE_CONFIG_PROJECT_DISABLE` — rust: read crates/oc-core/src/config.rs:398; indirect crates/oc-server/src/server_process_env.rs:87 | ref: read /opt/opencode-ref/packages/cli/src/server-process.ts:115
- ✅ `OPENCODE_DB` — rust: read crates/oc-core/src/database/path.rs:67; read crates/oc-server/src/boot.rs:596; indirect crates/oc-util/src/global_roots.rs:146 | ref: read /opt/opencode-ref/packages/cli/src/database-path.ts:6; read /opt/opencode-ref/packages/core/src/database/v1-migration.bun.ts:704; write /opt/opencode-ref/packages/desktop/src/main/lifecycle/configure.ts:66
- ✅ `OPENCODE_DISABLE_AUTOUPDATE` — rust: indirect crates/oc-cli/src/services/autoupdate.rs:121 | ref: read /opt/opencode-ref/packages/cli/src/services/updater.ts:456
- ✅ `OPENCODE_DISABLE_CHANNEL_DB` — rust: read crates/oc-core/src/database/path.rs:70 | ref: read /opt/opencode-ref/packages/cli/src/database-path.ts:8; read /opt/opencode-ref/packages/cli/src/database-path.ts:9
- ✅ `OPENCODE_DISABLE_FFF` — rust: read crates/oc-server/src/server_process_env.rs:159 | ref: read /opt/opencode-ref/packages/cli/src/server-process.ts:126; read /opt/opencode-ref/packages/cli/src/server-process.ts:128
- ✅ `OPENCODE_DISABLE_MODELS_FETCH` — rust: indirect crates/oc-server/src/server_process_env.rs:78 | ref: read /opt/opencode-ref/packages/cli/src/server-process.ts:110
- ✅ `OPENCODE_DISABLE_PROJECT_CONFIG` — rust: read crates/oc-core/src/config.rs:399; indirect crates/oc-server/src/server_process_env.rs:87 | ref: read /opt/opencode-ref/packages/cli/src/server-process.ts:115
- ✅ `OPENCODE_EDITOR_SSE_PORT` — rust: read crates/oc-tui/src/editor_ide.rs:161 | ref: read /opt/opencode-ref/packages/tui/src/context/editor.ts:117
- ✅ `OPENCODE_FAST_BOOT` — rust: read crates/oc-tui/src/app.rs:8270 | ref: read /opt/opencode-ref/packages/tui/src/app.tsx:351
- ✅ `OPENCODE_GIT_BASH_PATH` — rust: read crates/oc-core/src/shell/select.rs:172 | ref: read /opt/opencode-ref/packages/cli/src/server-process.ts:121; read /opt/opencode-ref/packages/cli/src/services/updater.ts:186
- ✅ `OPENCODE_LOG_LEVEL` — rust: read crates/oc-ai/src/protocols/anthropic_messages.rs:638; read crates/oc-util/src/logging.rs:54 | ref: read /opt/opencode-ref/packages/util/src/observability/logging.ts:165
- ✅ `OPENCODE_MODELS_PATH` — rust: indirect crates/oc-server/src/server_process_env.rs:77 | ref: read /opt/opencode-ref/packages/cli/src/server-process.ts:109
- ✅ `OPENCODE_MODELS_URL` — rust: indirect crates/oc-server/src/server_process_env.rs:76 | ref: read /opt/opencode-ref/packages/cli/src/server-process.ts:108
- ✅ `OPENCODE_PASSWORD` — rust: read crates/oc-cli/src/util.rs:22; read crates/oc-server/src/main.rs:100; write crates/oc-cli/src/handlers/serve.rs:151 | ref: read /opt/opencode-ref/packages/cli/src/server-process.ts:79
- ✅ `OPENCODE_PRINT_LOGS` — rust: read crates/oc-ai/src/protocols/anthropic_messages.rs:645; read crates/oc-cli/src/services/standalone.rs:165; write crates/oc-cli/src/main.rs:45 | ref: read /opt/opencode-ref/packages/cli/src/services/standalone.ts:29; read /opt/opencode-ref/packages/util/src/observability/logging.ts:160; write /opt/opencode-ref/packages/cli/src/framework/runtime.ts:98
- ✅ `OPENCODE_PTY_BIN` — rust: read crates/oc-core/src/persistent_pty/transport.rs:268; read crates/oc-core/src/persistent_pty/transport.rs:277 | ref: read /opt/opencode-ref/packages/core/src/persistent-pty/binary.bun.ts:10; read /opt/opencode-ref/packages/core/src/persistent-pty/binary.bun.ts:10
- ✅ `OPENCODE_PTY_HANDOFF` — rust: read crates/oc-server/src/boot.rs:842; write crates/oc-server/src/boot.rs:843 | ref: read /opt/opencode-ref/packages/cli/src/server-process.ts:46; read /opt/opencode-ref/packages/cli/src/server-process.ts:47
- ✅ `OPENCODE_REPO_CLONE_GITHUB_BASE_URL` — rust: read crates/oc-core/src/reference/repository.rs:318 | ref: read /opt/opencode-ref/packages/core/src/repository.ts:166
- ✅ `OPENCODE_SERVER_PASSWORD` — rust: read crates/oc-cli/src/util.rs:23; write crates/oc-cli/src/handlers/serve.rs:152; write crates/oc-server/src/main.rs:105 | ref: read /opt/opencode-ref/packages/cli/src/server-process.ts:80
- ✅ `OPENCODE_SIMULATE` — rust: read crates/oc-server/src/server_process_env.rs:173 | ref: read /opt/opencode-ref/packages/cli/src/server-process.ts:103
- ✅ `OPENCODE_SSH_ASKPASS_PORT` — rust: read crates/oc-cli/src/main.rs:29; read crates/oc-cli/src/ssh_askpass.rs:17 | ref: read /opt/opencode-ref/packages/cli/src/index.ts:20; read /opt/opencode-ref/packages/cli/src/ssh-askpass.ts:10
- ✅ `OPENCODE_SSH_ASKPASS_TOKEN` — rust: read crates/oc-cli/src/ssh_askpass.rs:22 | ref: read /opt/opencode-ref/packages/cli/src/ssh-askpass.ts:24
- ✅ `OPENCODE_TEST_HOME` — rust: indirect crates/oc-util/src/global_roots.rs:74; write crates/oc-cli/src/services/updater.rs:1524 | ref: read /opt/opencode-ref/packages/util/src/global.ts:17
- ✅ `OPENCODE_TUI_CHANNEL` — rust: read crates/oc-tui/src/storage.rs:69 | ref: read /opt/opencode-ref/packages/cli/src/commands/handlers/default.ts:100
- ✅ `OPENCODE_ZED_DB` — rust: read crates/oc-tui/src/app.rs:4226 | ref: read /opt/opencode-ref/packages/tui/src/editor-zed.ts:189
- ✅ `OTEL_EXPORTER_OTLP_ENDPOINT` — rust: read crates/oc-server/src/observability.rs:138 | ref: read /opt/opencode-ref/packages/cli/src/index.ts:127; read /opt/opencode-ref/packages/util/src/observability.ts:22
- ✅ `OTEL_EXPORTER_OTLP_HEADERS` — rust: read crates/oc-server/src/observability.rs:144 | ref: read /opt/opencode-ref/packages/cli/src/index.ts:128; read /opt/opencode-ref/packages/util/src/observability.ts:23
- ✅ `OTEL_RESOURCE_ATTRIBUTES` — rust: read crates/oc-server/src/observability.rs:75 | ref: read /opt/opencode-ref/packages/util/src/observability/otlp.ts:30
- ✅ `PATH` — rust: read crates/oc-cli/src/services/updater.rs:1623; read crates/oc-core/src/formatter.rs:556; write crates/oc-cli/src/services/updater.rs:1624 | ref: read /opt/opencode-ref/packages/core/src/util/which.ts:5
- ✅ `PWD` — rust: read crates/oc-cli/src/mini.rs:239; read crates/oc-cli/src/run/mod.rs:446 | ref: read /opt/opencode-ref/packages/cli/src/mini.ts:202; read /opt/opencode-ref/packages/cli/src/run/run.ts:73
- ✅ `SHELL` — rust: read crates/oc-cli/src/services/updater.rs:1321; read crates/oc-core/src/engine/skills.rs:219 | ref: read /opt/opencode-ref/packages/cli/src/commands/handlers/uninstall.ts:160; read /opt/opencode-ref/packages/core/src/plugin/skill.ts:93
- ✅ `SNOWFLAKE_CORTEX_PAT` — rust: read crates/oc-core/src/integration/providers_enterprise.rs:931; read crates/oc-core/src/provider_catalog/facade.rs:731; table crates/oc-core/src/integration/providers_enterprise.rs:762 | ref: read /opt/opencode-ref/packages/core/src/plugin/provider/snowflake-cortex.ts:269
- ✅ `SNOWFLAKE_CORTEX_TOKEN` — rust: read crates/oc-core/src/integration/providers_enterprise.rs:930; read crates/oc-core/src/provider_catalog/facade.rs:730; table crates/oc-core/src/integration/providers_enterprise.rs:762 | ref: read /opt/opencode-ref/packages/core/src/plugin/provider/snowflake-cortex.ts:269
- ✅ `SSH_ASKPASS_PROMPT` — rust: read crates/oc-cli/src/ssh_askpass.rs:29 | ref: read /opt/opencode-ref/packages/cli/src/ssh-askpass.ts:26
- ✅ `STY` — rust: read crates/oc-tui/src/system.rs:53 | ref: read /opt/opencode-ref/packages/tui/src/util/system.ts:18
- ✅ `TERM` — rust: read crates/oc-core/src/engine/skills.rs:202; read crates/oc-tui/src/components/command_palette.rs:1691 | ref: read /opt/opencode-ref/packages/core/src/plugin/skill.ts:83; read /opt/opencode-ref/packages/core/src/plugin/skill.ts:83
- ✅ `TERM_PROGRAM` — rust: read crates/oc-core/src/engine/skills.rs:201; read crates/oc-tui/src/app.rs:4221 | ref: read /opt/opencode-ref/packages/core/src/plugin/skill.ts:82; read /opt/opencode-ref/packages/core/src/plugin/skill.ts:82
- ✅ `TERM_PROGRAM_VERSION` — rust: read crates/oc-tui/src/system.rs:47 | ref: read /opt/opencode-ref/packages/tui/src/util/system.ts:17; read /opt/opencode-ref/packages/tui/src/util/system.ts:17
- ✅ `TMUX` — rust: read crates/oc-tui/src/system.rs:51 | ref: read /opt/opencode-ref/packages/tui/src/util/system.ts:18
- ✅ `VERTEX_LOCATION` — rust: indirect crates/oc-ai/src/protocols/vertex.rs:64 | ref: read /opt/opencode-ref/packages/ai/src/providers/google-vertex-shared.ts:28; read /opt/opencode-ref/packages/core/src/plugin/provider/google-vertex.ts:22
- ✅ `VISUAL` — rust: read crates/oc-tui/src/editor.rs:42 | ref: read /opt/opencode-ref/packages/tui/src/editor.ts:15
- ✅ `XDG_CACHE_HOME` — rust: read crates/oc-core/src/engine/skills_url.rs:582; read crates/oc-plugin-host/src/context/install.rs:74; indirect crates/oc-util/src/global_roots.rs:97 | ref: read /opt/opencode-ref/packages/util/src/global-roots.ts:6; write /opt/opencode-ref/packages/desktop/src/main/lifecycle/configure.ts:69
- ✅ `XDG_CONFIG_HOME` — rust: read crates/oc-cli/src/services/updater.rs:1320; indirect crates/oc-util/src/global_roots.rs:99 | ref: read /opt/opencode-ref/packages/cli/src/commands/handlers/uninstall.ts:140; read /opt/opencode-ref/packages/util/src/global-roots.ts:7; write /opt/opencode-ref/packages/desktop/src/main/lifecycle/configure.ts:68
- ✅ `XDG_DATA_HOME` — rust: indirect crates/oc-util/src/global_roots.rs:96 | ref: read /opt/opencode-ref/packages/desktop/src/main/native/logging.ts:259; read /opt/opencode-ref/packages/util/src/global-roots.ts:5; write /opt/opencode-ref/packages/desktop/src/main/lifecycle/configure.ts:67
- ✅ `XDG_STATE_HOME` — rust: read crates/oc-server/src/lifecycle.rs:89; indirect crates/oc-util/src/global_roots.rs:100 | ref: read /opt/opencode-ref/packages/client/src/service-probe.ts:19; read /opt/opencode-ref/packages/desktop/src/main/lifecycle/environment.ts:26; write /opt/opencode-ref/packages/desktop/src/main/lifecycle/configure.ts:70
- ✅ `ZDOTDIR` — rust: read crates/oc-cli/src/services/updater.rs:1326 | ref: read /opt/opencode-ref/packages/cli/src/commands/handlers/uninstall.ts:141
- ✅ `ZED_TERM` — rust: read crates/oc-tui/src/app.rs:4220 | ref: read /opt/opencode-ref/packages/tui/src/context/editor.ts:121

## Census 裁定

- 已裁定 179/179（全集逐项归属，覆盖率 100%）。
- RUST_ONLY 44 项中 8 项有 GAPS-AND-PITFALLS.md 背书（有据分歧，如 OPENCODE_PTY_BIN）；其余为待登记 rust 旋钮。
- REF_ONLY 59 项中：共享包侧 23 项已逐项定性（裁定表：已文档化缺口/缺口候选/有据分歧/生态资产不适用/注册表族待集成通道）；ref 独有包 36 项为生态胶水（非本移植进程边界）。

## 提取不确定性

- 只提取字面量名；`envs()`/`vars()`/`process.env[expr]` 动态键不可枚举（计数见上）。
- rust 间接读取按三类机械捕获：包装函数（&str 参数直通 env::var/_os，如 non_empty/xdg/truthy）、常量承载（const NAME: &str + env::var(NAME)/wrapper(NAME)，含跨文件唯一常量）、integration `Method::Env` 表内 vec!["NAME"]（标 table）；更深的数据驱动注册表（如 models.dev 快照 env 列表）未并入——provider/integration 族差异归后续通道。
- ref `Bun.env.X` 仅出现在构建脚本（已排除）与 desktop（生态类）；ref provider 插件经字面量读取的 env（AICORE_*/CLOUDFLARE_* 等）在 rust 侧多经注册表/凭据链，直接字面比对会高估缺口（见裁定表'注册表族'）。
- 大小写归一可能合并同族拼写（CASE_DIFF 已标注原始拼写）。
- 环境变量命名规约过滤：仅保留 `[A-Z][A-Z0-9_]*`（进程边界两侧的惯例）；非常规拼写 rust 20 个 / ref 5 个被排除（主要为同名局部 helper/JSON 取键（如 `get("model")`）误触与 camelCase wire 键，非 env 读取）。
- rust `env!("CARGO_PKG_VERSION")` 等编译期常量与运行时 env 语义不同，已标 compile。

## 验证记录

- 差异项（REF_ONLY/RUST_ONLY 全部、BOTH 抽样）已亲读双侧当前代码核实（file:line 见上；旧文档仅作线索）。
- 幂等性：连跑两次输出经 `diff` 逐字节一致（/tmp/opencode/ 验证）。
- 确定性：文件排序遍历、集合输出前排序；无时间戳/随机源。
- exit 0（census 非门禁）。

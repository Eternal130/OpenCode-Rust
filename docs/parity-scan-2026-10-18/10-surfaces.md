# 10 · 整体表面覆盖清单（packages / services / sdks / 入口产物）

> 基线：opencode v2.0.24（`/opt/opencode-ref`，commit e7a34f09bf，只读） ↔ `/opt/opencode-rust`（workspace 2.0.24）。
> 方法：逐包读 `package.json`（name/description/bin/deps/scripts）↔ 逐 crate 读 `Cargo.toml` description，再对疑似缺失概念做全仓 grep 反查（§5），避免「没见过=没有」的误判。
> 范围裁定的权威依据：`ROADMAP.md:7-13`（scope ruling）与 `README.md`「Scope」段。

---

## 1. 包级映射总表

Rust workspace 成员（`/opt/opencode-rust/Cargo.toml` members，12 crate + 2 tools）：
`oc-util / oc-schema / oc-theme / oc-ai / oc-core / oc-syntax / oc-plugin-host / oc-server / oc-client / oc-tui / oc-tui-core / oc-cli` + `tools/openapi-gen`、`tools/parity`。

判定口径：**直接对应**（1:1 移植）｜**被合并**（并入某 crate）｜**部分对应**（生态位/形态不同）｜**间接覆盖**（构建期由参考源带出）｜**完全缺失**。

| 参考包 | name（package.json） | 角色 | Rust 对应 | 判定 |
|---|---|---|---|---|
| `packages/util` | @opencode/util | 基础工具库（fs/process/observability/npm/arborist…） | `oc-util`（XDG roots、retry、hash、wildcard、flock） | 直接对应（面收窄：observability/npm 相关子面未全量） |
| `packages/schema` | @opencode/schema | 共享 schema 类型（effect + standard-schema） | `oc-schema`（serde + schemars） | 直接对应 |
| `packages/protocol` | @opencode/protocol | Effect 化 API 协议层（api/client/errors/groups/middleware，`protocol/src/`） | 路由生成 `oc-server/src/routes.gen.rs` + `oc-client` + `tools/openapi-gen` | 被合并（重设计：Effect 中间件栈不移植，以 openapi 契约为锚） |
| `packages/theme` | @opencode/theme | TUI 主题 token/解析（@opentui/core 依赖） | `oc-theme`（63 主题 byte-equal，README「Architecture」表） | 直接对应 |
| `packages/ai` | @opencode/ai | LLM SDK（AWS/Google 凭证、事件流） | `oc-ai`（Anthropic/OpenAI chat+responses/Gemini/Mistral/Cohere/openai-compatible，274 cassettes） | 直接对应 |
| `packages/codemode` | @opencode/codemode「Effect-native confined code execution」 | 受限代码执行 | `oc-plugin-host`（`src/context/codemode_defaults.rs`、`tests/codemode.rs`） | 被合并 |
| `packages/plugin` | @opencode/plugin | 插件 API（含 `plugin/tui` 面向 TUI 的 feature-plugin 接口） | `oc-plugin-host` + `oc-tui/src/plugin_ui.rs`（UI-request 应用缝） | 直接对应（TUI feature-plugin 面部分，见 §3 latex/merman） |
| `packages/core` | @opencode/core | 引擎（db/bus/session/tools/permissions/pty） | `oc-core` | 直接对应（缺内建 browser 插件，见 §3） |
| `packages/server` | @opencode/server | HTTP server（141 路由/SSE/pty ws） | `oc-server` | 直接对应 |
| `packages/client` | @opencode/client | 由 openapi 生成的客户端 | `oc-client`（"generated Rust HTTP/SSE client SDK"） | 直接对应 |
| `packages/sdk` | @opencode/sdk（npm 名 `@opencode-ai/sdk`） | **JS/TS 用户 SDK**（含 workerd/effect 入口，`sdk/src/`） | `oc-client` 仅 Rust 形态 | **部分对应**——npm 生态位缺失（见 §3） |
| `packages/cli` | @opencode/cli，bin: `opencode→./bin/opencode.cjs`、`opencode2→./bin/opencode2.cjs` | CLI 入口（40 命令） | `oc-cli`（`src/cli.rs:88` 起 Commands；`README.md` 称 40/40） | 直接对应 |
| `packages/tui` | @opencode/tui（`src/index.tsx:1` `export { run }`） | TUI（opentui/solid 实现；由 CLI 以库方式拉起：`cli/src/commands/handlers/default.ts:3`） | `oc-tui` + `oc-tui-core`（ratatui/taffi + opentui-rust 移植层，ROADMAP M 系列记录） | 直接对应 |
| `packages/app` | @opencode/app | **Web UI**（Solid+Vite+PWA，被 `serve` 托管） | 不重写前端；release 流水线构建参考 app 并嵌入：`oc-server/src/app_archive.rs:1-13`、`oc-server/src/assets.rs`、`.github/workflows/release.yml:35`（web-assets job）、`tools/web-assets/package.ts` | 间接覆盖（ROADMAP R-04 裁决，`ROADMAP.md:47`） |
| `packages/session-ui` | @opencode/session-ui | Solid 会话 UI 组件（app/desktop/enterprise 共享） | 无自有实现；随 app 归档间接带出 | 间接覆盖 |
| `packages/ui` | @opencode/ui | Solid UI 基件（kobalte/pierre-diffs/katex/marked…） | 同上 | 间接覆盖 |
| `packages/gui-extensions` | @opencode/gui-extensions（private） | 桌面伴侣：SSH/WSL 远程 CLI 管理 | 无 | 完全缺失——**已裁定 out-of-scope**（`ROADMAP.md:12`：desktop companion, excluded with the desktop app） |
| `packages/desktop` | @opencode/desktop | **桌面壳，Electron**（`desktop/README.md:1` "built with Electron"；electron-builder/vite，`desktop/package.json` deps） | 无 | 完全缺失——**已裁定 out-of-scope**（`ROADMAP.md:8`；注：ROADMAP 写 "Tauri shell" 与实际 Electron 不符，但排除结论不变） |
| `packages/web` | @opencode/web | Astro 官网+文档（starlight） | 无 | 完全缺失——**已裁定 out-of-scope**（`ROADMAP.md:9`：not product functionality） |
| `packages/console/*` | @opencode/console-{app,core,function,mail,resource,support}（6 子包） | 托管 Console（opencode.ai/console：账号/SSO/计费/邮件） | 无 | 完全缺失（托管服务；其**客户端侧**集成已移植：`oc-core/src/integration/console.rs:1-14`） |
| `packages/stats/*` | @opencode/stats-{app,core,server}（`stats/README.md:1-9`） | 托管统计站（SolidStart + Lambda） | 无 | 完全缺失（托管服务）。注意与 CLI `stats` 命令区分：后者已移植 `oc-cli/src/handlers/stats.rs:1-3`（活动日历为已注册简化） |
| `packages/enterprise` | @opencode/enterprise（private，SolidStart+Cloudflare+hono） | 企业托管部署 | 无自有实现；**配置 schema 与流**已移植（`oc-schema/src/config/mod.rs:73-75,157-159` Enterprise；`oc-core/src/integration/tests.rs:688-757` enterprise 连接流） | 完全缺失（托管服务）；用户可感知面仅剩 config schema，已覆盖 |
| `packages/function` | @opencode/function（private，octokit/hono/jose，`src/{api,github}.ts`） | Cloudflare Workers GitHub App 认证 | 无 | 完全缺失（托管服务） |
| `packages/posts` | @opencode/posts（astro+mdx） | 官方博客 | 无 | 完全缺失（托管/营销） |
| `services/www` | @opencode/www | 官网 + V2 文档（Wrangler，`services/README.md:6`） | 无 | 完全缺失（托管服务） |
| `services/update` | @opencode/update | 版本元数据/更新选择服务 | 无服务端；**升级通道已换道** GitHub Releases（`oc-cli/src/services/update_feed.rs:18` FEED_URL=api.github.com/repos/Eternal130/OpenCode-Rust/releases；`updater.rs:16` 永不探活 opencode.ai） | 完全缺失（托管服务，已有注册替代通道） |
| `services/files` | @opencode/files | `/files/*` 公开文件分发（`services/README.md:7`） | 无 | 完全缺失（托管服务） |
| `packages/plugin-browser` | @opencode/plugin-browser「OpenCode's desktop browser plugin」 | **内建 agent `browser` 工具**（CDP 驱动浏览器） | 无（`oc-core/src/tool/` 18 个工具文件无 browser；见 §3） | **完全缺失——主程序功能缺口（P0）** |
| `packages/latex` | @opencode/latex | TUI 内 LaTeX 公式渲染插件 | 无（全仓 grep latex/katex 零命中） | **完全缺失——用户可见（P1）** |
| `packages/merman` | @opencode/merman | TUI 内 mermaid 图渲染插件（flowchart/gantt/sequence/state/gitgraph…） | 无（grep mermaid 零命中） | **完全缺失——用户可见（P1）** |
| `packages/simulation` | @opencode/simulation | 模拟后端/前端（录制回放测试架，`simulation/src/{backend,frontend,control-server.ts}`）；server 启动可注入（`server/src/routes.ts:151-156`）、TUI demo 模式（`tui/src/app.tsx:254`） | 无（仅 `oc-tui/tests/g11f1_proof.rs` 提及 simulation 一词，非移植） | 完全缺失（开发工具） |
| `packages/storybook` | @opencode/storybook | 组件 storybook；TUI 侧还有内建 storybook feature-plugin（`tui/src/plugin/builtins.ts:29`，仅 `OPENCODE_STORY` 时注册） | 无 | 完全缺失（开发工具；TUI storybook 因 env 门控，定为 P2） |
| `packages/httpapi-codegen` | @opencode/httpapi-codegen | effect client 代码生成器 | `tools/openapi-gen`（契约生成，非 client 代码生成） | 部分对应（开发工具） |
| `packages/http-recorder` | @opencode/http-recorder「deterministic cassettes」 | cassette 录制器 | 录制端无；回放端内置于 `oc-ai`（274 cassettes replay，README） | 部分对应（开发工具） |
| `packages/containers` | （无 name；`containers/README.md:1-9`：base/bun-node/rust/tauri-linux/publish 五镜像） | CI 容器镜像 | 无（Rust CI 直接跑 ubuntu-latest，`release.yml:37`） | 完全缺失（开发工具） |
| `packages/script` | @opencode/script（`script/src/index.ts:1-10`：bun 版本一致性检查助手） | 仓库脚本助手 | 无 | 完全缺失（开发工具） |
| `packages/effect-drizzle-sqlite` | （examples 目录，无 package.json name 输出） | effect-drizzle 集成示例 | 无 | 完全缺失（开发工具） |
| `packages/identity` | （无 package.json；`mark-*.png/svg` 品牌资产） | 品牌 mark 资产（desktop/web/console 引用） | 无 | 完全缺失（资产，P3；仅影响桌面/站点视觉，终端产品不引用） |
| `sdks/vscode` | name: `opencode`，「opencode for VS Code」 | VS Code 扩展（打开 opencode 终端/插入文件路径，`src/extension.ts:6-26`） | 无 | **完全缺失（用户可见生态，P1-P2）** |
| `github/` | opencode GitHub Action（`github/README.md:1-5`：`/opencode explain|fix|review` 评论集成，`action.yml`） | GitHub Action | 无 | **完全缺失（用户可见生态，P1-P2）** |
| 根 `install` | bash 安装器（`curl -fsSL https://opencode.ai/v2/install \| bash`） | curl 安装脚本 | `install/install.sh:1-13`（镜像 UX；三项注册分歧：二进制名 `opencode-rust`、URL 参数化不探活、--sha256 校验） | 直接对应 |
| 根 `package.json` scripts/workspaces | `workspaces: packages/*, services/*, packages/console/*, packages/stats/*`（`package.json:38-44`）；`dev:desktop/web/console/stats/www/storybook`（:14-19） | monorepo 编排 | cargo workspace 取代；dev:* 入口 N/A | 结构性对应 |

---

## 2. 完全缺失包清单（按影响分类）

### P0 — 主程序功能缺失（`opencode` 本体行为缺口）

| 缺失 | v2 证据 | 影响 |
|---|---|---|
| **browser 工具（plugin-browser）** | `packages/core/src/plugin/internal.ts:89` `import BrowserPlugin from "@opencode/plugin-browser"`（内建注册）；`packages/plugin-browser/src/tools.ts:36` 工具名 `"browser"` | v2 会话内可直接用 agent 浏览网页/操作页面；Rust `oc-core/src/tool/`（glob/grep/read/edit/write/shell/webfetch/websearch/patch/skill/subagent/question/codemode/…）无此工具。缺它属**能力缺口**而非生态缺口 |
| **TUI 内建 LaTeX / mermaid 渲染** | `packages/tui/src/plugin/builtins.ts:24-26`：`Stats, Merman, Latex` 在默认注册列表（storybook 才是 env 门控） | v2 TUI 转录区默认渲染数学公式（`latex/src/parser.ts` 等 14 文件）与 mermaid 图（`merman/src/{flowchart,gantt,sequence,state,gitgraph,timeline}`）；Rust 全仓 grep `latex|mermaid|katex` 零命中，回退纯文本 |

### P1 — 用户可见（产品之外、用户直接可用的表面）

| 缺失 | v2 证据 | 影响 |
|---|---|---|
| **npm 包 `@opencode-ai/sdk`（JS SDK）** | `packages/sdk`（deps 覆盖 client/core/plugin/server；`sdk/src/{opencode,tool,workerd}.ts`） | JS/TS 生态以 SDK 编程接入 opencode 的入口；Rust 仅有自用 `oc-client`。SDK 用户无法迁移 |
| **VS Code 扩展** | `sdks/vscode/src/extension.ts:6-26`（openNewTerminal / addFilepathToTerminal）；`.github/workflows/publish-vscode.yml`（vscode-v* tag 发布） | 编辑器集成面完全缺失 |
| **GitHub Action** | `github/README.md:1-5`、`github/action.yml`、`publish-github-action.yml`/`release-github-action.yml` | `/opencode` 评论式 CI 自动化缺失 |
| TUI storybook feature-plugin | `builtins.ts:29`（`OPENCODE_STORY` 门控） | 仅开发可见，严格说 P2；列此备查 |

### P2 — 开发工具（不影响交付物用户）

`packages/containers`、`packages/simulation`、`packages/storybook`、`packages/httpapi-codegen`（生成器半缺）、`packages/http-recorder`（录制半缺）、`packages/script`、`packages/effect-drizzle-sqlite`、`packages/identity`（资产）。

### P3 — 托管服务 / 营销（anomalyco 运营面，非产品功能）

`packages/console/*`（6 子包）、`packages/enterprise`、`packages/function`、`packages/stats/*`、`packages/posts`、`services/{www,update,files}`。
其中两个用户可感知侧翼已有 Rust 对应物：
- Console 连接（provider 目录/SSO 状态）：`oc-core/src/integration/console.rs:1-14`（`SsoRequired` 403 / `Unauthorized` 401 → connection.status）；
- 升级通道：`oc-cli/src/services/update_feed.rs:18` 指向本仓 GitHub Releases（v2 走 services/update + opencode.ai，属已注册换道）。

### 已裁定 out-of-scope（有明确书面依据，不算缺口）

`packages/desktop`、`packages/web`、`packages/gui-extensions` —— `ROADMAP.md:7-13`：
- OUT：`packages/desktop`（Tauri shell）；`packages/web`（Astro 营销/文档站——"not product functionality; migrate only on separate instruction"）；
- `packages/gui-extensions`（desktop companion，随桌面排除；"reassess case-by-case if it contains desktop-independent capability"）。
⚠️ **README 与 ROADMAP 表述冲突**：`README.md`「Scope」段写 "Not ported (out of scope by design): the desktop app, GUI extensions, **and the web UI**"，但 ROADMAP R-04（`ROADMAP.md:47,120-133`）明确把 Web UI 划为 IN 且已落地（web-assets job + `app_archive.rs`）。**README 该句已过时，建议修订**。

---

## 3. 逐项核查明细（防误判的 grep 证据）

- **console**：Rust 命中是「Console 客户端集成」（`oc-core/src/integration/console.rs`），不是托管 Console 站点本体 → `packages/console/*` 仍判缺失。
- **stats**：`oc-cli/src/handlers/stats.rs` 是 CLI `stats` 命令（v2 `cli/src/commands/handlers/stats.ts` 的移植，文件头自注"GitHub-style activity calendar is a REGISTERED simplification"）；`oc-tui/src/components/session.rs:1340-1343` 的 stats 是转录区统计行（wave C）。`packages/stats/*` 托管站仍判缺失。
- **enterprise**：`oc-schema/src/config/mod.rs:73-75`（`enterprise` 配置项）、`:157-159`（`Config.Info.enterprise`）、`oc-core/src/integration/tests.rs:688-757`（enterprise 连接流 e2e）→ **配置面已覆盖**，仅托管部署缺失。
- **codemode**：`oc-plugin-host/src/context/codemode_defaults.rs`、`oc-plugin-host/tests/codemode.rs`、`oc-core/src/tool/codemode.rs` → 已合并，非缺失。
- **simulation**：唯一命中 `oc-tui/tests/g11f1_proof.rs` 是注释措辞；`server/src/routes.ts:151-156` 的 simulation 注入缝无对应 → 判缺失（开发工具）。
- **desktop**：命中均为注释/客户端 UA（`oc-core/src/session/runner/tests_misc.rs:242-245` 设 `OPENCODE_CLIENT=desktop` 做测试；`oc-core/src/cli_config/mod.rs:26,467` 记录 desktop/mini 注入缝——该缝本身已移植）→ desktop 壳缺失符合裁定。
- **mini / acp**：曾列延后项，现已落地（`oc-cli/src/mini.rs`、`oc-cli/src/acp/`、third_party agent-client-protocol，`Cargo.toml` [patch.crates-io] 段）→ 不列缺口。
- **browser**：`grep -i browser` 命中全是 provider OAuth 的 `"browser"` method id（`oc-core/src/integration/providers.rs:608+`），与 browser 工具无关 → **确认工具缺失**。

---

## 4. 入口 / 产物 / 平台差异

| 维度 | v2 | Rust | 差异判定 |
|---|---|---|---|
| CLI 入口 | npm bin 双壳：`packages/cli/package.json` `bin: {opencode: ./bin/opencode.cjs, opencode2: ./bin/opencode2.cjs}`；`bin/opencode.cjs` 为 node spawn 包装（信号转发 win32 分支） | 单原生二进制 `target/release/opencode-rust`（README「Build」段） | 形态差异（注册）；**无 npm 产物**，见下行 |
| 分发渠道 | npm 包 + 原生二进制（`.github/workflows/publish.yml:11` 分支 `fix/npm-native-binary-install`）+ update 服务 + AUR（containers publish 镜像含 AUR tooling） | 仅 GitHub Release tarball + `install/install.sh`（:1-13 自注三项分歧：隔离名/URL 参数化/sha256） | **npm、AUR、Homebrew 等渠道全缺**（P1-P2，取决于用户获取方式） |
| 隐藏入口 | `cli/src/index.ts:20-23` `OPENCODE_SSH_ASKPASS_PORT` → ssh-askpass | `oc-cli/src/main.rs:21-24` 同名 env 分支 + `src/ssh_askpass.rs` | 对齐 |
| TUI 启动方式 | CLI 以库调用：`cli/src/commands/handlers/default.ts:3` `import { run } from "@opencode/tui"` | `oc-cli/src/run_ui.rs` | 对齐（形态同构） |
| Web UI 产物 | app 构建产物归档嵌入 CLI（`cli/src/app-assets.ts`；`publish.yml:204` "Build app archive"） | `release.yml:35-82` web-assets job（钉版 ref → bun build → brotli 归档）→ `oc-server/build.rs` 嵌入；`OPENCODE_APP_ASSETS` 运行时替换（`app_archive.rs:1-13`） | 对齐（R-04 已落地） |
| 平台矩阵 | `publish.yml`：linux（:44 blacksmith ubuntu）、windows（:88，:137 artifact）、macos-26 签名（:150-198） | `release.yml:85` 仅 **linux-x64** 一个 build job | **macOS / Windows / aarch64 / musl 全缺**——`ROADMAP.md:60` R-11（P3，awaiting user scheduling）。当前 macOS/Windows 用户无法安装 |
| GitHub workflows | 27 个（publish、publish-vscode、containers、deploy-www/files/posts、stats、storybook、nix-*、triage…） | 2 个（`ci.yml`、`release.yml`） | 缺的都是发布/托管/社区管理面，与 §2 分类一致 |
| GitHub Action / VS Code 发布 | `github/action.yml` + 4 个相关 workflow；`publish-vscode.yml` | 无 | 见 §2 P1 |
| 升级探活 | `opencode.ai`（services/update） | `api.github.com/repos/Eternal130/OpenCode-Rust/releases`（`update_feed.rs:18`）；永不采纳 TS 流（`updater.rs:16,103`） | 注册换道，合理 |
| nix | `flake.nix` / `nix/` / nix-eval、nix-hashes workflow | 无 | 缺（P2，安装渠道之一） |

---

## 5. 建议优先级汇总

- **P0（主程序必需，建议进入功能缺口台账）**
  1. `plugin-browser` → agent `browser` 工具（`core/src/plugin/internal.ts:89`、`plugin-browser/src/tools.ts:36`）。
  2. `latex` + `merman` TUI 内建渲染（`tui/src/plugin/builtins.ts:24-26`）——至少 mermaid（开发日常高频），latex 次之。
- **P1（用户可见）**：npm `@opencode-ai/sdk` 生态位、VS Code 扩展、GitHub Action、多平台二进制（若 R-11 启动则升级为 P0-分发）、npm/AUR 安装渠道。
- **P2（开发工具）**：containers、simulation、storybook、http-recorder 录制端、httpapi-codegen client 生成、script、nix flake。
- **P3（托管服务/资产）**：console/*、enterprise、function、stats/*、posts、services/{www,update,files}、identity。
- **已闭（凭书面裁定）**：desktop、web、gui-extensions（`ROADMAP.md:7-13`）；README「web UI not ported」一句与 R-04 矛盾，建议改 README。

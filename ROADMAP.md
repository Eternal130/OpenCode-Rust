# opencode-rust 移植路线图（ROADMAP）

> 2026-10-11 起。范围裁定（用户指令）：**除桌面端（packages/desktop Tauri 壳）外，v2.0.24 的一切都要迁移，包括 Web UI（packages/app）**。
> 规范源不变：opencode v2.0.24（commit e7a34f09bf，/opt/opencode-ref 只读）。
> 本文档是唯一的差距台账：每项有编号、来源、状态与目标批次；完成即勾销并注明验收证据。

## 范围解释（登记防歧义）

- **IN**：`packages/app`（Web UI，由 `opencode serve` 一并托管，PWA）、`packages/cli` 全部命令（含 `mini`、`acp`）、`session-ui` 等被 app/cli 共享的包、TUI 全部能力（C2 动画类解冻）、插件体系全部语义。
- **OUT**：`packages/desktop`（Tauri 壳）；`packages/web`（Astro 官网/文档站——营销站非产品功能，如需一并迁移另行指令）；v2.0.24 之后的上游新特性（钉版纪律不变）。
- `packages/gui-extensions`（SSH/WSL 远程 CLI 管理）：属桌面配套，随桌面端排除；若其中含独立于桌面的能力再个案评估。

## 状态图例

`pending` 未开工 ｜ `in-flight` 当前批次 ｜ `blocked` 等外部依赖 ｜ `done` 已完成并验收

---

## 一、当前批次 beta.3（in-flight，修复真实使用反馈）

| 编号 | 项 | 状态 |
|---|---|---|
| B3-01 | DB 行映射 NULL 容忍：按真实 v2 库普查逐列改 Option（session_v2×22 列、project×7、credential×3、project_directory×2、worktree×1、event_sequence.owner_id）；500 必落服务端日志（不再静默） | in-flight |
| B3-02 | TUI 终端交互六连修：stderr 不再糊屏（子进程与引导错误入日志文件）、会话错误按 v2 形态在转录中渲染、OSC 11 查询/应答消费时序对齐（应答不得进输入流）、主题强调色解析链修复（灰条→蓝条）、composer 光标闪烁（v2 时序）、`/` 命令面板预览区 | in-flight |
| B3-03 | rust 独立默认服务端口（sha1 派生：latest=52568 等；显式配置优先）——已落码，随本批门禁合并 | done(待门禁) |
| B3-04 | 新验证类补盲：「会应答转义序列的 PTY」集成测试；「真实库副本」集成测试（创建/列表/载入/重放） | in-flight |

## 二、功能缺失（全量消项——用户指令：全部解决）

| 编号 | 项 | v2 来源 | 优先级 | 目标批次 |
|---|---|---|---|---|
| R-01 | tree-sitter 语法高亮（diff/代码着色） | core 高亮管线 | P1 | beta.4；**方案已裁定（oracle）**：新叶 crate `oc-syntax`，原生精选 18 语言编译内置（bash/pwsh 已有；+rust/python/ts/js/go/c/cpp/c#/java/json/yaml/toml/html/css/diff），.scm 查询按 v2 的逐语言来源选择钉版 vendor（修复 v2 指向 nvim master 的不可复现性）；未覆盖语言回退纯文本（=现状）；禁运行时拉取（断网独立）；边界 API `highlight(block, filetype)`，markdown.rs/diff_viewer.rs/tool-row 三处接线；先行 spike（3 语言+体积/性能测量）后铺全量 |
| R-02 | `opencode mini` 第二前端 | cli/handlers/mini.ts | P1 | beta.4 |
| R-03 | `acp` 编辑器协议（Zed/Neovim 等） | cli/handlers/acp.ts | P1 | beta.4 |
| R-04 | **Web UI**：`serve` 托管 `packages/app`（PWA）+ 前端功能等价 | packages/app + serve 静态托管 | P1（大体量） | beta.5 起、跨多批 |
| R-05 | worktree UI | TUI/worktree | P2 | beta.6 |
| R-06 | 声音 / OS 通知 | TUI C2 类 | P2 | beta.6 |
| R-07 | 图像支持（粘贴/显示 kitty/sixel） | TUI/app | P2 | beta.6 |
| R-08 | 半块字形、子像素 RGBA 渲染 | TUI C2 类 | P3 | beta.7+ |
| R-09 | PTY 守护跨重启交接 | core pty | P3 | beta.7+ |
| R-10 | rmcp 0.5 elicitation | 协议库升级 | blocked（上游） | 跟踪 |
| R-11 | 平台矩阵扩展（aarch64/musl/macOS/Windows） | 非功能差距，单独决策 | P3（待用户定时间） | — |

## 三、行为分歧（消项或给出不可消的理由）

| 编号 | 现状 | v2 行为 | 处置 |
|---|---|---|---|
| D-01 | JSONC 配置写回：拒绝+`--force` | 保留注释重写 | **修**：实现注释保留写回（jsonc-parser 位置保持式重写），beta.4 |
| D-02 | 非空草稿时 leader 和弦不触发 | 和弦挂起输入框后触发 | **修**：beta.4 |
| D-03 | 插件 hook 可变对象 copy-in/copy-out | 引用可变（进程内） | **spike 评估**：bun 宿主内对象代间同步或等价语义；无法等价则永久登记并给出隔离性收益说明 |
| D-04 | 请求 schema 校验仅 2 路由 | 全路由 400 形状 | **修**：按 openapi 契约生成全路由校验电池，beta.5 |
| D-05 | /help clamp；模型 release-date 精细序简化；input_paste preventDefault 未移植 | 各自精确行为 | **修**：beta.6 |
| D-06 | TUI 本地状态 local→stable 目录不自动迁移 | 出厂 stable | **修**（一次性迁移逻辑，参照 v2 的 v1→v2 套路），beta.4 |
| D-07 | G11-F1 首连快照/流竞态理论窗口 | v2 存在同类（行为对齐） | 已双侧加固；补一个时序证明或长期登记 |

## 四、验证体系补强（本轮盲区的制度化）

1. **应答型 PTY 测试类**：终端模拟器对 OSC 11/DA 给真实应答——B3-04 落地后成为 TUI 门禁一环。
2. **真实语料库**：以脱敏后的真实 v2 库快照为只读 fixture 跑引擎/DB 集成（NULL 分布、大库性能）。
3. **终端矩阵冒烟**：tmux / kitty / Windows Terminal 差异面纳入 TUI_SMOKE 可选电池。
4. **长时浸泡**：serve+TUI 数小时级 soak（内存/句柄/事件序）。

## 四点五、开发纪律（结构性强制，2026-10-11 起）

本机与线上服务（newapi、49374）共享 16 核/22G。历史事故：未限流 cargo 构建把 CPU 打满一次；root 直跑 cargo 弄脏属主三次。纪律已从「写进泳道提示词」升级为**结构性强制**：

- **`tools/dev-gate.sh` 是一切本地构建/测试命令的唯一入口**：强制 eternal130 身份执行、装入 systemd 瞬态 scope（`CPUQuota=400%` 四核硬顶 + `MemoryMax=10G` + `TasksMax=512`）、1 分钟负载 ≥6 拒绝启动。已实测 `cpu.max=400000 100000` 生效——任何泳道漏写 `-j4`、或并行泳道叠加，物理上都不可能再打满机器。
- CI（GitHub Actions）豁免（不在本机跑）。
- 其余冻结纪律继续有效：绝不触碰 49374 与线上目录；测试一律 `127.0.0.1:0`；/tmp/opencode 顶层用 PID 唯一名；无裸 PID 杀进程；开发测试只写沙箱，不写用户真实数据目录；DNS 探针（/tmp/opencode/dns-probe.*）不属于构建产物，勿清理。
- 之后所有泳道规格必须写明「本地命令一律经 `tools/dev-gate.sh`」。

## 五、批次节奏

- **beta.3**（当前）：B3-01..04 → CI → 发布 → 真机验收。
- **beta.4**：R-01/02/03 + D-01/02/06（高感知日常项）。
- **beta.5**：R-04 Web UI（先架构侦察+spike：托管方式、构建链、鉴权流、与 API 的实时通道；预期跨批次）+ D-04。
- **beta.6**：R-05/06/07 + D-05。
- **beta.7+**：R-08/09、D-03 结论落地、R-11 按用户排期。
- 验收全部通过后发正式 **v2.0.24**（发布即终态，台账清零或带已具名豁免）。

### R-04 Web UI 架构决策（2026-10-11 侦察后裁定）

事实基础：Rust 侧托管契约已全量就绪（`oc-server/src/assets.rs` 的 serveUI 移植：SPA 回退、`_assets|icons` 404、immutable/no-cache 拆分、CSP+内联脚本哈希、nosniff、HEAD；`auth.rs` Basic+`auth_token`+同名端口 cookie；`pairing.rs` 配对；SSE 事件流）——**唯一缺口是 `state.rs` 的资产 map 为空**。v2.0.24 不发布独立 app 产物：app（Solid+Vite8+PWA，`packages/app`，无任何 `@opencode/desktop` 依赖）构建产物在 v2 内以 brotli+base64 归档**编译期嵌入 CLI**；dist 不入库、必须源码构建。

裁定（镜像 v2 语义、适配我们的发布管线）：
1. **构建**：release workflow 新增 `web-assets` job——拉取钉版参照源码（anomalyco/opencode @ e7a34f09bf tarball）→ `bun install` → `VITE_OPENCODE_SERVER_MODE=origin bun run build`（packages/app）→ 逐文件 brotli(q6) → 归档产物 job 间传递。不在开发机构建（无本地构建纪律），不依赖任何已安装二进制。
2. **嵌入**：Rust 侧新增 `oc-server/build.rs` 读归档（`include_bytes!` 等价），编译期写入资产 map；`OPENCODE_APP_ASSETS` 环境可指向磁盘归档（对应 v2 的 `OPENCODE_LOCAL` dist 回退），运行时惰性解压。
3. **一致性纪律**：归档携带 provenance 清单（ref commit、app 构建输入哈希）；`_assets` 内容哈希与 SW 完整性校验天然拒绝混版部署。
4. **验证**：浏览器端到端冒烟（serve → 登录屏 → Basic/配对链接登录 → 会话列表/发送/PTY WS）；套件内用微型 fixture 资产 map 钉版 headers/CSP/回退行为（不依赖真实归档）。
5. 体积预期：含 ui 包字体/图标/音频，dist brotli 后估计数 MB—十余 MB 级，随二进制分发（与 v2 嵌入 bun 单文件的做法对等）。

## 六、变更记录

- 2026-10-11：初版。范围扩大（Web UI 入、桌面端维持排除）；功能缺失全量消项令；beta.3 批次定义。

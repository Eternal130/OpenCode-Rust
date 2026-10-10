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
| R-01 | tree-sitter 语法高亮（diff/代码着色） | core 高亮管线 | P1 | beta.4 |
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

## 五、批次节奏

- **beta.3**（当前）：B3-01..04 → CI → 发布 → 真机验收。
- **beta.4**：R-01/02/03 + D-01/02/06（高感知日常项）。
- **beta.5**：R-04 Web UI（先架构侦察+spike：托管方式、构建链、鉴权流、与 API 的实时通道；预期跨批次）+ D-04。
- **beta.6**：R-05/06/07 + D-05。
- **beta.7+**：R-08/09、D-03 结论落地、R-11 按用户排期。
- 验收全部通过后发正式 **v2.0.24**（发布即终态，台账清零或带已具名豁免）。

## 六、变更记录

- 2026-10-11：初版。范围扩大（Web UI 入、桌面端维持排除）；功能缺失全量消项令；beta.3 批次定义。

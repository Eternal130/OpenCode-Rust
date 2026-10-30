# 缺口复核终版（第二轮可证伪式核查）

对第一轮 `PORT_COMPARISON_REVIEW.md` 所列全部缺口逐项复核（要求行级证据，允许推翻首轮结论）。结果：**4 项证伪删除、多项降级、2 项新发现、若干数字修正**。

## 一、终版缺口清单（按修正后风险排序）

### 高/中高

| # | 缺口 | 证据 | 修正说明 |
|---|---|---|---|
| 1 | **插件宿主 API 面缺 10 个命名空间**（aisdk、experimental.terminal、integration、model、provider、reference、vcs、websearch、worktree、plugin.list）≈ ref 26 命名空间的 62% | supervisor.rs:336-446 vs plugin.ts:27-53 | 证实；但修正两点：ref 的 shell 本就 hook-only 非缺失；tool.execute 两边都没有（Rust 另有 codemode.tool 补嵌套执行）。无 rpc 转发/http 变通，缺失即真缺失 |
| 2 | **plugin_update 为静默 no-op stub** + 插件 update/check/代数 GC 全缺 | handlers/misc.rs:166-174（`let _targets = …; Ok(no_content())`）；oc-plugin-host/context/install.rs 仅 install | ➕ 首轮未发现的实锤。影响：TUI 更新提示链整体不可用 + 重复安装磁盘累积；install 本身走 npm/bun 双路径可用（"仅 bun"说法不准确） |
| 3 | **本地 provider 动态发现缺失**（ollama /api/tags、lmstudio /api/v1/models、vllm /v1/models；Rust 全库零命中） | plugin/provider/{ollama,lmstudio,vllm}.ts vs grep 无命中 | 证实，评级中：custom provider 静态手工配置可逃生（facade.rs:227-241, 1044-1085），但新拉取模型不自动出现 |
| 4 | **9 个长尾 provider 使用时才报 UnsupportedProtocol**：perplexity、v0、aihubmix、sap-ai-core、qvac、salad-cloud、gitlab、merge-gateway、watsonx | facade.rs:447-451 gate；native.rs PACKAGES 表 | 数字修正：snowflake-cortex（@ai-sdk/openai-compatible）与 vercel（@ai-sdk/gateway）实际**支持**，首报有误。体验缺陷：模型在列表可见、OAuth 可完成，发起会话才报错。workaround 成立（custom provider + baseURL） |
| 5 | **Windows：无 CI、无测试、进程树清理（taskkill /T）无等价**；34 处 cfg(windows) 分支为编译性桩 | shell/select.rs:5 自述；ci.yml 仅 Linux | 证实"未验证"，但非空白（tilde 展开/COMSPEC/Git Bash 发现已移植）；release 目标含 Windows |

### 中

| # | 缺口 | 修正说明 |
|---|---|---|
| 6 | operationId 漂移 | **数字证伪：10 个而非 31 个**，仅 persistentPty 组带 `server.experimental.` 前缀（routes.gen.rs:401-669）；其余 125+ identifier 与 ref 逐字一致（首轮误用 endpoint 短名对比）。路径/方法零差异结论不变 |
| 7 | TUI 默认构建缺语法语言 | 数量修正：**17 个注册项（16 crate）而非 21**；ref 全集 37 证实；体积门控 +43.7MiB 超发布上限，属有意裁剪（注释在案），需 all-languages feature |
| 8 | 插件运行期改写配置的 state transform 框架缺失 | 部分属实，降级中偏低：compaction/snapshot 的 config 驱动初值已覆盖（engine/mod.rs:1352、snapshot.rs:172）；PATCH /api/experimental/config 只回流 shell。真正受影响的是动态 provider 注册等插件生态路径 |
| 9 | schema legacy 缺口 | 大幅修正：TODO 注释夸大——pty-ticket（oc-server/tickets.rs）、models-dev refresh（provider_catalog/refresh.rs + boot.rs:295）、event-manifest（oc-server/event_manifest.rs）、connection（integration.rs:23）**均已在他处实现**。真缺口仅：rpc schema 层校验弱化（运行时派发在）+ v1 部分死代码 |

### 低（降级或确认不影响）

| # | 项 | 修正 |
|---|---|---|
| 10 | ~~provider-context legacy media 迁移缺失~~ | **证伪删除**：to_llm_message.rs:634-687 `upgrade_legacy_media` 完整复刻 provider-context.ts:96-118 |
| 11 | ~~workspace/environment 执行域整层缺失（架构性）~~ | 降级为低：ref 生产路径也只有 local driver（memory 为测试用），workspace 是面向未来的挂载点（session.ts:316 TODO 自证）。属扩展点缺失，非默认行为回归 |
| 12 | ~~feature-plugins 热加载为近似实现~~ | **证伪**：ref 的 feature-plugins 本身是静态编译 builtin（builtins.ts:1-30），热加载只覆盖外部插件；Rust 内置面为原生等价重实现 + 外部插件真热加载（digest-scan + cache-bust），**两者均覆盖，等价替代** |
| 13 | flock 无 stale-lock 恢复 | 证实但降级低：flock(2) 内核级崩溃释放反而不会死锁；锁对象两侧一致；仅"进程假死"场景理论退化 |
| 14 | SystemOne 评测协议缺失 | 证实但降级低：库级 experimental API，monorepo 内无消费者、无用户入口，仅 zen/openrouter/typesafe 的 evaluation 模型位 |
| 15 | workerd（Cloudflare 部署形态）/ simulation 注入后端 | 均证实无对应；workerd 与 Rust 本地二进制目标冲突（范围外但未书面豁免）；simulation 影响 dev/录制工作流，parity 框架部分覆盖，不影响终端用户 |
| 16 | 未移植包 | 修正：真正用户可感且未豁免仅 **function + enterprise**（企业云产品对，中）；stats 聚合已被 oc-server/CLI 覆盖（站点本体缺）；storybook 已具名豁免；session-ui/ui 随 R-04 前端豁免；identity/posts/script 无需移植 |

## 二、复核统计

- 首轮缺口论断：16 项 → 证伪删除 3 项（legacy media、feature-plugins、shell/tool.execute 子项）、数字修正 3 项（operationId 31→10、语法 21→17、长尾 10→9）、降级 5 项、维持 5 项
- 新发现 1 项：plugin_update no-op stub（含 update/GC 链整体缺失）
- 双轨渲染像素差、PTY daemon 架构差异、rmcp 版本漂移三项维持原判（本轮未重验，有 golden tests/注释锚兜底）

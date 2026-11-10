# opencode v2.0.24 → Rust 移植全量逐行代码审查报告

日期：2026-11-10 ｜ 锚定：/opt/opencode-ref（v2.0.24, e7a34f09）｜ 对象：/opt/opencode-rust（2.0.24）

## 1. 结论

**移植整体高度忠实，未发现 P0 数据损坏级缺陷。** 27 条逐行对比 lane 覆盖两侧全部实现源码（ref ~455k / rust ~505k 非测试 LOC，非抽查；测试文件按需查阅）；核心域（内建工具、会话循环、上下文压缩、事件总线与存储、权限、凭证与模型目录、CLI 命令树、ACP、wire schema、openapi 生成物）达到逐行/逐字节级对齐——大量提示词文案字节级 pin、48 个 DB 迁移 ID 全同、141 个 API operation 逐对象字节级 0 diff、keybind 230/230 全对齐、主题与模型目录资产 md5 全等。

代价集中在三类：
1. **10 项高危发现（G-21..G-30）**——多数是小成本可修的真实 bug 或断线（见 §4）；
2. **4 类产品级功能缺口**——媒体 provider 已移植未接线、SystemOne/evaluation 域、cloudflare-ai-gateway 网关语义、workerd/simulation 剖面；
3. **架构换轴带来的系统性弱化**——CodeMode 沙箱逃逸面（G-01）、插件事件 best-effort、TUI live 投影延迟到 resync。

## 2. 方法与证据强度

- 三条 P0 侦察 lane 建立两侧清单（inv-ref-core / inv-ref-ui / inv-rust-map）→ 26 条域 lane（C×12 oc-core、A×5 oc-ai、T×4 oc-tui、S×6 外围）逐行成对阅读，每条输出逐功能对照表 + 差异清单 D-n（双侧 file:line）+ 风险 R-n + 覆盖声明。
- **不信任已有文档**：仓库内 PORT_PARITY_AUDIT/PORT_REVIEW/ROADMAP 等一律不作证据；审查中证伪了多处早前记录（工具名"bash/task 改名"不实、"36 内置主题"实为 33+1、"ref 任意色值 vs rust OKLCH"不实——ref 本身即 OKLCH、C6a 对 mcp.timeout 的反驳被裁定不成立、C5b 声称的 git.ts worktree 族缺失实为位置转移）。
- 两道 oracle 一致性门：P1 门（oc-core 11 报告，5/5 抽查成立，裁定 5 处矛盾，两项放行条件均由 orchestrator 亲验闭环）；FINAL 门（15 报告，0 未裁定矛盾，6/6 抽查成立）。
- 全部 32 份原始报告归档于 `lanes/`，含各 lane 诚实声明的未逐行部分（§6）。

## 3. 各域判定一览

见 `TABLE.md` 表 2（24 域）。摘要：✅高度对齐/忠实 19 域；✅/🟡（含明确缺口）4 域（snapshot-vcs、mcp-permission、pty-shell-fs、server）；结构性偏离 1 域（codemode ❌ 解释器语义换轴）。Provider 矩阵 46 项：31✅/13🟡/2❌。

## 4. 高危发现（建议进 P1 修复批次；均为小成本高收益）

| ID | 一句话 | 位置 | 修复方向 |
|---|---|---|---|
| G-01 | CodeMode 沙箱逃逸：Bun/process/`__rawFetch` 在 guest 全局可达，违背"无文件系统访问"承诺 | host.js:2833-2856,2841,2863 | 求值期冻结/白名单化全局；至少删 `__rawFetch` |
| G-02 | 非持久 pty per-chunk lossy UTF-8：CJK/emoji 输出跨 read 边界损坏 | oc-core pty.rs:623-638 | 改字节缓冲/增量解码（persistent_pty 已有先例） |
| G-03 | 用户中断后 hosted 工具多发持久 tool.failed(result-missing)，污染投影与下轮上下文 | step.rs:557 | 门补 `&& !stream_interrupted` |
| G-21 | WebSocketSeams（experimental.ws.* 插件钩子）构建后全链无消费者，插件改写静默失效 | model_request.rs:974-1318 → Prepared.web_socket_seams 无消费 | 接线到 SessionChannel execute |
| G-22 | permission.evaluate 插件 hook 注册成功但生产路径零派发 | plugin_apply.rs:1103（dispatch 仅测试调用） | Permission 引擎桥接 HookRegistry |
| G-23 | ACP：新增文件的权限 diff 预览全丢（added→old_text=None→continue） | acp/permission.rs:194-197 | added 分支对 "" 应用 patch |
| G-24 | ACP：retry.scheduled 的 nextRetryAt 恒空串（数字 at 经 string() 取值） | acp/translate.rs:529 | 按数字取值后 toISOString |
| G-25 | TUI 时长格式化 `%10_000` 取模 bug：10–60s 区间显示全错（15000ms→"5.0s"） | message/model.rs:915-926 | 删 `% 10_000` |
| G-26 | TUI stdin 双读竞争：OSC52 查询与输入线程并发 poll fd 0（吞键/吞应答） | terminal_proto.rs:474-519 vs app.rs:8717-8830 | 查询走输入线程串行化 |
| G-27 | Sessions 对话框 pin/unpin 死键（toggle_pin 仅测试调用；且 pinned 不持久化） | store.rs:6929；dialog.rs:524-529 | 补分发+持久层 |
| G-28 | runway cancel 路径重复 `/v1` → 404 | media/video/runway.rs:310-312 | cancel_path 去 `/v1`（当前无消费方缓冲） |
| G-29 | replicate/stability unsupported 字段静默丢弃（ref typed 拒绝） | image/replicate.rs:283-286、stability.rs:172-175 | body_from 补 check_unsupported |
| G-30 | gemini 命名空间工具重写丢 provider_executed/cache/metadata（server tool 回放降级） | oc-ai gemini.rs:1345-1371 | clone+改名字段保留 |

中级（G-31..G-48）与低级簇（G-49..G-63）全文见 `lanes/gate-FINAL.md`；P1 批次后建议按序处理 G-31（错误体截断先分类）、G-32（媒体接线或显式禁用）、G-35/G-36（server 幂等/插件 vcs）、G-39（mini 重试分类）、G-40..G-42（ACP 取消语义族）、G-48（tps 默认）。

## 5. 产品级功能缺口（单列，非行为偏差）

1. **媒体 provider 未接线（G-32）**：oc-ai/media 23 协议+route 全部移植完成，但 oc-core/oc-server 零引用——image/speech/transcription/video 上层功能静默不可用，易被误判"已支持"。
2. **SystemOne/evaluation 域（G-33）**：typesafe-ai（❌ provider 缺失）、opencode-zen/openrouter/vercel 的 evaluation 车道全缺。
3. **cloudflare-ai-gateway（G-34）**：URL 合成/cf-aig-* 头/前缀分派/passthrough 鉴权全缺，仅按 plain openai-chat。
4. **workerd/fetch 剖面 + simulation 协议（G-56/G-61）**：rust 仅独立进程 axum 形态；OPENCODE_SIMULATE 显式拒绝（golden/回放体系为内部等效替代）。

## 6. 审查盲区（如实披露；不构成"已验证一致"的声明）

按 FINAL 门清单 10 项（全文见 lanes/gate-FINAL.md）：影响最大的三项——①S3a 对 rust updater/plugin 安装路径（plugin.rs 151-863、updater.rs 121-2329、update_feed.rs）未逐行，G-46 的 installer 细节与升级原子性结论置信度受限；②A3 对 ~180 个 catalog-only provider 按 openai-compatible seam 推定，未逐一验证 api/env；③T1/T3 对 oc-tui store.rs 其余 ~13k 行与 SSE 增量 append 逐事件对拍未做（G-38 时序细节依赖此项）。其余为各 lane 声明的结构性/清单级核验区域。

## 7. 建议路线

1. **P1 修复批次**（§4 十项，多数为单行/单函数级）→ 补 `cargo test` + golden 回归。
2. **决策项**：媒体接线排期或配置面显式禁用；CodeMode 沙箱策略（全局冻结 vs 接受并文档化）；plugin/install 端点是否保留在 openapi（加 x-rust-extension 标注）。
3. **对测项**：G-07（真实分页 MCP 服务器）、G-10（跨 opencode-pty 二进制混用）。
4. **后续窄读**：§6 ①②③ 三处盲区补审。

## 8. 附

- `TABLE.md` — 逐项功能对比总表（架构差异 10 项、24 域对照、配置/资产级精确对照、未移植清单）。
- `lanes/` — 32 份原始报告（3 清单 + 26 分域逐行审查 + 2 门），全部差异/风险带双侧 file:line。

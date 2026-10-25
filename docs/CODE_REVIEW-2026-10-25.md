# opencode v2.0.24 → Rust 移植 全量代码审查(2026-10-25)

方法:5 条并行审查泳道(CLI / Server-API / Core / AI+插件 / TUI),每条泳道直接对照 /opt/opencode-ref 与 /opt/opencode-rust 源码逐项核实,不信任 PORT_REPORT 自述。判定:✅等价 / ⚠️语义分歧 / ❌缺失 / ➕超出上游。

## 总判定

| 域 | ✅ | ⚠️ | ❌ | 总评 |
|---|---|---|---|---|
| CLI | 17 | 10 | 0 | 命令树/旗标/错误消息逐字对齐;分歧集中在升级通道与二进制身份 |
| Server/API | 18 | 5 | 0 | 141 路由逐条一致、openapi 同 hash、48 迁移逐一相等 |
| Core | 40+ | 3 | 0 | 15 内置工具逐字核实,权限/MCP/config/skills 无缺口 |
| AI+插件 | 25 | 7 | 0 | provider/流式/usage 高保真;插件宿主与 CodeMode 为架构性替代 |
| TUI | 17 | 5 | 0 | 核心链路真实锚点;遗留均为已登记交互细节 |

**无 ❌ 级缺失;PORT_REPORT 未见虚报完成项,两处表述过时/失真(见问题 2、3)。**

## 跨域总对比表(合并要点)

| 功能项 | 判定 | 备注 |
|---|---|---|
| CLI 命令树(21 子命令+旗标面) | ✅ | serve 互斥消息、stats 全旗标、mini replay 三级解析逐字可核 |
| root `--session` 存在性探测 | ⚠️ | CLI 层直接透传,探测逻辑需在 oc-tui app::run 内核实 |
| `pair --url` 校验 | ⚠️ | 参照拒绝非 HTTP(S)/带凭据 URL;rust 仅 Option<String> 透传 |
| upgrade 7 种包管理器方法 | ⚠️ | 非真实执行,默认走自建 GitHub 通道(刻意、已登记) |
| 更新通道(feed 源) | ➕⚠️ | 换源必要分歧;sha256 边车校验为超上游加固(上游确无校验) |
| 二进制身份 `opencode-rust` | ➕⚠️ | 隔离身份设计,XDG 目录隔离+单向拷贝迁移 |
| HTTP 路由 141 操作 | ✅ | path×method 逐条 diff 一致 |
| OpenAPI 文档 | ✅ | sha256 与参照导出完全相同(实测);"语义 diff"叙述证明力弱 |
| SSE 帧编码/容量/心跳/过滤 | ✅ | 4096 容量、15s 心跳、server.connected 首帧逐语义对齐 |
| Last-Event-ID | ⚠️ | 实为客户端重连+本地去重,服务器不回放(报告表述失真) |
| 错误 envelope + 7 错误路径 | ✅ | `_tag` 形状一致;7/7 为 opt-in 门(OC_P10=1) |
| 请求 schema 校验 | ⚠️ | 28 条 ENFORCED_OPS + 21 处 in-handler,仍非 v2 全端点 decode(P10-N1 开放);报告"仅 2 条"已过时 |
| DB 48 条迁移 / 引擎语义 | ✅ | 名称顺序逐一相等;Semaphore(1) 对应单连接会话 |
| 企业 JWKS | ✅ | RS256/kid 选择;30s timeout 为超上游加固 |
| session 生命周期/projector/revert/fork/compaction | ✅ | projector 165 事件逐列对账背书 |
| usage/stats/affinity/history 子域 | ⚠️ | 无同构文件、语义分散,未做行级 diff(建议补定向对账) |
| 权限系统 | ✅ | 求值序/deny 优先/saved-sql 全对齐 |
| 15 内置工具(read…websearch,逐一) | ✅ | 工具描述逐字一致;todowrite/list/LSP 在 v2.0.24 core 本就不存在 |
| MCP(含 elicitation/OAuth) | ✅ | rmcp 3.7;capability 差异已注释论证 |
| config(global/project/JSONC 保注释写回) | ✅ | jsonc_edit 移植 jsonc-parser 3.3.1;`--force` 为 rust-extra |
| config watch flock | ⚠️ | 部分语义未移植,单进程无影响 |
| skills(含 URL 源)/command/AGENTS.md | ✅ | 2026-10-24 补齐项已核实 |
| v2_migration(单向拷贝) | ➕ | 独立身份所需,marker 闩/锁/backup API 与报告相符 |
| Provider 全家(anthropic/openai×3/gemini/bedrock/vertex/groq) | ✅ | usage 口径、SigV4、WS 通道(报告曾误登记延后,已实现) |
| 流式/工具循环/cache-policy/tool-history/cost | ✅ | 274 cassette + 132 golden 背书;tier 重复 size 时取值口径理论性差异 |
| 插件宿主(bun 子进程) | ⚠️➕ | copy-in/copy-out hook、私有 NDJSON 线协议(良性行为等价) |
| CodeMode | ⚠️ | 真 JS 引擎替代受限解释器;**缺 MAX_STRING/ARRAY/PENDING_PROMISES 结构性上限**;嵌套结果取值口径(带 schema 的 MCP 工具返回 string) |
| /api/plugin、零插件 CodeMode 核心化 | ✅ | 生产接线已核实 |
| TUI 渲染(opentui-solid 唯一路径) | ✅ | 旧引擎/旗标已删净 |
| composer/tabs/transcript/dialog/sidebar/home | ✅ | golden 机制真实存在(37 场景未复跑) |
| 键位系统与 leader 和弦 | ✅⚠️ | 数据层完整;非空草稿和弦不触发(v2 会挂起 textarea) |
| 主题 63 个 | ✅ | SHA256 钉版 + parity golden |
| tree-sitter 语法高亮 | ✅⚠️ | vendored query vs 参照在线拉取,存在漂移风险 |
| 图像 kitty/sixel | ⚠️ | env 启发式探测(无 DA1);PNG→sixel 转码缺失(可能同 v2) |
| 声音/OS 通知 | ⚠️✅ | Linux 同引擎;非 Linux 外部播放器为平台妥协;OSC 通知直连 |
| $EDITOR 挂起恢复 / 重启恢复 / CJK 宽度 | ✅ | 均有单测/golden 锚点 |
| PTY 守护跨重启交接 | ✅➕ | 独立 oc-pty 二进制,超出参照的架构 |
| acp / mini 前端 | ✅ | 文件级映射完整 |

## 问题清单(跨域合并,按严重度)

**🟠 中**
1. CodeMode 缺结构性资源上限(oc-core/tool/codemode.rs:37-60 自认)——建议 guest 注入分配计数。
2. 请求校验白名单子集 vs v2 全端点 decode(P10-N1);且 PORT_REPORT"仅 2 条"描述过时,应更新为 28+21 实况。
3. "Last-Event-ID 重订阅为我们新增"表述与实现不符(服务器不消费该 header)——报告应改为客户端重连协议预留。
4. root `--session` 存在性探测与 `pair --url` 校验疑似缺失(CLI 层未见,需 oc-tui/misc::pair 定性)。
5. leader 和弦非空草稿不触发——用户可感知键位分歧,最值得收口。
6. usage/stats/affinity/history 子域未做行级语义 diff,仅靠 projector 对账背书。

**🟡 低**
7. 升级通道 7 种包管理器方法非真实执行;uninstall 无安装法探测输出。
8. 语法 query vendored 漂移风险——建议定期对照 nvim-treesitter HEAD。
9. 图像协议探测 env 启发式;非 Linux 声音外部播放器。
10. 内部 operationId 系统性改名(experimental.* 前缀)——按 operationId 映射的生态工具两侧不互通,应文档化。
11. CodeMode 嵌套结果取值口径(带 schema 工具返回 string 而非结构化值)。
12. `api --param` 可重复(超集)、`--force` rust-extra、websearch provider 硬编码注册表。
13. 核心一致性证据多在 opt-in 门(OC_P10=1),常规 CI 不跑;建议至少把错误路径形状纳入常规门。

**ℹ️ 信息**:tier 重复 size 取值口径、`fs.read/*path` 命名、JWKS 30s timeout、SSE announce 并入公共过滤、install.sh 无上游锚点——均为已登记或理论性项。

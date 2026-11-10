# FINAL-GATE 结论: 15 份 lane 报告交叉一致性全部成立（0 处未裁定矛盾），6 条关键结论抽查 6/6 核验通过，主风险登记册合并至 G-63，判定**通过**（附非阻断条件）。

## 矛盾裁定（第 1 门已裁定 5 项不重审；本门新查 6 个交叉点）

| # | 交叉点 | 裁定 |
|---|---|---|
| 1 | **A3 媒体未接线 vs A4 "协议已移植无消费方"** | **同口径，互证成立**。A3-D1（grep 证实 oc-core/oc-server 对 `oc_ai::media` 零引用）与 A4-R5/D-3（route.rs:1004-1006 代码注释自认未消费）证据同源同向。合并为 G-32。 |
| 2 | **T1 selection/clipboard vs T2/T3 引用** | **一致**。无交叠无矛盾。 |
| 3 | **S2 store 投影缺口 vs T3 store.rs 审查** | **兼容（互补）**。S2-D9 称 6 类 transcript 事件无 live reducer、仅靠 resync 填充；T3 证实渲染侧变体全集存在（model.rs:262-361 ✅）——"能渲染、但延迟到下一次 resync"。 |
| 4 | **S4a TUI API 缺失清单 vs T4 D-9/R-2** | **同口径，互证成立**。清单完全包含关系。合并为 G-45/G-46。 |
| 5 | **C7a pty 协议超集（G-10）vs S1 pty WS 端点 ✓** | **兼容（不同层）**。S1 审 handler 层共有 op 帧语义；C7a 审 wire schema 层 rust 额外 op 超集。互操作风险（G-10）维持。 |
| 6 | **A1a D-3 deepinfra 探测表 vs A3 deepinfra 行 ✅** | **一致**。A3 证实 facade 注入同等 compat。 |

已裁定项复核：mcp.timeout、注册失败通道、ws seams、copilot bearer、端点数 142=141+1（S4b 给出 141 operation 字节级 0 diff）——均无反复。

## 抽查核验（6 条，全部亲读源码）

1. **A2-R1（WS seams 无消费者）——成立 ✅**。`web_socket_seams` 构建于 model_request.rs:974-1318、存入 Prepared(:473/:1065)；全仓消费点仅 tests/plugin_guest_wiring.rs:276-307。oc-ai 执行路径零消费。
2. **T3-R1（locale_duration 取模 bug）——成立 ✅**。model.rs:916-917 `f64::from(input as u32 % 10_000)/1000.0`：15000ms→"5.0s"（应为 15.0s）；10s–60s 区间全部显示 `x-10.0s`。
3. **S3b-R2（nextRetryAt 恒空串）——成立 ✅**。session_event.rs:594 `at: NonNegativeInt`（数字）；translate.rs:529 `string(&event.data,"at")` 实现 `as_str()→unwrap_or("")`——恒回空串。
4. **S4a-R1（permission.evaluate 无生产派发）——成立 ✅**。`dispatch_permission_evaluate` 全仓仅 2 个调用点（hooks.rs:1959、plugin_apply.rs:2727），均在 `#[tokio::test]` 内。
5. **T2-R1（toggle_pin 死键）——成立 ✅**。`toggle_pin` 全仓 3 处：定义 store.rs:6929 + 测试 :14443/:14445。
6. **S1-D3（shell DELETE 404 vs 204）——成立 ✅**。handlers/shell.rs:119-122 remove 分支映射 404；ref 吞 NotFoundError 恒 204。

## 合并主风险登记册（延续 P1 的 G-01..G-20）

### 高
| ID | 域 | 摘要 | 状态 |
|---|---|---|---|
| G-21 | A2/S4a | **WebSocketSeams 三 seam 构建后无消费者**，experimental.ws.* 插件改写静默失效 | 成立（抽查+互证） |
| G-22 | S4a | **permission.evaluate guest hook 注册后永不被生产派发** | 成立（抽查） |
| G-23 | S3b | added 文件的权限 diff 预览全丢 | 成立 |
| G-24 | S3b | session.retry.scheduled 的 nextRetryAt 恒空串 | 成立（抽查） |
| G-25 | T3 | locale_duration 秒档 `%10_000` 取模 bug，10–60s 时长全错 | 成立（抽查） |
| G-26 | T1 | stdin 双读竞争：OSC52 查询与输入线程并发 poll fd 0 | 成立 |
| G-27 | T2 | Sessions 对话框 pin/unpin 死键 | 成立（抽查） |
| G-28 | A4 | runway cancel 路径重复 `/v1` → 404 | 成立（无消费方缓冲） |
| G-29 | A4 | replicate/stability unsupported 字段静默丢弃（行为反转） | 成立 |
| G-30 | A1a | gemini 命名空间扁平重写丢 provider_executed/cache/metadata | 成立 |

### 中
| ID | 域 | 摘要 |
|---|---|---|
| G-31 | A2 | 错误体先 2KB 截断后分类：嵌套 error.code 漏检→误判 retryable |
| G-32 | A3/A4 | 媒体 provider 协议已移植但零部署接线（13+ 家静默不可用）+ decode unwrap_or_default 系统性宽松 |
| G-33 | A3 | SystemOne/evaluation 域缺失（typesafe-ai/zen/openrouter/vercel /evaluate） |
| G-34 | A3 | cloudflare-ai-gateway 网关语义全降级（URL 合成/cf-aig 头/前缀分派/passthrough 鉴权缺） |
| G-35 | S1 | shell DELETE 非幂等（ref 204 / rust 404） |
| G-36 | S1 | vcs.init 插件 provider 不可达（硬编码 git\|hg 旁路） |
| G-37 | S2 | 空白 delta 丢弃：流式空行/缩进丢失，终值可不一致 |
| G-38 | S2/T3 | live 投影缺口：6 类事件延迟到 resync；usage 只取 cost 漏 tokens |
| G-39 | S3a | mini 重连循环对任何非 "Session not found" 错误无限重试 |
| G-40 | S3b | Sessions::release 身份判定恒真→可误拆并发重 attach |
| G-41 | S3b | turn 取消后事件流静默则 prompt 永不返回 |
| G-42 | S3b | $/cancel_request 预取消不中断 server + ask 阻塞折叠循环 |
| G-43 | A1a | vertex Express+tuned 未守卫 + apiKey/accessToken 互斥缺 |
| G-44 | A1a | /compact 响应宽容解析→"成功但空/残"历史 |
| G-45 | T4/S4a | guest 插件无法注册 markdown 代码块渲染器 |
| G-46 | S4a | install 无原子性：残缺安装可被复用（staging 从未创建） |
| G-47 | A1a | google-interactions 事件形状零校验（index 缺失折叠至 0） |
| G-48 | T4 | session.tps 默认值反转（ref true / rust false） |

### 低（簇）
G-49（T1 剪贴板/标题/SIGHUP/吞键/kitty/300ms 丢键/颜色经验推导/帧率/轮询）、G-50（T2 pinned 不持久/附件标签/静态面板/历史守卫）、G-51（T3 低 verbosity 族/synthetic 空行/mount 权重/tok-s 口径/Diagnostics/RetryProvider/语法默认窄）、G-52（A1b bedrock metadataKey/诊断 body/AWS 链子集/invoke 独有/mistral 哈希/copilot kind）、G-53（A2 pending 序/Some(false)/complete 时机/Metric/typed 子集）、G-54（A3 alibaba 区域/bigmodel 根/xai conv-id/package 不用/anthropic-compatible 降级）、G-55（A4 cancel 反转/去抖动/拒绝时机后移/multipart 边界）、G-56（S1 workerd/simulation 剖面/独有端点/events.persist/cursor/replay 分块）、G-57（S2 effect 面/连接放大/basePath/serde 宽松/版本谓词）、G-58（S3a TTY 交互/升级换源/param 覆盖/诊断缺席/端口错开）、G-59（S3b child _meta/MCP 去重/handoff/command 时序/zed:// /realpath/multiselect）、G-60（S4a 子路径矛盾/vcs dispose 清空/stub/键回退/事件 best-effort/sidebar 不可点/effect 拒载/rpc client 面/vcs·worktree 读面/超时/进程模型）、G-61（S4b BTreeMap 键序/NonEmpty/identifier 竞窗/simulation/flock/glob/theme null）、G-62（T4 对象键绑定丢弃/win32 ctrl+z/插件目录发现/未消费键/修饰折叠/热重载）、G-63（A1a partial-json NaN/providerExecuted/thinking delta/error.code 截断/选项硬失败/轻校验/route 字段/baseURL 模板）。

## 各域状态统计表
（见 lanes/ 目录各 cmp 首行与 gate-P1；汇总于 REPORT.md 表 3）

## 审查盲区清单（10 项）
1. S3a 窄读：rust plugin.rs:151-863、updater.rs:121-2329、update_feed.rs 全文、auth.rs:701-1518、mini.rs:161-398——G-46 installer 细节未逐行复核（最高优先披露）。
2. A3：~180 个 catalog-only provider 未逐一验证 api/env 字段。
3. T1：store.rs 其余 ~13k 行、components 逐行（靠 T2/T3+注释引用密度覆盖）。
4. T3：store.rs SSE 增量 append 逐事件对拍未做；solid 画笔逐像素依赖 golden；history prepend 位置未定位。
5. S1：rust model.rs/fs.rs/plugins.rs/lifecycle.rs/errors.rs 未逐行（路由表+抽查）。
6. S4a：host.js codemode 解释器内部按任务书跳过（逃逸面结论已独立核验）。
7. C 系遗留：variant.ts 其余 ~40 家族常量 diff、optimize/*.txt 字节 diff、persistent_pty/frontend.rs(583L)、repository_cache.rs 结构级。
8. A1a/A1b：open-responses 主文件仅签名级、responses-hosted-tools 三端交叉验证、bedrock SSO/IMDS 细节、copilot 转换器全量。
9. T4：mini footer.prompt/command 子特性、theme expand/resolve/syntax 字段级（结构面+md5 替代）。
10. S4b：theme resolve.rs 后段、v1_migrate 函数级、低风险 util 仅确认落位。

## 门判定：**通过**（非阻断条件）
1. 修复跟踪（高，小成本高收益）：G-21/G-22（插件断线）、G-23/G-24（ACP）、G-25（删 `% 10_000` 一行）、G-26（stdin 串行化）、G-27（toggle_pin 分发）、G-28（runway cancel_path）。
2. S2-D9 与 T3 的"渲染在、live 更新缺"拼合口径写入终报告。
3. 盲区 10 项如实披露；S3a updater/plugin 结论标注置信度受限。
4. G-32/G-33/G-34 属产品级功能缺口，单列不混入行为偏差。

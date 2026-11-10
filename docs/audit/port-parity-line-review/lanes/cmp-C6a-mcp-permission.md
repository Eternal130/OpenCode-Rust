# C6a 结论: MCP 客户端/注册表/权限域整体是高保真移植（超时、会话过期恢复、权限求值/ask-reply 语义逐行对齐），但工具桥丢失 sessionID 元数据、连接期无 OAuthClientProvider（静态 bearer + 预检探测替代）带来四处可观察行为差异，且工具列表不分页与桥同步无互斥是两个真实风险点。

## 对比表

| 主题 | ref (file:line) | rust (file:line) | 判定 |
|---|---|---|---|
| StreamableHTTP 传输 + codemode=false 参数 + 原始 URL 一次回退 | client.ts:267-292 | client.rs:1478-1487, 1560-1587 | 对齐 |
| stdio 传输（env seam、16MiB 帧上限、stderr 尾巴诊断、BUN_BE_BUN） | stdio.ts:10-18, 45-115, 165-190; client.ts:248-258 | client.rs:1308-1465 (stdio_overlay/FrameCapReader/StopOnDropReader/stderr_tail) | 对齐（spawn 语义经 StdioSpawner seam） |
| Mcp-Session-Id 门控的过期检测（404/特定 400） | client.ts:228-244 | client.rs:313-529 (SessionObservingClient + session_expired, G-223 类型化) | 对齐（typed downcast 替代文本匹配，更稳） |
| 会话过期一次上报 + 前台重放一次 / 后台 recover | index.ts:340-365, 415 | client.rs:781-798; registry.rs:941-967, 1302-1323 | 对齐 |
| 启动 30s / catalog 30s / 执行 12h / terminate 1s（`config.timeout` 覆盖） | client.ts:34-37, 218, 317-318 | client.rs:107-113, 1262-1278 | 对齐（**C4 的 “mcp.timeout 无消费者” 结论被证伪**：`config_timeout` 逐字段消费，client.rs:1222-1278） |
| 瞬态重试 250ms/1s 两跳、408/429/5xx≠501、startup 截止 | client.ts:52-58, 169-182, 280-292 | client.rs:534-570, 1534-1587 | 对齐（G-280① 已修正延迟为 250/1000） |
| needs_auth 判定 | client.ts:383 (SDK UnauthorizedError) | client.rs:1500-1637 (preflight_challenge + unauthorized()) | 机制不同，见 D-1 |
| initialize 能力（elicitation form/url + roots）、roots/list 应答 | client.ts:186-206 | client.rs:684-717, 1703-1727 | 对齐（applyDefaults hint 无法经 rmcp 传递，文档化） |
| 协议协商 absent/legacy/auto/pin + 不可协商错误文案 | client.ts:376-380, 393-397 | client.rs:1229-1260, 1648-1677 | 对齐（2025-11-25 提供版本固定） |
| toCallToolResult 内容归一（text/media/resource_link/resource） | client.ts:399-418 | client.rs:180-229 | 对齐 |
| callTool `_meta ai.opencode/sessionID` + progress 保活 | client.ts:352-363 | client.rs:1033-1111 (meta 有; progress token 无显式设置) | 见 D-2 |
| MRTR input_required 重试轮（单执行截止、仅 elicitation 可满足） | (SDK 内部) | client.rs:1037-1152 | rust 显式实现，与 TS SDK 2.0 行为对齐（test 引证） |
| 状态机五态 + startup latch + per-server 锁 + per-URL 锁 | index.ts:78-109, 169, 427-494 | registry.rs:127-179, 464-493, 687-765, 767-771 | 对齐 |
| whenLive 晚回调丢弃 | index.ts:328-336 | registry.rs:1034-1055 (is_same) | 对齐 |
| 事件 mcp.{tools,resources,status,prompts}.changed 发布点 | index.ts:402-425, 460-466, 489-493 | registry.rs:738-764, 830-842, 905-989 | 基本对齐，见 D-6 |
| reconcile（初始异步连接、diff replace/remove、disabled、override tombstone） | index.ts:534-575, 599-625 | registry.rs:1095-1202 | 对齐 |
| credential.switched → reconnect（owned 过滤 + 锁下身份复查 + disabled 跳过） | index.ts:577-598 | registry.rs:588-633 | 对齐 |
| 集成身份 `mcp_<sha1(name\0url)[0..16]>` | index.ts:176-190 | registry.rs:431-447, 852-865 | 派生对齐；**method 注册不入 Integration 服务**，见 D-5 |
| OAuth authorize（challenge→discovery→DCR/CIMD/复用 client→PKCE+state→loopback→exchange→凭证落库） | oauth.ts:330-532 | oauth.rs:695-932 | 对齐（CIMD 条件、iss 校验 G-110②、resource 参数 G-110①、回调页 oauth_page.rs 与 page.ts 逐串一致） |
| refresh 并发去重（键=endpoint+client_id+refresh_token） | oauth.ts:29-53 | oauth.rs:962-1036 (Shared future + RAII 移除) | 语义对齐，键用 config.url 替代 token endpoint（文档化偏差） |
| refresh token 轮转保存 / 拒绝时删行 | oauth.ts:286-296, 304-314 | registry.rs:779-828; oauth.rs:1107-1115 | 对齐 |
| OAuth 回调页（成功/失败卡、自动关窗 2.5s） | oauth/page.ts:24-120 | oauth_page.rs:20-97 | 逐字对齐（含 fail/ok 内联脚本） |
| elicitation：global 哨兵会话、url/form 两模式、空 schema 直接 accept、complete 结算、pending 表 all-exit 删除、字段转换（机器标题正则/number-vs-integer/enum/enumNames） | index.ts:93-94, 231-302, 786-844 | elicitation.rs:165-375, 382-548 | 对齐（G-280② number/integer 修复；`custom:false` 见 D-7） |
| instructions 注入（mcp-guidance key、可见性过滤 canExecute+逐工具非 deny、增量/全量 delta 文案） | instructions.ts:17-54, 69-106 | runner/context.rs:517-620, 1368-1410 | 对齐（空集=Removed 语义保留） |
| 工具桥：namespace/name 清洗、permission action=`{server}_{tool}`、assert 先行、结构化输出偏好（structured→无 schema 时 JSON.parse 文本→文本）、media→data: URI、isError 文本拼接 | tool/mcp.ts:16-17, 43-118 | registry.rs:1628-1743, 1809-1958 | 对齐（G-242 pipeline 包裹、G-291 有效名键）；sessionID 见 D-2 |
| 工具桥刷新：初始一次 + mcp.tools.changed 100ms 滑动去抖 + flush 门 | tool/mcp.ts:33-144 | registry.rs:1812-1958 | 机制对齐；互斥缺失见 R-2 |
| 权限 evaluate（扁平化 findLast 通配、默认 ask）、merge、configured（agent 缺失=deny-all） | permission.ts:87-101, 158-163 | permission.rs:50-94 | 对齐（wildcard 语义有测试表） |
| evaluateInput 四步（base deny 短路不查 saved→saved 追加→hook 仅非短路路径） | permission.ts:173-189 | permission.rs:430-470 | 对齐 |
| ask/assert/reply/close（parked map、replied 先 publish 后 resolve、reject 批量同会话、always+save 落库后批量重评放行、Declined 缺陷隧道） | permission.ts:204-325, 133-146 | permission.rs:491-688 | 对齐（拒绝批注 message→Corrected 一致） |
| saved 持久化（唯一索引冲突忽略、空 resources no-op、project 过滤） | saved.ts:42-82 | permission/saved.rs:56-116; sql.rs:8-34 | 对齐 |
| managed-policy（Console statements 整体替换、组织名消息、authored 反序合并、provider.use/permission 双消费面 findLast） | managed-policy.ts:23-32; config/plugin/policy.ts:26-67 | config/managed_policy.rs:60-159 | 对齐 |
| codemode 默认表（executor.sh 正则/本地 `executor mcp`→codemode:false；PostHog 主机表→x-posthog-mcp-mode: tools，?mode= 或已有头任一大小写则跳过） | plugin/mcp-codemode-defaults.ts:11-38 | registry.rs:1971-2069 (+测试) | 对齐（URL 解析手写替代 `new URL`，默认端口剥离等价） |

## 行为差异清单 D-n

- **D-1 连接期鉴权机制重构（最大结构差异）**：ref 把完整 `OAuthClientProvider` 交给 SDK——握手 401 触发 SDK 内部 discovery/register/authorize、**运行中 401 自动用 refresh token 续期并重放请求**（client.ts:149-151 注释 + provider）；rust 无 provider：连接前 `connect_bearer` 一次性取凭证（5 分钟余量内先 refresh，registry.rs:779-828），运行期 401 仅归类为 `(unauthorized)` 错误文本（client.rs:799-803），**不会续期也不会转 needs_auth**——token 过期后所有调用持续失败直到手动重连。另：每次远程连接前多发一次 initialize POST 预检（client.rs:1500-1637, 10s 上限），ref 无此请求。
- **D-2 工具桥不传 sessionID**：ref 每次工具桥调用把 `context.sessionID` 放进 `_meta["ai.opencode/sessionID"]`（tool/mcp.ts:69 + client.ts:358）；rust `McpToolExecutor::execute` 调 `call_tool(..., None)`（registry.rs:1672），registry 具备该参数但桥不用——服务器无法把调用归属到会话（影响按会话计费/审计的 MCP 服务器）。
- **D-3 列表请求无分页**：ref 经 SDK `client.listTools/listPrompts/listResources`（SDK 2.0 内部处理分页）；rust 发单次 `ListToolsRequest::default()` 不跟 `nextCursor`（client.rs:855-971）——分页服务器的目录会被截断（与 R-1 合并看）。
- **D-4 OAuth authorize 触发面收窄**：ref 经 Integration 服务注册 `authorize` 方法，UI `/mcps` 可直接发起登录；rust `register_integration` 只算 ID，授权流只在 CLI `mcp auth` 驱动（registry.rs:844-865 注释自认）。刷新流的 token 请求也不带 RFC 8707 `resource` 参数（oauth.rs:1055-1070），ref SDK 的 refresh 带。
- **D-5 晚期 resources-changed 回调无 whenLive 门**：ref `onResourcesChanged` 包在 `live()` 里（index.ts:424）；rust 回调无条件 spawn publish（registry.rs:925-935）。被替换连接的迟到通知会多发出一次事件；同理 `refresh_tools` 失败时 rust 仍发布 tools.changed（registry.rs:1015-1019 无 Ok 门）。
- **D-6 关闭原因文本**：ref onClose 上报 `Connection closed: {lastError}`（client.ts:364-366）；rust 固定 `"Connection closed"`（registry.rs:985），丢失传输层最后错误细节。
- **D-7 elicitation string 字段 `custom`**：ref 有 options 时 `custom: false`（禁自定义输入，index.ts:840）；rust `Field::String { custom: None }`（elicitation.rs:544）——枚举字段仍允许自由输入。
- **D-8 authorize scope 回退链**：ref `oauth?.scope ?? challenge.scope`（oauth.ts:470）；rust 多一级 `?? discovery.scope`（oauth.rs:764-765），授权 URL 可能带上 ref 不会带的 scope。
- **D-9 needs_auth 检测时序**：rust 预检在 codemode 参数追加**之后**的 URL 上发（client.rs:1523 用最终 url），且 403 也判 needs_auth；ref 仅 401。行为面接近但服务器视角多一次探测请求。

## 正确性风险 R-n

- **R-1 工具/资源/提示目录分页截断**：rmcp 单次 List* 请求返回首页；若目标服务器启用分页（`nextCursor` 非空），rust 的 `tools()`/`resources()` 丢弃后续页 → 工具静默缺失、权限面与 codemode catalog 不完整（client.rs:855-971 四个 list 均单发）。ref 侧 SDK 是否真自动分页无法从 checkout 验证（node_modules 缺失），两侧需以真实分页服务器对测。**建议**：按 `nextCursor` 循环或改用 rmcp 的 all-variants。
- **R-2 桥同步无互斥（初始 vs 去抖刷新竞态）**：ref 的初始注册与 reconcile 共用 `Semaphore(1)`（tool/mcp.ts:33, 124）；rust `McpToolBridge::install` 的初始 `sync` 与去抖 `sync` 各自 spawn、无锁（registry.rs:1831-1882）。交错时可短暂注册过期工具集；低概率、自愈，但违反 ref 的串行不变量。
- **R-3 运行期 token 过期无恢复路径**：12h 执行超时窗内 access token 过期的长连接，ref 由 SDK 静默 refresh；rust 连接的 Authorization 头是连接时快照（client.rs:1498-1502），过期后每次调用报 unauthorized 直到会话过期事件或手动重连。对 expires_in < 会话寿命的 OAuth 服务器是持续故障。
- **R-4 预检 initialize 的副作用面**：预检用 `id:0` 的裸 initialize 打真实服务器（client.rs:1600-1637），某些单会话 legacy 服务器会为探测请求分配会话状态/日志；且预检 10s + 真握手可能叠加双倍延迟。
- **R-5（低）** elicitation 数组默认值只接受字符串数组（elicitation.rs:490-497）；ref 透传 `property.default`。非字符串默认项丢失——影响极小。

C1 R-1（坏工具拖垮整批）在本 lane 的注册路径上**不成立**：rust 注册是逐工具 infallible map insert（location.rs:561-564），单条坏工具最多污染自身键，无批次失败通道；若 C1 所指为 schema 校验层，属 C1 lane。

## 覆盖声明

已逐行读完：ref `mcp/{index,client,stdio,oauth,instructions}.ts`、`permission.ts`、`permission/{saved,sql}.ts`、`managed-policy.ts`、`tool/mcp.ts`、`plugin/mcp-codemode-defaults.ts`；rust `mcp/{mod,client,registry,oauth,oauth_page,elicitation}.rs`、`permission.rs`、`permission/{saved,sql}.rs`、`config/managed_policy.rs`、桥与 instructions 注入（`session/runner/context.rs` 相关段、`location.rs` ToolRegistryState 注册段）、`oc-schema` mcp 配置形状。`mcp/oauth_page.rs` 与 ref `oauth/page.ts` 以关键字符串比对而非全文 diff；`permission/sql.rs` 前 60 行 + 模式注释；`mcp/tests.rs`（3111L）与 ref 测试未读。ref 侧 SDK 内部行为（listTools 分页、MRTR、OAuth 401 处理）以其 checkout 内调用点+注释为证据，node_modules 不可见，相关结论（R-1、D-1）标注了不确定性。

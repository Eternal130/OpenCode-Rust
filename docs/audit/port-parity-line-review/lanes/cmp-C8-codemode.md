# C8 结论: CodeMode 宿主面（catalog/instructions/limits/协议）基本逐字节对齐，但 rust 把「受限解释器」换成了 Bun 裸 JS 执行——语法/stdlib 面从「白名单子集」变成「全集+逃逸面」，且签名渲染、诊断结构、超时语义、search 别名等存在多处可观测差异。

## 能力矩阵

### A. 解释器 AST 节点支持（ref = acorn 解释器 interpreter.ts:436-481/1291-1354 switch；rust = host.js:2862-2865 `new AsyncFunction("tools","search", '"use strict";'+code)` 全真 JS）

| 节点/语法 | ref | rust(host.js) | 备注 |
|---|---|---|---|
| var/let/const、if/switch/循环/try/throw/label/break/continue | ✅ | ✅ | ref 有逐节点语义（含 per-iteration let 绑定 interpreter.ts:678-692） |
| 函数/箭头/async/generator/yield*/await/for-await | ✅（自研 generator 状态机 :1810-2032、Pending 调度 promises.ts） | ✅（原生引擎） | 调度/微任务顺序不逐位等价（ref 每步可中断） |
| 解构（对象/数组/Rest/默认值）、展开 | ✅ | ✅ | |
| 模板串/标签模板（strings.raw、per-site 缓存） | ✅ :2135-2166 | ✅（原生） | |
| 可选链/空值合并/逻辑赋值/instanceof/in/delete/arguments/this=undefined | ✅ | ✅（strict this=undefined 同） | |
| **class 声明/表达式/getter/setter/私有字段** | ❌ unsupportedSyntax（new 也拒绝用户构造器 :1356-1374） | ✅ 可用 | 超集 |
| **eval / new Function** | ❌（无 eval 全局；Function 构造器抛错 globals.ts:30-58） | ✅ 可用（strict 不禁 eval） | 逃逸面（见 R-1） |
| with | ❌ unsupportedSyntax | ❌（strict SyntaxError，可观测近似） | |
| 静态 import/export | ❌（无该节点） | ❌（Function 体 SyntaxError） | |
| 动态 import()/require | ❌（无节点/无全局） | ❌ 双侧静态扫描拒绝（codemode.rs:1262 / host.js:2672，注释自认 token 级近似可绕） | |
| BigInt 字面量 | ❌ typeError :166 | ✅（过 JSON 边界会抛） | |
| `new` 内建（Date/Map/…） | ✅（Native.construct） | ✅ | |
| 顶层 return / 隐式完成值 | ✅（末表达式 :400-402） | ✅（AsyncFunction 完成值） | |
| 空程序 | ❌ ParseError "Code cannot be empty."（execute.ts:19-25，trim 判空） | rust 仅 `code.length===0`（host.js:2820）——**空白程序 rust 成功返回 undefined** | D-7 |
| 未 await Promise 属性读 | ❌ invalidData "This value is an un-awaited Promise; await it first"（interpreter.ts:2239-2248） | ✅ 可读（原生 thenable 属性） | D-5 |

### B. stdlib / 全局面（ref = globals.ts:87-131 白名单 ~45 个绑定；rust = Bun 全局，仅 console/fetch/timers 被换装 host.js:2833-2856）

| 面 | ref | rust | 备注 |
|---|---|---|---|
| Object/Array/String/Number/Boolean/Math/JSON/RegExp/Date/Map/Set/WeakMap/WeakSet/groupBy | ✅ 受限实现（stdlib/*.ts，原型链冻结、长度上限内置） | ✅ 原生（超集） | ref 各方法带 checkStringLength/ArrayLength（如 interpreter.ts:1438）；rust 仅桥上检查 |
| console log/info/warn/error/debug/dir/table | ✅ console.ts（格式化+logs 捕获） | ✅ host.js:2528-2601 移植 formatConsoleMessage/table/前缀；捕获进 result.logs | 逐句对齐（G-180①） |
| Iterator helpers/Generator/AsyncGenerator/Promise/Symbol.iterator/asyncIterator | ✅ | ✅ 原生 | |
| URL/URLSearchParams/Headers/TextEncoder/TextDecoder/Uint8Array(base64/hex)/crypto/structuredClone/atob/btoa/encodeURI* | ✅ | ✅ 原生 | |
| `fetch`（仅 Extension 注入，30s 超时+净化响应） | ✅ web.ts:13-38 | ✅ host.js:2610-2636 换装 codemodeFetch（30s、url/status/ok/headers get/has/entries/text/json/bytes 同形） | 对齐；但见 R-1 `__rawFetch` 泄漏 |
| timers（setTimeout/setInterval/setImmediate） | ❌ 无全局 | ❌ 换成抛错占位 host.js:2809-2813 | 对齐（G-247），restore 正确 |
| **Bun/process/globalThis 其它全局（Bun.file、Bun.spawn、process.env、node:* 经 globalThis）** | ❌（无此进程面） | **✅ 可达**（仅换装 console/fetch/timers 四件） | R-1 沙箱逃逸 |
| Error/Error 子类（TypeError/RangeError…） | ✅ errors.ts/intrinsics（带 AST 定位） | ✅ 原生（无定位、无 Diagnostic 归一化） | D-4 |
| search 全局 / tools 树 | ✅ globals.ts:88-90（ToolReference 链、根不可调、`tools.search(...)` 别名 tool-runtime.ts:452） | ✅ host.js:2436-2519 buildToolsTree/makeSearch；❌ `tools.search(...)` 别名、根调用报错文案、UnknownTool "Did you mean" 均无 | D-2/D-3 |

### C. ExecutionLimits（ref codemode.ts:22-41,126-137 + limits.ts；rust codemode.rs:1133-1178 + host.js:2306-2319）

| 项 | ref | rust | 判定 |
|---|---|---|---|
| timeoutMs ≥1，无默认=不限 | ✅（步间中断 execute.ts:56-89；程序已返回但后台未完 → ok:true+TimeoutExceeded warning :74-87） | ✅（宿主 tokio timeout codemode.rs:1834-1856 + guest Promise.race host.js:2872-2883） | 消息逐字同；**后台未完场景 rust 整体报错**（D-6） |
| maxToolCalls ≥0 | ✅（recordCall 先查后记 tool-runtime.ts:392-397） | ✅ 双侧计数（guest host.js:2372-2374 + leaf codemode.rs:1792-1799） | 对齐 |
| maxOutputBytes ≥0 | ✅ boundOutput（值/warnings 独立预算/logs 用余量 execute.ts:129-193） | ✅ leaf bound_output codemode.rs:1499-1576 + guest 预截断 host.js:2919-2928 | 逐字节文案对齐（测试钉死） |
| 校验错误文案 | `X must be a safe integer greater than or equal to N.` | 同 | ✅ |
| MAX_STRING_LENGTH 1<<24 / MAX_ARRAY_LENGTH 1e7 / MAX_PENDING_PROMISES 1e4 | ✅ **分配时**检查（limits.ts:15-21，ref 内建/运算符产出即抛） | ✅ 仅 **桥跨越时**（host.js:104-125/133-139）；guest 内可构造超大串/数组不受限 | R-2 |
| MAX_VALUE_DEPTH 32 | ✅ 边界 walk（data.ts） | ✅ 双侧（codemode.rs:1441 / host.js:2330，先于环检测） | ✅ |
| MAX_CALL_DEPTH 10000 | ✅ interpreter.ts:376,1751 "Maximum call stack size exceeded" | ❌ 无（原生栈溢出 RangeError 文案不同、由 supervisor 兜底） | D-8 |
| RangeError 文案 "Invalid string length"/"Invalid array length"/"Too many pending promises (limit 10000); …" | ✅ | ✅ 逐字 | ✅ |

## 对比表（其余功能）

| 功能 | ref | rust | 判定 |
|---|---|---|---|
| **tool-runtime prepare/search** | tool-runtime.ts:126-319（trie、canonical 去重、searchText=path+desc+ns desc+input 属性名/描述） | host.js:2439-2519（tokenize/termForms/rank/20-12-8-4-2 打分、exact/namespace 归一/offset/limit10/remaining/next 全对齐）；searchText 由宿主渲染（codemode.rs:168-179） | ✅（测试 codemode.rs:391-533 钉） |
| **Hooks** | Hooks tool.before/after + extension.before/after（tool-runtime.ts:56-81）；progress 每调用实时（codemode/tool.ts:143-176，fetch 有专用行） | pipeline executor 包 execute+每个嵌套调用（hooks 等价）；progress 仅结束时一次性（codemode.rs:1707-1708 注释自认）；**无 extension hooks / fetch 行** | D-9 |
| **make() 语义/值拷贝** | 数据经 JSON 边界往返（tool-runtime.ts:431-438） | guest↔host NDJSON JSON 序列化天然拷贝 | ✅ |
| **tool-schema JsonSchema 子集渲染** | tool-schema.ts:25-201（$defs/$ref 解析、anyOf 数字哨兵、docTags 顺序、pretty、MAX_RENDER_DEPTH 8） | codemode.rs:255-510（docTags 顺序/哨兵/深度 8 对齐）；**$ref → "unknown"（无 $defs 解析）**；字段序=serde 字母序（ref=声明序，测试自认分歧 codemode.rs:2022-2027） | D-10 |
| **signature 输出类型** | `Promise<outputTypeScript>`；core 无 outputSchema 时兜底 `Schema.NullOr(Schema.String)` → **`Promise<string \| null>`**（core codemode/tool.ts:234 + tool-schema.ts:269-273） | **恒 `Promise<void>`**（codemode.rs:242-249，测试钉死 void） | **D-1（文案级偏差，所有无输出工具的 catalog 签名都不同）** |
| **catalog summarize/instructions** | catalog.ts:54-189 / instructions.ts | codemode.rs:629-803/955-1108（UTF-16 计费、120 截断、pinned 预付、round-robin、update 最小 delta、文案逐句对齐） | ✅（G-106 测试钉） |
| **execute 工具 description/instructions 文案** | core codemode/tool.ts:62-69 | codemode.rs:76-81 EXECUTE_DESCRIPTION | ✅ 逐句一致 |
| **execute 输出形状** | output+toolCalls+error+files（data: 文件直出 content） | output 文本+metadata（toolCalls/error）+progress；**files/data: 附件不收集**（nested_value 只取 text/output，codemode.rs:1712-1734） | D-11 |
| **openapi/**（spec.ts 725L resolve/$ref/operationInput/Output/securitySchemes + runtime invoke + fromSpec→工具集） | ✅ | **❌ 无任何对应**（grep 全 crates 仅 server /openapi.json 与 vertex 端点） | D-12：属 @opencode/codemode 库导出面，ref core/execute 不消费；若外部插件用 fromSpec 构建工具集则不可用 |
| **extension.ts 宿主全局注入** | 全局注册+重名 TypeError+值拷贝（codemode.ts:151-157, extensions.ts） | 无通用 extension 机制；fetch 硬编码换装（G-180②） | D-13 |
| **mcp codemode defaults** | plugin/mcp-codemode-defaults.ts | codemode_defaults.rs（executor.sh/posthog 全案表逐字） | ✅ |
| **codemode.evaluate 协议 seam** | 进程内直调 | protocol.rs NDJSON（GUEST_ID_BASE 2^40）+ host.js METHODS:3610；token 路由嵌套调用到本求值闭包 host.js:2386-2393 | ✅ 架构换位（文档声明） |
| **超时对同步死循环** | 步间可中断（解释器天然） | guest 内不可中断 → supervisor kill+重启（codemode_limits.rs:185 测试） | 架构换位，可观测=进程重启 |

## 行为差异清单

- **D-1** 工具签名输出类型：ref `Promise<string | null>`（无 outputSchema 兜底 NullOr(String)，core tool.ts:234）vs rust 恒 `Promise<void>`（codemode.rs:245）→ 全部 catalog 行/instructions 与 ref 逐行不同。
- **D-2** `tools.search(...)` 别名：ref 在无同名工具时映射到内建 search（tool-runtime.ts:451-453）；rust `tools.search` 是普通对象键 → TypeError。
- **D-3** 未知工具：ref `Unknown tool 'x'. Did you mean tools.y?` + suggestions（tool-runtime.ts:333-355）；rust 宿主 `Unknown Code Mode tool: x`（codemode.rs:1819），无 did-you-mean/suggestions；且 ref 的诊断有 kind/location（codemode.ts:84-91），rust 只有裸 message 字符串。
- **D-4** 异常形态：ref 归一化为 Diagnostic（ParseError/UnsupportedSyntax/… + line/column，errors.ts locate/materialize）；rust 直接 errText(error)（host.js:2888），SyntaxError/TypeError 原生文案、无位置字段；formatResult 的 suggestions 附加逻辑（tool.ts:313）随之失效。
- **D-5** 未 await 的 Promise：ref 读其属性抛 invalidData 提示 await（interpreter.ts:2241-2246）；rust 原生可读 `.then` 等，静默。
- **D-6** 超时且程序已返回但有后台 pending：ref 返回 ok:true + warning（execute.ts:74-87）；rust Promise.race 整体 reject → 报错。
- **D-7** 空白 code：ref trim 后判空报 ParseError "Code cannot be empty."（execute.ts:19）；rust 仅 length===0（host.js:2820），空白程序静默成功。
- **D-8** 调用深度：ref MAX_CALL_DEPTH=10000 + "Maximum call stack size exceeded"（interpreter.ts:376,1751）；rust 无对应（原生栈 RangeError 文案/阈值不同）。
- **D-9** progress 事件：ref 每次调用实时 toolCalls 流（tool.ts:86-89）；rust 结束后一次性（codemode.rs:1707）；且无 fetch 的 extension 行。
- **D-10** schema 渲染：rust `$ref→unknown`（codemode.rs:375）+ 字段字母序 vs ref 声明序；签名/搜索文本对 $defs 工具不同。
- **D-11** 嵌套工具产生的 file content（data: 附件）：ref 收集为 ExecuteOutput.files + content file parts（tool.ts:100-127,329-342）；rust 丢弃非 text part（nested_value 只拼 text）。
- **D-12** openapi/ 子包（fromSpec/securitySchemes/runtime invoke）完全未移植——库 API 面缺失，产品内无消费点。
- **D-13** Extension 机制（重名全局 TypeError、extension hooks）无通用移植，仅 fetch 硬编码。

## 正确性风险

- **R-1（高）沙箱逃逸面**：rust 程序在 Bun 全局环境执行，host.js 仅换装 console/fetch/timers（2833-2856）。`Bun.file(p).text()`、`Bun.spawn`、`process.env`、`globalThis.__rawFetch`（无超时原始 fetch，2841 行设置）在程序内全部可达，违背 execute description "direct filesystem access … unavailable" 的承诺（ref 解释器结构性无这些面）。静态扫描只挡 import()/require。建议：求值期冻结/遮蔽 Bun/process/globalThis 白名单化，或至少删 `__rawFetch` 全局残留。
- **R-2（中）结构上限只在桥上**：MAX_STRING/ARRAY_LENGTH 在 guest 内构造阶段不生效（ref 在每个内建/运算符产出点检查），`"a".repeat(1e9)` 可在 guest 内耗尽内存后才被桥拦截；timeoutMs 对纯同步构造不中断，靠 supervisor kill（进程级重启代价）。
- **R-3（中）token 级扫描绕过**：`eval("require('x')")`、`new Function(...)`、`(0,eval)` 可绕过 import()/require 扫描（host.js:2666-2668 注释自认"follow-up"）；与 R-1 叠加即事实逃逸。
- **R-4（低）重叠求值竞态**：console/fetch/timers 换装用全局栈帧（host.js:2640,2946-2961），同一 guest 进程并发两次 evaluate 时 restore 依赖栈序；嵌套 OK，但一次求值抛错路径（2889 finally）与另一次进行中的捕获交错时 `__rawFetch` delete 可能过早。
- **R-5（低）maxToolCalls 双计数**：guest（host.js:2372）与宿主 leaf（codemode.rs:1792）各自计数并各自报同文案错误；guest 计数含被宿主拒绝的未知工具调用（ref 先 decodeInput 成功才 recordCall），极限下计数可偏移。
- **R-6（低）`require` 误报面**：`tools.ns.require(...)` 点访问被双侧扫描拒绝（自认 pinned false-positive，codemode.rs:1254-1257）——若真实 MCP 工具名含 require 段则永久不可调。

## 覆盖声明

已逐行读：ref codemode/src 顶层全部（codemode/data/extension/index/namespace/tool-error/tool-runtime/tool-schema/tool/tools）、interpreter/{interpreter(2334L 全),execute,limits,globals}、core/src/codemode/{tool,catalog,instructions,web} 全部；rust host.js 全 3733 行、protocol.rs、codemode_defaults.rs、oc-core/tool/codemode.rs 全 3156 行；openapi/spec.ts 等按需 grep+读入口（确认无 rust 对应物）。**未逐行读**：ref interpreter/{objects,promises,scope,references,generators,intrinsics,native,callback,errors,extensions,model} 与 stdlib/{string,array,object,math,json,collections,url,bytes,iterator,…} 各方法注册表（仅经 globals.ts/interpreter.ts 交叉印证其存在性与受限语义；rust 为原生超集，逐方法对照对 rust 无判别力）、ref openapi/{spec(725L),runtime} 内部、oc-plugin-host/tests/codemode*.rs 仅作行为证据抽查（三文件测试名/断言已核）。结论中 file:line 均出自上述已读源码。

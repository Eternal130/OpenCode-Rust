# Parity Scan 2026-10-18 · 02 工具与执行运行时

- 参考实现：`/opt/opencode-ref`（opencode v2.0.24，TypeScript）
- 目标实现：`/opt/opencode-rust`（Rust 移植）
- 范围：内置工具（shell/edit/write/read/glob/grep/patch/subagent/question/skill/webfetch/websearch/opencode.\*/mcp-resource/codemode `execute`）、tool 注册与执行管线、tool-output、file-access/file-mutation、filesystem/、snapshot、shell/、pty/、persistent-pty/、ripgrep、formatter、image。**不审计** AI tool-call 协议（另有 lane）。
- 方法：逐文件源级对照（ref 侧 ~10k 行、rust 侧 ~20k 行），只读；KNOWN/NEW 对照 `ROADMAP.md`、`FINDINGS.md`、`docs/reposcan-findings.md`、`docs/rescan-findings.md`、`docs/scan-ab-findings.md`、`docs/scan-cd-findings.md`、`docs/auditE-core.md`。

## 0. 总览统计

| 类别 | 数量 |
|---|---|
| 差异条目总数 | **33** |
| MISSING | 8 |
| STUB | 2 |
| PARTIAL | 9 |
| WRONG | 13 |
| EXTRA | 1 |
| S0 | 0 |
| **S1** | **11** |
| S2 | 22 |
| 其中 KNOWN（已登记） | 6 |
| 其中 NEW（本次新发现） | 27 |

整体判断：**执行侧骨架高度完整**——15/15 内置工具全部存在并注册（engine pre-plugins），六文件工具（read/write/edit/glob/grep/patch）与 subagent/question/webfetch/websearch 的模型可见字符串基本 byte-exact，tool-output 预算算法、file-access/file-mutation 锁与 BOM 语义、shell 解析器（tree-sitter legacy + 5355 行 portable 扫描器）、snapshot（git2 内容寻址树）都是实移植。缺口集中在**接线层**：config 键（shell / experimental.\* / tool_output / formatter）没有接到执行路径、结构化 `output` 在 ToolRunResult 边界被丢弃、formatter 是 no-op、`opencode` 命名空间在注册时被清空、shell 输出捕获的流序/死锁风险。

## 1. 工具清单与注册

### 1.1 参考 ↔ Rust 映射总表

| Ref（packages/core/src） | 行数 | Rust（crates/…） | 行数 | 状态 |
|---|---|---|---|---|
| `tool.ts`（注册/Editor/snapshot/executeTool/normalizeImages） | 332 | `oc-core/src/tool/mod.rs` + `tool/registry.rs` | 354+524 | PARTIAL（output 丢弃、图片归一化缺失，见 D1/A6） |
| `tool/runtime.ts`（decode/encode/normalizeContent/effectiveName） | 279 | `tool/registry.rs:26-252` | — | PARTIAL（自研 JSON-Schema 校验子集，见 1.2-R） |
| `tool/plugin/edit.ts` | 230 | `tool/edit.rs` | 416 | ✅ 基本全量（B1） |
| `tool/plugin/write.ts` | 101 | `tool/write.rs` | 142 | ✅ |
| `tool/plugin/read.ts` + `tool/read-filesystem.ts` | 211+402 | `tool/read.rs` | 653 | PARTIAL（B2/B3/B4/B5） |
| `tool/plugin/glob.ts` | 139 | `tool/glob.rs` | 173 | ✅ |
| `tool/plugin/grep.ts` | 170 | `tool/grep.rs` | 435 | ✅（C12） |
| `tool/plugin/patch.ts` + `util/patch.ts` + `tool/patch.txt` | 363+381+txt | `tool/patch.rs` + `patch_util.rs` + `patch.txt` | 561+683+txt | ✅（patch.txt byte 相同；E1/E4） |
| `tool/plugin/shell.ts` | 290 | `tool/shell.rs` | 672 | PARTIAL（C1-C8） |
| `tool/plugin/subagent.ts` | 311 | `tool/subagent.rs` | 642 | ✅（C5/E-中断） |
| `tool/plugin/question.ts` | 132 | `tool/question.rs` | 280 | PARTIAL（A8） |
| `tool/plugin/skill.ts` | 72 | `tool/skill.rs` | 189 | ✅ |
| `tool/plugin/webfetch.ts` + `tool/html-markdown.ts` + `tool/http-body.ts` | 189+658+30 | `tool/webfetch.rs` + `tool/html_markdown.rs` | 264+2023 | ✅（E2/E8） |
| `tool/plugin/websearch.ts` + `websearch.ts` + `plugin/websearch/*` | 196+227+6文件 | `tool/websearch.rs` + `websearch_providers.rs` | 819+574 | PARTIAL（A9/E3） |
| `tool/plugin/opencode.ts` | 201 | `tool/opencode_tools.rs` | 534 | PARTIAL（A1-A4） |
| `tool/plugin/mcp-resource.ts` | 101 | `tool/mcp_resource.rs` | 394 | PARTIAL（A7） |
| `tool/plugin/file-diff.ts`（helper） | 23 | `tool/file_diff.rs` | 290 | ✅ |
| `codemode/`（catalog/tool/instructions/web） | 718 | `tool/codemode.rs` | 1385 | ✅（AI 协议 lane 另审） |
| `file-access.ts` | 167 | `file_access.rs` | 842 | ✅ |
| `file-mutation.ts` | 134 | `file_mutation.rs` | 480 | ✅ |
| `tool-output.ts` | 150 | `tool_output.rs` | 496 | PARTIAL（D2/C11） |
| `filesystem.ts` | 211 | 无 core 服务；`oc-server/src/handlers/fs.rs` | 340+ | PARTIAL（D4） |
| `filesystem/search.ts`（fff+fuzzysort 索引） | 202 | `handlers/fs.rs:253-315` 朴素子串遍历 | — | PARTIAL（D4） |
| `filesystem/fff*.ts`（fast-file-finder 绑定） | 105 | — | — | MISSING（被朴素遍历替代） |
| `filesystem/watcher.ts`/`watcher-binding.ts`/`location-watcher*.ts`/`protected.ts` | 493 | `oc-server/src/announce.rs:505+`（仅 HEAD 监视） | — | ✅ 语义等价（v2 也只 watch `.git/HEAD`）；通用 Watcher util 无对应 |
| `snapshot.ts` | 233 | `snapshot.rs` | 663 | ✅（B7） |
| `shell.ts` | 455 | `shell.rs` | 676 | PARTIAL（C1/C2/C8/C11） |
| `shell/parse.ts`/`scan.ts`/`select.ts`/`result.ts` | 402+2409+227+81 | `shell/parse.rs`/`scan.rs`/`select.rs`/`result.rs` | 602+5355+313+241 | ✅（legacy 用原生 tree-sitter bash/powershell） |
| `pty.ts` + `pty/*` | 321+137 | `pty.rs`（portable-pty） | 818 | PARTIAL（C10） |
| `persistent-pty/*`（daemon 进程边界） | 1119+85 | `persistent_pty.rs`（进程内机器 + handoff 快照） | 1818 | PARTIAL（C9，KNOWN phase-9） |
| `ripgrep.ts` + `ripgrep/binary.ts` | 275+200 | `tool/grep.rs`（runner）+ `tool/mod.rs:116-139`（binary seam） | — | PARTIAL（A5） |
| `formatter.ts` + `formatter/builtins.ts` + `config/plugin/formatter.ts` | 104+297+90 | `formatter.rs`（NoopFormatter） | 147 | STUB（B8，KNOWN） |
| `image.ts` + `image/*`（photon 缩放） | 95+116 | — | — | MISSING（A6，KNOWN） |

工具名对照备注（任务清单中的旧名）：v2.0.24 中 `bash`→`shell`、`task`→`subagent`、`multiedit`→`patch`（多 hunk）、`list/ls`→`read` 的目录页（`list-page`）、`todo`/`lsp` 工具在 v2.0.24 core 中**不存在**（edit.ts:104-107 留有 LSP TODO 注释），故无需移植。

### 1.2 差异表

| ID | 标题 | 类型 | 严重度 | KNOWN/NEW |
|---|---|---|---|---|
| A1 | `opencode` 命名空间在注册时被清空 | WRONG | S1 | NEW |
| A2 | `session_move` 工具恒失败 | STUB | S1 | KNOWN（reposcan #51/#131、auditE #34） |
| A3 | worktree 指引 system part 未接线 | MISSING | S2 | NEW |
| A4 | `models` 工具 own-provider 优先排序失效 | WRONG | S2 | NEW |
| A5 | ripgrep 二进制自动下载缺失 | MISSING | S2 | NEW |
| A6 | Image（photon）归一化服务缺失 | MISSING | S2 | KNOWN（auditE §1.17 ❌ normalize） |
| A7 | `list_mcp_resources` 模型内容为固定文本 | WRONG | S2 | NEW |
| A8 | question 多选题退化为单选 | WRONG | S2 | NEW |
| A9 | websearch provider 注册被 env 键门控 | PARTIAL | S2 | KNOWN（代码注释自认；reposcan #79 已修 env 接线） |

**A1** `opencode` 命名空间丢失
- ref：`tool/plugin/opencode.ts:84-88` 注册 `namespace({name:"opencode",…})`；`:95/:115/:140` 三个工具 `options:{namespace:"opencode", codemode:true}`（`session_move` 另有 `pinned:true`，opencode.ts:115）。effectiveName ⇒ `opencode_session_rename` / `opencode_session_move` / `opencode_models`（tool/runtime.ts:276-279）。
- rust：`tool/registry.rs:274-279` 中 `registration.namespace = None;` **硬编码**，且 `BuiltinTool`（tool/mod.rs:182-194）根本没有 namespace 字段；engine 侧 `opencode_plugin`（engine/mod.rs:1608-1626）走 `register_builtin`。⇒ 广告名是 `session_rename`/`session_move`/`models`，codemode 目录路径 `tools.session_rename`（codemode.rs:123-129 `qualified_path`）而非 `tools.opencode["session_rename"]`。
- 影响：模型按 v2 习惯/按我们自己的 WORKTREE_GUIDANCE（A3）调用 `tools.opencode.session_move` 会失败；permission 默认 action（=effectiveName）与 v2 不同；mcp-resource 两个工具因为自带 `register`（mcp_resource.rs:236-258 设 `namespace=Some("opencode")`）是对的，同一 registry 里两套命名并存。

**A2** `session_move` 恒失败
- ref：`tool/plugin/opencode.ts:109-133` 经 `ctx.session.move({…delivery:"steer"})` 真实移动。
- rust：`tool/opencode_tools.rs:100-125` 校验后直接返回错误 `"session move is not wired until the session-move lane lands"`；`engine/mod.rs:1606` 注释自认。用户侧 `session.move` API 已通（reposcan #51），仅 LLM 路径断。

**A3** worktree 指引 system part 未接线
- ref：`tool/plugin/opencode.ts:71-81` 在 session context/compaction/generate 三钩子推送 system part："…consider using `execute` to call `tools.opencode.session_move`…"。
- rust：常量存在（`tool/opencode_tools.rs:9 WORKTREE_GUIDANCE`）但全仓 grep 无任何推送点。
- 影响：模型不会收到该指引；叠加 A1 后即使收到也指向不存在的路径。

**A4** `models` own-provider 排序失效
- ref：`tool/plugin/opencode.ts:145` 读取当前会话 `model?.providerID`，排序时本 provider 置顶。
- rust：`tool/opencode_tools.rs:397` `own_provider: parking_lot::Mutex::new(None)`，注册后无人写入（engine/mod.rs:1619-1624 只传 catalog）。⇒ 永远按字母序。

**A5** ripgrep 自动供给缺失
- ref：`ripgrep/binary.ts`——`which rg` → `<global.bin>/rg` → 从 GitHub 下载 `ripgrep-15.1.0` 平台包并解压（含 windows zip / tar.gz 路径）。
- rust：`tool/mod.rs:116-139`——`OPENCODE_RG_PATH`（**ref 没有的 EXTRA 环境变量**）→ `<exe_dir>/rg` → PATH。无下载、无 `<data>/bin` 检查。
- 影响：干净机器上 grep/glob 直接 `"ripgrep execution failed"`；语义与发现顺序均不同。

**A6** Image 归一化缺失
- ref：`tool.ts:74-101` 对工具输出中的 data-URI 图片做 `Image.normalize`（photon-wasm，2000×2000/5MiB 上限，超限缩放，失败时插入 `[N images omitted: …]` 文本注记）；`image.ts:58-93` + `image/photon*.ts`。
- rust：registry 管线（`tool/registry.rs:419-466`）无任何图片处理；全仓无 photon/resize/omitted 注记。
- 影响：超大图片原样进上下文（成本/失败），且 config `image` 限制键无效。

**A7** `list_mcp_resources` 模型内容
- ref：`tool/plugin/mcp-resource.ts:45` 返回 `{output}` 无 content ⇒ `normalizeContent(undefined, output)`（tool/runtime.ts:259-263）⇒ 模型看到 **资源目录的 JSON 文本**。
- rust：`tool/mcp_resource.rs:202-205` content 固定为 `"Listed MCP resources"`，真实目录塞进 `metadata["mcp.output"]`。
- 影响：direct 路径上模型拿不到资源列表（codemode 路径受 D1 连带影响）。

**A8** question 多选题
- ref：`tool/plugin/question.ts:119-132` `type: question.multiple === true ? "multiselect" : "string"`。
- rust：`tool/question.rs:45-72` `to_field` 恒 `Field::String`，`multiple` 字段被忽略。
- 影响：`multiple:true` 的问题只能单选。

**A9** websearch provider 注册策略
- ref：`plugin/websearch/*.ts` 无条件注册 provider，凭据在执行期经 integration host（`integration.connection.active`，env/OAuth）解析；`websearch.ts:177-210` 查询循环含 429 冷却轮换（retry-after 解析，websearch.ts:215-221）。
- rust：`websearch_providers.rs:519-530 providers_from_env()` 以 **env 键存在** 为注册门槛（代码注释自认为临时方案）；冷却轮换已移植（websearch.rs:210-247）。
- 影响：仅 OAuth/凭据库（无 env 键）配置的 provider 不可见。

## 2. 文件 / 快照（file tools + mutation + snapshot）

| ID | 标题 | 类型 | 严重度 | KNOWN/NEW |
|---|---|---|---|---|
| B1 | edit 声明输出缺 `files` | WRONG | S2 | NEW |
| B2 | read 二进制检查范围过宽 | WRONG | S2 | NEW |
| B3 | read 缺文件建议列表排序不同 | WRONG | S2 | NEW |
| B4 | AGENTS.md 发现未走 realpath | PARTIAL | S2 | NEW |
| B5 | read 整文件读入内存（无 256KB 分块流式） | PARTIAL | S2 | NEW |
| B7 | snapshot enabled 门缺 vcs 判定（服务层） | PARTIAL | S2 | NEW |
| B8 | formatter 子系统 no-op（26 个内置 + config 键 + 执行） | STUB | S2 | KNOWN（reposcan #55/#144：v2 默认不注册 formatter，默认行为等价） |

其余核对为**等价**：edit 三段匹配器（exact→unicode→行级 trimEnd，edit.rs:41-152 vs edit.ts:49-102）、CRLF 归一、BOM 保留（`bom_join`）、写前 permission assert 先于 0/多匹配报错（edit.rs:338-368 vs edit.ts:180-198）、write 的 added/modified 预览与 `existed`、`writeTextPreservingBom` 只留一个 BOM、`withLock` 按 resolve 后路径排序串行 + 跨图共享锁表（file_mutation.rs:161-241 vs file-mutation.ts:60-78）、父目录自动创建（ref 经 `writeWithDirs` 的 NotFound-then-mkdir，fs-util.ts:138-154；rust 直接 create_dir_all——语义等价）、非原子写两侧一致（均直接 writeFile）、read 的 `{n}: {line}` 行格式/2000 行行宽 clamp/offset 越界与媒体 20MiB 上限/目录页排序（目录优先）均逐字对齐；snapshot 的内容寻址仓库（`<data>/snapshot/<projectID>/<Hash.fast(worktree)>`，SHA-1 手写）、scope 计算、2MiB untracked 上限、源仓库 ignore 规则过滤（tracked 恒收录）、restore 的越界拒绝均对齐（snapshot.rs vs snapshot.ts:90-202）。

**B1** edit 输出缺 `files`
- ref：`tool/plugin/edit.ts:36-39` `Output = {files: FileDiff[], replacements}`；`:208-211` 返回两者。
- rust：`tool/edit.rs:408` `output: Some(json!({"replacements": N}))`——`files` 只进了 content/metadata。
- 影响：叠加 D1 后 API 消费者拿不到结构化 diff（metadata 里仍有 `files`，故降为 S2）。

**B2** read 二进制检查
- ref：`tool/read-filesystem.ts:164-173/205-210` 只检查**已消费跨度**（`first.bytes.subarray(0, result.consumed)`）内是否含 NUL。
- rust：`tool/read.rs:315-318` 对**整个文件**做 `contains(&0)`。
- 影响：NUL 出现在页尾之后的文本文件会被误报 `Cannot read binary file`。

**B3** 建议列表排序
- ref：`tool/plugin/read.ts:152-165` 用 `fs.readDirectory` 原始顺序取前 3 个匹配。
- rust：`tool/read.rs:454-468` 先对目录项**排序**再取前 3。
- 影响：>3 个候选时给出的 "Did you mean" 集合与 v2 不同。

**B4** AGENTS.md 副作用
- ref：`tool/plugin/read.ts:87-107` 以 `fs.resolve`（realpath）后的 start/root 向上走，且用 `FSUtil.contains` 过滤根内文件。
- rust：`tool/read.rs:584-612` 用词法 `target.absolute` 与 `services.directory` 比较。
- 影响：location 目录经符号链接进入时漏注/重注（instructions ledger 去重可兜底部分）。

**B5** read 无分块流
- ref：`tool/read-filesystem.ts:122/176-223` FIRST_CHUNK=256KB 增量读取 + rope 定位。
- rust：`tool/read.rs:278-281` `std::fs::read` 一次读全文件。
- 影响：仅内存峰值差异（超大文件 OOM 风险），模型可见行为一致。

**B7** snapshot enabled
- ref：`snapshot.ts:115` `location.vcs?.type === "git" && state.enabled`。
- rust：`snapshot.rs:241-243` 只有 atomic 开关；vcs 判定依赖构造侧（engine 是否只为 git 项目建 store）。服务层缺自查。

**B8** formatter no-op（见 §1 表；`formatter.rs:110-125` `file()` 恒 false，engine/mod.rs:808 注入 `NoopFormatter`）。v2 默认 config 无 `formatter` 键时 builtins **不注册**（config/plugin/formatter.ts:64-76 `resolve` 返回 undefined 即 return），故默认等价；用户配置 `formatter: true`/逐项覆盖时完全不生效。

## 3. shell / PTY / persistent PTY

| ID | 标题 | 类型 | 严重度 | KNOWN/NEW |
|---|---|---|---|---|
| C1 | stdout/stderr 顺序读 → 交错丢失 + 假死锁 | WRONG | S1 | NEW |
| C2 | `shell.create.before` 插件钩子永不派发 | MISSING | S1 | NEW |
| C3 | config `shell` 键被忽略 | WRONG | S1 | NEW |
| C4 | `experimental.portable_shell_scanner` 硬编码 false | WRONG | S1 | NEW |
| C5 | `experimental.subagent_depth` 硬编码 1 | WRONG | S1 | NEW |
| C6 | shell 工具 `progress({shellID})` 未发布 | MISSING | S2 | NEW |
| C7 | shell 工具 metadata 字段不全 | WRONG | S2 | NEW |
| C8 | 超时击杀无 TERM 宽限期 | WRONG | S2 | NEW |
| C9 | persistent-PTY 守护进程边界（socket/keepalive） | PARTIAL | S2 | KNOWN（ROADMAP R-09、reposcan #127） |
| C10 | PtyService 默认命令解析器未接线 | MISSING | S2 | NEW |
| C11 | 每小时保留清扫任务未启动（tool-output + shell 输出） | MISSING | S2 | NEW |
| C12 | rg 退出码/错误消息映射偏差 | WRONG | S2 | NEW |
| C13 | session env 未按 workspace 门控 | PARTIAL | S2 | NEW |

等价核对项：`DEFAULT_TIMEOUT_MS=120000`、background 默认 timeout 0、`AGENT=1`/`OPENCODE=1`/`AI_AGENT||="opencode"`/`OPENCODE_SESSION_ID` 注入（tool/shell.rs:277-280 vs shell.ts:210-213）、TERM/OPENCODE_TERMINAL（shell.rs:54）、prepare 的目录解析+解析器资源 assert+cwd 存在性即时校验（错误文案逐字对齐）、job 包装/后台化/block→backgrounded 时 `shell.timeout(id,0)`+通知、`<shell …>` 通知封装与 user-notification 前缀（shell/result.rs 全对齐，含 `Exited with code N` 数字格式化）、EXITED_LIMIT=25 驱逐、`Shell.output` 65536 默认页、ID 格式 `sh_<12hex><14 base62>`、`process_group(0)` 分离、scan.rs portable 扫描器 + parse.rs legacy（原生 tree-sitter bash/powershell；ARITY 表与 CWD 集合搬运）、select.rs META/deny/login/args、pty 的 buffer 2MiB/EXITED 25/attach cursor 语义（-1 尾随/越界钳制）/事件负载、persistent-pty 的 handoff 阶梯（过期→注册→协议→实例→锁→票据）与 state.json 快照。

**C1** 输出捕获顺序与死锁
- ref：`shell.ts:325` `handle.all`（stdout+stderr **合并交错**写入捕获文件）。
- rust：`shell.rs:529-551` `for mut stream in streams` —— **先读完 stdout 再读 stderr**。
- 影响：(a) 合并输出顺序与真实交错不同（模型看到的 stderr 位置错误）；(b) 子进程向 stderr 写满管道（≥64KB）而 stdout 保持打开时会互相阻塞——`read_to_end(stdout)` 等 EOF，子进程阻塞在 stderr 写——直到超时纤维 SIGKILL 才解开，命令被误报为 timeout。默认前台 120s 内表现为挂起。

**C2** `shell.create.before` 未派发
- ref：`shell.ts:274` `hooks.trigger("shell","create.before", invocation)`——插件可改写 command/cwd/env/timeout。
- rust：`hooks.rs:890-891` 已定义域与派发函数，但 `Shell::create/create_invocation`（shell.rs:427-595）**从不调用**（仅测试 hooks.rs:1744）。工具自身的 `before` 闭包在 tool/shell.rs 有等价物，但插件 API 断裂。

**C3** config `shell` 键
- ref：`config/plugin/shell.ts` 将 `Config.latest(entries,"shell")` 写入 ShellSelect；shell 服务（shell.ts:267 `priority:"config"`）与 shell 工具（shell.ts:112 `priority:"compat"`，仍读已配置 shell）都会用到。
- rust：全部调用点传 `None`——`tool/shell.rs:281,540,567`、`shell.rs:443`、HTTP `/api/shell` create（handlers/shell.rs:52 经 `create(input,None,…)`)。config.rs 只解析该键（config.rs:853），从不消费。
- 影响：用户配置的 shell 完全无效（始终 $SHELL/回退）。

**C4** portable 扫描器
- ref：`tool/plugin/shell.ts:125` `Config.latest(…,"experimental")?.portable_shell_scanner === true` 切换 `ShellParse.scan` 的 portable 路径。
- rust：5355 行的 `shell/scan.rs` 移植存在，但 engine 硬编码 `portable_scanner: false`（engine/mod.rs:1660）——生产不可达。

**C5** subagent 深度
- ref：`tool/plugin/subagent.ts:129` `Config.latest(…,"experimental")?.subagent_depth ?? 1`。
- rust：helper `subagent::subagent_depth(entries)`（subagent.rs:41-52）已写好但 engine 接线 `depth: Arc::new(|| 1)`（engine/mod.rs:1703）。嵌套子代理配置无效。

**C6** shell 进度
- ref：`tool/plugin/shell.ts:217` `yield* context.progress({ shellID: info.id })`（jobs.start 之前）。
- rust：`tool/shell.rs` 全文无 progress 调用（subagent 侧已接，engine/mod.rs:1715-1731）。TUI 运行中 shell 指示缺 metadata。

**C7** shell 工具 metadata
- ref：`tool/plugin/shell.ts:83-93` `metadata = {status, ...ShellResult.metadata(output), shellID?}`——`ShellResult.metadata`（shell/result.ts:36-43）**恒含** `truncated`，且含 `exit/signal/timeout`（有则）。
- rust：`tool/shell.rs:516-536` `truncated` 仅在 true 时写入；`signal`、`timeout` **从不写入**。
- 影响：TUI/API 的 shell 状态字段缺失。

**C8** 超时击杀
- ref：`shell.ts:306/401` `forceKillAfter: 3s`——TERM，3 秒后强杀。
- rust：`shell.rs:376-388` 立即 `start_kill()`（SIGKILL）+ 进程组 SIGKILL（含 /proc 扫描的安全封装）。无优雅期；进程收不到 TERM 钩子（vite/next 等清理不执行）。

**C10** Pty 默认命令
- ref：`pty.ts:168` `input.command || (yield* shell.resolve({priority:"config"}))`。
- rust：`pty.rs:347-353` 提供 `with_default_command` 接缝，但 `oc-server/src/state.rs:107` `PtyService::new(bus)` 未设置——落入 `$SHELL`/`/bin/bash…` 回退（pty.rs:548-563），config shell 与 ShellSelect 兼容过滤都不参与。

**C11** 保留清扫
- ref：`tool-output.ts:129-144` 与 `shell.ts:83-117` 两个**每小时**清扫（7 天保留；shell 侧清 `<data>/shell/<project>/*.out`）。
- rust：`tool_output.rs:304 spawn_cleanup_task` 定义后**无人调用**（全仓 grep 仅测试）；shell 侧清扫**完全没有**。磁盘无限增长。

**C12** rg 退出码
- ref：`ripgrep.ts:143-151`——截断早返；code 2 且 regex 解析错误→InvalidPattern；code∉{0,1,2}→`stderr.trim() || "ripgrep failed with code N"`；**code 2 非 regex 错误 → 按成功返回 rows**。
- rust：`tool/grep.rs:142-158`——code 2 非 regex → `Err(Failed(stderr))`；code>2 恒 `"ripgrep failed with code N"`（丢弃 stderr 优先）；code 1 返回 rows（ref 返回 `[]`，实际不可达）。

**C13** session env 门控
- ref：`shell.ts:258-262` 仅 `location.workspaceID === undefined && is(SessionID)` 时使用 session 环境，否则 process.env。
- rust：`tool/shell.rs:271` `deps.env.get(...)` 无 workspace 判定。workspace location 中变量集合不同。

## 4. 输出与截断（tool-output / 结构化输出 / fs 搜索）

| ID | 标题 | 类型 | 严重度 | KNOWN/NEW |
|---|---|---|---|---|
| D1 | 工具结构化 `output` 全链路丢弃 | WRONG | S1 | NEW |
| D2 | config `tool_output` 限制键无效 | WRONG | S2 | NEW |
| D4 | `/api/fs/find` 朴素子串遍历替代 fuzzysort 索引 | PARTIAL | S2 | NEW |

等价项：2000 行/50KiB 预算算法（含 `kept` 计数、marker `[showing lines 1-N of M; full output saved to <file>]`、file 部件保位、`truncated:false` 标注、已截断结果直通、尾随换行不算一行、文件名 `tool_<12hex><14base62>` 升序、7 天 mtime 清理逻辑本身）逐行为对齐且测试钉死（tool_output.rs:193-299 vs tool-output.ts:67-118）；runner 在 tool fiber 内结算前调用 truncate（step.rs:645-680 vs step.ts:122）；溢出降级为原文一致。

**D1** 结构化 output 丢弃（本报告最重要发现）
- ref：`tool/runtime.ts:45-59` 对声明了 outputSchema 的工具 encode `output`，`Tool.NormalizedResult` 携带之；`tool.ts:140-155` after-hook 与 normalize 均保留；session 工具部件对外含 `output`。
- rust：`ToolOutcome.output`（tool/mod.rs:215）在 `PipelineExecutor::run` 返回处被**静默丢弃**——`ToolRunResult`（location.rs:361-366）只有 `content`+`metadata`；`session.tool.success`（publisher.rs:893-900）也无 output 字段。各工具辛苦构造的 `output: Some(...)`（edit/write/shell/webfetch/websearch/question/skill/subagent/patch）全部无效。
- 影响：API/客户端与 codemode 取值路径拿不到 v2 的结构化输出（edit 的 files、write 的 existed、shell 的 exit/signal/status、read 的分页结构、subagent 的 sessionID/status…）。TUI 侧以 `tool_output_from_content`（oc-tui/data.rs:411）从 content 反推，说明已在 compensated，但 wire 字段缺失是协议级偏差。

**D2** config `tool_output`
- ref：`config/plugin/tool-output.ts` 调 `ToolOutput` editor.configure；`shell.ts:235-237` 每次 capture 现读 `Config.latest(…,"tool_output")`。
- rust：engine 直接 `ToolOutput::new(&data_root, clock)`（engine/mod.rs:444）从不 `configure`；shell capture 用编译期常量（shell.rs:296-297）。`oc-schema/src/config/tool_output.rs` 已解析该键。

**D4** fs find
- ref：`filesystem/search.ts`——ripgrep `find` 维护 files/directories 索引（10s 刷新节流），`fuzzysort.go(query, …, {limit: 50})` 模糊排序；`filesystem/fff*` 为索引后端。
- rust：`oc-server/src/handlers/fs.rs:253-315`——查询时全树 walk + 小写**子串**匹配，默认无 limit（不传则不设上限），跳过 `.git/node_modules/target`（ref 用 ignore 规则）。
- 影响：@ 文件补全的排序质量与性能（每次全树遍历 vs 增量索引）；子串 vs fuzzy 的命中集不同。

## 5. 权限与错误

| ID | 标题 | 类型 | 严重度 | KNOWN/NEW |
|---|---|---|---|---|
| E1 | patch 权限 metadata `filepath` 顺序/去重不同 | WRONG | S2 | NEW |
| E2 | webfetch 权限 metadata 不含完整输入 | WRONG | S2 | NEW |
| E3 | websearch 表单缺 `kind` metadata | WRONG | S2 | NEW |
| E4 | patch 写入自动建父目录（ref 会失败） | WRONG | S2 | NEW |
| E8 | webfetch 超时/HTTP 错误文案 | WRONG | S2 | NEW |

等价项（抽核）：edit/write/patch 的 `action:"edit"`、`save:["*"]`、preview 进 metadata 的顺序；read 的 `authorizeRead`（sibling 共享目录审批但独立 read 规则）；glob/grep 的 `action=name`、resources=[pattern]、metadata 形状（root/path/hidden|include/literal/caseSensitive/limit——glob rust 侧 `limit` 原样传 `Option`，ref 传 `input.limit` 可能 undefined，JSON 序列化后等价）；shell 的解析器资源 assert（resource=命令文本/save=前缀通配）；subagent 的 `action:subagent, save:[agent.id]` 与 `Subagent denied:` 包装；question 的 `resources:["*"]`；websearch 的 `save:["*"]`+`metadata:input`；外目录 `external_directory` 批量审批（按 resource 去重保序、save 锚到项目根）；失败消息的 blanket + 嵌套解包（`FailureCause` 分类、decline 隧道为中断）与 `toSessionError` 规则——均与 v2 一致且有测试。权限引擎本身在 permission lane 另审。

**E1** `filepath` 元数据
- ref：`tool/plugin/patch.ts:196-204` `filepath: targets.map(t=>t.resource).join(", ")`——**遇见序、含重复**。
- rust：`tool/patch.rs:358-369` sort+dedup 后 join。审批 UI 展示的文件列表不同。

**E2** webfetch metadata
- ref：`tool/plugin/webfetch.ts:124-132` `metadata: input`（含 url/format/timeout）。
- rust：`tool/webfetch.rs:168-173` 仅 `{url, format}`。

**E3** websearch 表单 metadata
- ref：`tool/plugin/websearch.ts:78/111` `metadata:{kind:"websearch.provider"}`。
- rust：`tool/websearch.rs:314/383` `metadata: None`。客户端无法按 kind 识别该表单。

**E4** patch 父目录
- ref：`environment/files.ts:51-54` 的 write 是裸 `fs.writeFile`（父目录缺失 → `Failed to write …` 报错进部分失败链）。
- rust：`tool/patch.rs:438-446` 写前 `create_dir_all`。rust 更宽容——非法路径（深层新增）在 v2 会以部分失败上报。

**E8** webfetch 错误文案
- ref：超时 `Request timed out`（webfetch.ts:148）；非 2xx 为 effect `StatusCodeError` 文本。
- rust：reqwest 超时/状态错误字符串（`tool/webfetch.rs:110` `"HttpClientError: {status}"`）。外层 blanket `Unable to fetch {url}` 相同，嵌套 cause 文本不同。

## 6. 其它备注（不计入差异）

- **R（registry 输入校验）**：ref 用 effect/zod 标准 schema 全量校验；rust `registry.rs:88-208` 是自研 JSON-Schema 子集（type/required/enum/minLength/minimum/items），`x-opencode-message` 支持自定义消息。对内置工具所用关键字覆盖足够；第三方/MCP 工具 schema 的其它关键字（anyOf/oneOf/pattern/additionalProperties=false 等）会被**静默放过**——MCP 工具的参数校验宽松于 v2（S2 观察，属 MCP lane 边界，此处仅登记）。
- `tool/shell.rs:561-606 user_command`（`!` 命令路径）为 ref `session.ts` shell 模式的等价物，非 EXTRA 差异。
- subagent：`execute.before` 空 `model`/`sessionID` 剥离、描述钩子（context/compaction/generate）、progress→`session.tool.progress`、前台 block 的中断守卫（Drop 火烧 child interrupt+cancel）均已接；`deps.interrupt` bell 为 None（engine/mod.rs:1732），中断只靠 future-drop 路径——行为可达，列为观察项。
- shell `create` 的 env：rust `env_clear()+envs(map)` 与 ref 传完整 env 对象等价（map 本身就是 session/process env）。
- `session.tool.success` 的 `executed`（providerExecuted）字段、settle 顺序与 v2 step.ts 一致（runner lane 已钉）。

## 7. 覆盖清单（审计到位率）

| 区域 | ref 文件数 | 已逐一比对 | 备注 |
|---|---|---|---|
| tool.ts + runtime.ts | 2 | ✅ | A6/D1 |
| 15 个 plugin 工具 + file-diff + patch.txt | 17 | ✅ | 全部读过并对照 |
| tool-output.ts | 1 | ✅ | D2/C11 |
| file-access/file-mutation | 2 | ✅ | 等价 |
| filesystem.ts + filesystem/（9 文件） | 10 | ✅（fff 3 个绑定壳仅确认无对应） | D4；watcher 结论=等价（v2 只 watch HEAD） |
| snapshot.ts | 1 | ✅ | B7 |
| shell.ts + shell/（parse/scan/select/result + 3 个 wasm 壳） | 8 | ✅（scan.ts 2409 行抽查结构/ARITY/出口语义） | C1/C2/C12 |
| pty.ts + pty/（5 文件） | 6 | ✅ | C10 |
| persistent-pty/（6 文件） | 6 | ✅（daemon.ts 690 行按 handoff 阶梯核对文档+结构） | C9 |
| ripgrep.ts + binary.ts | 2 | ✅ | A5 |
| websearch.ts + plugin/websearch/（6） | 7 | ✅ | A9/E3 |
| formatter.ts + builtins + config plugin | 3 | ✅ | B8 |
| image.ts + image/（4） | 5 | ✅（确认无对应） | A6 |
| rust 侧反向：tool/\*、shell/\*、pty、persistent_pty、snapshot、formatter、tool_output、file_access/mutation、engine 接线点、server handlers/fs+shell、announce | — | ✅ | 发现集中在接线层 |

未深入（声明）：`tool/codemode.rs` 的 JS 运行时行为（AI 协议 lane）、`tool/html_markdown.rs` 2000 行转换器的逐例对齐（仅核常量与抽样用例）、`shell/scan.rs` 5355 行的逐规则对齐（结构与出口语义核对）、`patch_util.rs` 补丁语言文法逐例、jobs.rs 恢复记录持久化细节（jobs lane）。

## 8. 建议优先级（只读建议，不实施）

1. **S1 接线批**（一处 engine 改动各自很小）：C3/C4/C5（config shell / portable_shell_scanner / subagent_depth 三个键接到既有 seam）、A1（BuiltinTool 增加 namespace 字段或让 opencode 工具走 mcp_resource 式 register）、D1（ToolRunResult 增加 output 字段并贯通 publisher）。
2. **S1 行为批**：C1（合并流：两个 reader 并发汇入同一 writer，或 select! 双流）、C2（create_invocation 派发既有 hook）、A2（session_move 接 Move_ inbox item，engine 已有 move_session）。
3. **S2 清扫批**：C11（启动两个 hourly 任务）、C7/C6/E1/E2/E3（字段补齐）、B2/B3、A7/A8、D2。
4. **结构性（另行立项）**：A5（rg 下载器）、A6（photon 或 rust 图像库）、B8（formatter 执行器 + builtins 表）、C9（phase-9 daemon）、D4（索引式 find）。

# L2 文件类工具 (Tools: files) — 审查报告

> 审查方式：两侧逐文件逐行阅读，结论均以代码为准（不信任文档）。证据格式 file:line。
> 参照：/opt/opencode-ref (v2.0.24 TS) · 移植：/opt/opencode-rust

### 1. 覆盖证明

**Ref 逐行读完**：`tool/plugin/` 下 read.ts(211)、write.ts(101)、edit.ts(230)、patch.ts(363)、glob.ts(139)、grep.ts(170)、file-diff.ts(23)；`tool/read-filesystem.ts`(402)、`tool.ts`(332)、`tool-output.ts`(150)、`ripgrep.ts`(275)、`packages/util/src/patch.ts`(381)、`environment/local.ts`(write/remove 语义)；`filesystem.ts` grep 确认 `DEFAULT_SEARCH_LIMIT=100`/`TIMEOUT=30_000`；`fs-util.ts` `contains`/`up` 语义。无独立 list/multiedit 工具（read 兼任目录列举）。

**Rust 逐行读完**：`oc-core/src/tool/` 下 read.rs(993)、write.rs(146)、edit.rs(445)、patch.rs(609)、patch_util.rs(700)、glob.rs(192)、grep.rs(531)、file_diff.rs(341)、mod.rs(353)、registry.rs(874)、ripgrep_runner.rs(前 120 行+结构)、`../tool_output.rs`(704)；`session/runner/step.rs` grep 确认 truncate/normalizeImages 接线；`environment/local.rs|files.rs` grep 确认 write/write_file/remove 语义；patch.txt 两侧 `diff` 相同。

### 2. 逐项对比表

| 功能点 | Ref 实现 (file:line) | Rust 实现 (file:line) | 状态 | 差异说明 |
|---|---|---|---|---|
| read 参数 schema | read.ts:17-25 (path/offset/limit, NonNegativeInt) | read.rs:33-43 | ✅ | 描述文本逐字一致；0 值 falsy→默认值两侧均模拟 (read.rs:432-435,491-494) |
| read 目录列举(排序/分页) | read-filesystem.ts:258-289 (目录优先+path 排序, `name/` 后缀, next) | read.rs:192-229 | ✅ | 排序键与 truncated/next 公式一致 |
| read 文本分页 (256KiB chunk/rope/50KiB/2000行) | read-filesystem.ts:116-224,291-324 | read.rs:405-605 | ✅ | textPage/clampLine(2000字符)/consumed/NUL 二进制检测均逐条对应 |
| read offset 越界错误 | read-filesystem.ts:43-50,246-247 | read.rs:94-97,385-386 | ✅ | 消息逐字一致 |
| read 媒体 (5 mime/20MiB/base64) | read-filesystem.ts:21,136-149 | read.rs:20,25-26,126-157,501-521 | ✅ | BMP/AVIF 检测到但不摄取，与 ref MEDIA_MIMES 门控一致 |
| read 图片→模型 file part | read.ts:175-185 | read.rs:958-974 | ✅ | "Image/PDF read successfully" + data: URI |
| read 未找到→建议/替代路径(NFC+排版规范化) | read.ts:138-171 | read.rs:769-842 | ✅ | 唯一匹配 sibling、3 条建议、"Did you mean"文案一致 |
| read AGENTS.md 副作用 | read.ts:87-107 | read.rs:917-947 | 🟡 | 语义等价(向上走到 root 排除、排除自身)；ref 经 fs seam+realpath resolve，Rust 用 std::path 词法父遍历且 `is_file()` 绕过 Files seam（workspace 驱动环境下可能漏/多发现） |
| read 输出格式 `{n}: {text}` | read.ts:200-210 | read.rs:662-726 | ✅ | 0 lines/entries、truncated 提示逐字一致 |
| write schema/权限/父目录 | write.ts:23-39,59-92 | write.rs:18-39,57-122 | ✅ | authorizeExternal→edit assert(带 diff 预览)→BOM 保留写→formatter→BOM resync；"Wrote/Created file successfully"一致 |
| write 输出 {operation,target,resource,existed} | write.ts:30-39 | write.rs:132-137 | ✅ | |
| edit 三级匹配 (exact→unicode→line) | edit.ts:49-102,163-168 | edit.rs:17-152,319-337 | ✅ | normalizeForMatch 字符表、trimEnd、重叠去重、倒序替换均对应；pass2 字节偏移回映正确 |
| edit CRLF 归一/唯一性/replaceAll | edit.ts:159-162,189-198 | edit.rs:315-317,373-382 | ✅ | 主导换行符判定、两条失败消息逐字一致 |
| edit 先 permission 后失败判定 | edit.ts:180-198 | edit.rs:352-382 | ✅ | 顺序刻意保持 |
| edit 锁/BOM/formatter 回读失败即失败 | edit.ts:199-213,219 | edit.rs:242-258,397-415 | ✅ | G-267② 修复对齐 |
| edit 输出/metadata | edit.ts:36-39,214-218 | edit.rs:417-438 | ✅ | "Edited X (n replacement(s))"、files metadata、output{files,replacements} |
| patch 语言解析 (Begin/End/Add/Delete/Update/Move to/End of File/heredoc) | util/patch.ts:49-126,143-300,381 | patch_util.rs:80-409 | ✅ | 全部错误文案、`*** Environment ID:` 跳过、CRLF 剥离、heredoc 尾随空白容忍逐字对应 |
| patch derive (exact→rstrip→trim→normalized 阶梯/eof/空行回退) | util/patch.ts:128-137,311-360 | patch_util.rs:411-531 | ✅ | seek 每级扫全表、排序后倒序 splice、补尾换行、BOM 保留 |
| patch 执行 (准备链/updates 累积/单次 assert/顺序写/部分失败) | patch.ts:102-279 | patch.rs:219-556 | ✅ | 资源 Set 去重、filepath/diff/files metadata、"Completed before failure" 一致 |
| patch move-to-new-dir 父目录 | patch.ts:227-229 (`environment.files.write`→local.ts:51-55 **mkdir -p**) | patch.rs:431-433 `write_file`(**不建父目录**) | ⚠️ | ref move 腿也自动建父目录；Rust 显式不建（注释自认差异），move 到新目录会失败 |
| patch delete | patch.ts:214-223 (`files.remove`=rm -r force) | patch.rs:405-424 (`remove_file` 非递归) | 🟡 | 目标必为文件时等价；ref force 容忍 NotFound，Rust 报错 |
| patch gpt-* codemod 钩子 | patch.ts:296-309 | patch.rs:443-445,565-608 | ✅ | `use_patch_tool` 规则逐字一致；context/compaction/generate 三点 |
| file-diff (createTwoFilesPatch/diffLines) | file-diff.ts:4-23 + `diff` 包 | file_diff.rs:26-237 | ✅ | 4 行上下文、hunk 合并、`\ No newline`、`,1` 显式计数；测试逐字钉死 |
| glob (rg --files 参数/hidden/!**/.git/limit+1) | glob.ts:62-119, ripgrep.ts:165-192 | glob.rs:74-184, grep.rs:212-231 | ✅ | 参数构造逐项一致；"undefined"/"null" path 降级一致 |
| glob 输出/metadata | glob.ts:34-42,119-128 | glob.rs:61-72,154-184 | ✅ | Entry[](location 相对路径)+count/truncated+截断提示 |
| grep (rg --json 参数映射) | grep.ts:111-120, ripgrep.ts:219-234 | grep.rs:234-298,447-465 | ✅ | fixed-strings/ignore-case/include/file-vs-dir、`--`、exit 1/2 容忍策略一致 |
| grep 2000 UTF-16 截断/100 submatch | ripgrep.ts:20,261 | grep.rs:23-35,270-288 | ✅ | UTF-16 码元数学刻意复刻 |
| grep 超时 30s | glob.ts:101-109, grep.ts:122-130 | grep.rs:172-178 | ✅ | 消息一致 |
| grep InvalidPattern→"Invalid regex pattern" | grep.ts:158-163, ripgrep.ts:145-147 | grep.rs:467-470 | 🟡 | Rust 消息含 rg 原始 stderr（ref 也是 stderr.trim()），一致 |
| grep 输出分组/metadata | grep.ts:44-61,145-157 | grep.rs:352-384,482-523 | ✅ | `path:` 空行分组、`  Line n:`、Match[] output |
| Tool 基类/注册校验/有效名 | tool.ts:26-33,304-326 | registry.rs:26-70 | ✅ | 名称正则、namespace 段校验、"execute" 保留 |
| 执行协议 (before→decode→execute→after, 错误腿 after 后返回) | tool.ts:103-156, runtime.ts | registry.rs:240-482 | ✅ | 含插件重写 error/result、空 content→stringify(output) |
| 输出 schema 校验 (encodeOutput) | runtime.ts:45-56,110-132 | registry.rs:333-378 | 🟡 | 已实现但 builtin 均未声明 output schema（ref 声明了，Rust 走 accept-all 回退——自述性偏离） |
| normalizeImages | tool.ts:74-101,150 | step.rs:738-744 (`image::normalize_tool_content`) | 🟡 | 移到 step 层；且顺序为 truncate→normalize，ref 为 normalize→truncate（影响截断字节数计算的对象） |
| tool-output 截断/溢写文件/UTF-16 预算 | tool-output.ts:13-15,67-118 | tool_output.rs:74-331 | ✅ | 标记文案、7 天保留、每小时清理、`tool_<12hex>` 命名与清理正则等价 |
| 工具结果事件 (execute.before/after hooks) | tool.ts:103-149 | registry.rs:246-265,398-413 | ✅ | |
| list 工具 | 不存在（read 兼任） | 不存在 | ✅ | 两侧一致 |
| ripgrep 二进制获取 | ripgrep/binary.ts | ripgrep_runner.rs:1-120 | ✅ | 版本 15.1.0、平台表、OPENCODE_RG_PATH 优先（🔵 另加全局缓存与受控自动下载） |

### 3. 关键差异与风险 Top 5

1. **⚠️ patch move 不创建父目录**：ref `environment.files.write` 统一 `mkdir -p`（local.ts:51-55），patch.ts:227-229 move 腿同样受益；Rust patch.rs:431-433 特意用 `write_file`（files.rs:101 无父目录创建）并注释为设计。`*** Move to: new/dir/x.rs` 在父目录缺失时 ref 成功、Rust 失败——行为分歧且注释与 ref 语义相悖。
2. **🟡 read 的 AGENTS.md 发现绕过 Files seam**：read.rs:930-941 用 `std::path::parent()` + `Path::is_file()`，而 ref（read.ts:89-100）经 `fs.resolve`/`fs.up`（异步 env seam，跟随 symlink）。workspace/远程驱动环境下 Rust 会直接查宿主 FS；本地环境等价。
3. **🟡 normalizeImages 与 truncate 的顺序**：ref tool.ts:150 先 normalize 再由 step 截断；Rust step.rs:711-744 先 truncate 再 normalize。含 data-URI 图片重编码的极端场景下，50KiB 预算的判定对象不同。
4. **🟡 builtin 工具未声明 output schema**：ref 每个插件工具声明 `Output` 并在 runtime 校验；Rust registry.rs:159-234 自述 14 个 builtin 走 accept-all 回退，坏 output 不会在开发期暴露（风险低但防护缺）。
5. **🟡 patch delete 用 `remove_file` 而非 rm-force**：ref `files.remove`（recursive+force，容忍 NotFound）；Rust 对 NotFound/目录报错。正常路径等价，并发删除/符号链接目录场景错误面不同。

### 4. 缺失文件/功能汇总

- **无缺失文件**：六工具 + patch 解析器 + file-diff + tool-output + 注册/执行协议 + ripgrep runner 全部有对应实现；list/multiedit 两侧均不存在。
- 功能层面缺口仅为上表 🟡 项：builtin output schema 声明未移植、read AGENTS.md 发现未走 Files seam、patch move 父目录行为分歧、delete 的 rm-force 语义、normalize/truncate 顺序。其中仅第 1 项（patch move 父目录）构成可复现的用户可见行为差异。

# L7 文件系统 / VCS / Snapshot — 审查报告

> 审查方式：两侧逐文件逐行阅读，结论均以代码为准（不信任文档）。证据格式 file:line。
> 参照：/opt/opencode-ref (v2.0.24 TS) · 移植：/opt/opencode-rust

### 1. 覆盖证明
逐行读完的 Ref 文件：`filesystem.ts`(211)、`filesystem/search.ts`(202)、`protected.ts`(57)、`file-access.ts`(167)、`file-mutation.ts`(134)、`file-retention.ts`(23)、`snapshot.ts`(233)、`git.ts`(758)、`vcs.ts`(260)、`ripgrep.ts`(275)、`formatter.ts`(104)、`formatter/builtins.ts`(297)、`image.ts`(95)、`image/photon.ts`(108)、`mime.ts`(33)；worktree/vcs-git 插件侧以 `worktree/*.ts`、`git.ts` 内 worktree 段与 handlers 对照。
逐行读完的 Rust 文件：`oc-server/src/handlers/fs.rs`(1162)、`oc-core/src/filesystem/{mod,protected,watcher,location_watcher}.rs`、`file_access.rs`(842)、`file_mutation.rs`(539)、`file_retention.rs`(101)、`snapshot.rs`(1061)、`project.rs` worktree-strategy 段(960-1140)及 handlers/project.rs VCS 段(1240-1860)、`reference/repository_cache.rs`(clone/fetch/checkout/reset 段)、`tool/grep.rs`(参数段)、`tool/ripgrep_runner.rs`(头部)、`formatter.rs`(1-900+执行段)、`image.rs`(572)。grep 全仓确认 git/vcs/worktree/ripgrep 无其他实现。

### 2. 逐项对比表

| 功能点 | Ref 实现 (file:line) | Rust 实现 (file:line) | 状态 | 差异说明 |
|---|---|---|---|---|
| fs.read 双重围栏（lexical→realpath 对 root） | filesystem.ts:132-139 | handlers/fs.rs:127-149 | ✅ | lexical_resolve+canonicalize+contains 逐语义对齐；越权→500、缺失→404 一致 |
| fs.read 非文件 die | filesystem.ts:159 | fs.rs:172-175 | ✅ | |
| fs.read mime 推断 | mime.ts:3-33（经 FSUtil.mimeType） | fs.rs:34-81,197-214 | ✅ | 魔数表+isText(30% 控制字符/UTF-8/NUL) 一致；BMP/AVIF 均含 |
| fs.list（相对路径、目录尾 `/`、目录优先） | filesystem.ts:172-195 | fs.rs:246-281 | ⚠️ | 见风险③：symlink 条目与排序差异 |
| fs.write 故意越界写 | filesystem.ts:196-202 | fs.rs:920-961 | ✅ | path 必填 400（G-220）、create_dir_all+write |
| find 索引（10s 刷新、首扫 await、single-flight） | search.ts:65-114 | fs.rs:686-913 SearchIndex | ✅ | 语义等价；另加空闲驱逐/30s await 上界（新增，安全侧） |
| find 预算（VCS 根 MAX_SAFE_INTEGER 否则 100k） | search.ts:72 | fs.rs:312,389,415-417 | ✅ | VCS 根判定走 project resolver（T1），保守等价 |
| find 隐藏文件/`.git`/node_modules 排除 | ripgrep.ts:198-206 | fs.rs:541-543,453 | ✅ | rg 无 --hidden ⇒ 点开头全部跳过，一致 |
| find .gitignore 遵循 | ripgrep.ts（rg 默认） | fs.rs:550-555（git2 ignore oracle） | 🟡 | 已记录分歧：仓库外的裸 .gitignore/.ignore 不生效（rg 生效） |
| find 保护目录（home 名单/绝对路径，平台门控） | protected.ts:39-55 + search.ts:73 | protected.rs:96-163 + fs.rs:426-465 | ✅ | darwin/win32 表逐项一致，linux 空表一致 |
| find 排名（fuzzysort/fff） | search.ts:39,116-182 | fs.rs:326-352,474-500 | ⚠️ | 贪心子序列近似 fuzzysort（文档已声明）；fff 原生层未移植 |
| fff 原生索引层 | search.ts:116-196 | 无（grep 全仓确认） | 🔴 | VCS 非 home 位置 REF 走 fff；Rust 统一走自建索引。可观察契约（排除/预算/刷新）已匹配，排名算法为近似 |
| FileAccess.resolve（~/展开、lexical、internal=location∪worktree、save 锚 project root） | file-access.ts:67-123 | file_access.rs:100-244 | ✅ | 含 fs-root worktree 不作边界的特例 |
| authorizeExternal 去重批量断言 / authorizeRead sibling | file-access.ts:125-161 | file_access.rs:249-312 | ✅ | |
| FileMutation 事务锁（resolve+sort+右起嵌套）/per-target 锁 | file-mutation.ts:60-78 | file_mutation.rs:267-298 | ✅ | 全局表+排序防死锁一致 |
| write / writeTextPreservingBom | file-mutation.ts:87-118 | file_mutation.rs:304-345 | 🟡 | BOM 语义一致；existed 探测 ref 用 stat(跟链)、Rust 用 Files.stat(lstat)，悬空 symlink 下 existed 判定不同（边界） |
| FileRetention.cleanup（mtime<cutoff 删） | file-retention.ts:6-23 | file_retention.rs 仅设置；实际清扫在 tool_output.rs:358（mtime 比对） | ✅ | 语义等价；Rust 另把 interval 配置化（新增） |
| watcher / location-watcher（事件归并、HEAD/branch 监听、TTL 触活） | watcher.ts:1-273, location-watcher.ts, policy.ts | filesystem/watcher.rs(338), location_watcher.rs(497, 含 policy) | ✅ | 模块声明对齐 v2.0.24，归并/policy 移植 |
| Snapshot 启用门（vcs==git && enabled） | snapshot.ts:115 | snapshot.rs:415-425 | ✅ | 以 discover 到 git 仓库作 type==git 等价物 |
| Snapshot 存储布局 `<data>/snapshot/<projID>/sha1(realpath(worktree))` | snapshot.ts:94-99 + git.ts:345-382 | snapshot.rs:342-372,465-480 | ✅ | bare 仓库+alternates 指向源 objects；config 隔离（autocrlf/quotepath）等价 ref 的 opencode.gitconfig |
| capture（tracked 全收、untracked 源忽略规则、>2MiB 跳过、git add --all stat 跳读） | git.ts:384-493 + snapshot.ts:117-132 | snapshot.rs:507-618（自走目录+stat-cache 边车+symlink blob 120000/可执行位） | 🟡 | 实现换轨：CLI git index → 自实现遍历；语义等价（含 tracked 优先于 ignore、symlink 存 target、2MiB 仅限 untracked）；REF 的 refresh 会 `rm --cached` 剔除被忽略文件，Rust 重建全树天然剔除 |
| capture 并发（per-gitDirectory 锁） | git.ts:170-172 | snapshot.rs:98-100,386-413 | ✅ | 进程级锁+自驱逐 |
| files(from,to)（--no-renames 双侧、源 ignore 过滤） | snapshot.ts:155-162 + git.ts:495-511 | snapshot.rs:640-686（树映射 diff+filter_ignored） | ✅ | |
| diff（context 默认3、二进制→空 patch+0/0、10MiB 总量截断、超限 emptyPatch） | snapshot.ts:164-183 + git.ts:518-576 + vcs/patch.ts | snapshot.rs:691-776 | ⚠️ | 见风险②：patch 文本由自研 file_diff 生成，与 `git diff` 输出（hunk 头、`\ No newline`、binary 头、jsdiff 式 emptyPatch）存在格式差异风险；capped 语义已对齐 |
| restore（含 escape 校验、缺树删除、filemode/symlink/755） | snapshot.ts:185-202 + git.ts:578-623 | snapshot.rs:779-857 | 🟡 | escape 校验、120000/100755、absent→递归删一致；非 blob 树条目：REF 报 `Invalid tree entry`，Rust 当 absent 删除（边界分歧） |
| Git.repo.discover（.git 向上+rev-parse 三元组） | git.ts:174-191 | project.rs:1283-1323 resolve_git（git2）+ handlers | ✅ | |
| Git.repo.clone（--depth 100、--branch、--） | git.ts:246-268 | repository_cache.rs:24,250-262 | ✅ | 默认深度 100 一致 |
| Git.remote.get | git.ts:193-197 | repository_cache.rs:207-208 | ✅ | |
| history head/branch/defaultRemoteBranch/rootCommits | git.ts:199-228 | handlers/project.rs:1112-1205；project.rs:1544-1690(rootCommits, git2 等价 oid) | ✅ | |
| sync fetchRemotes(--all --prune)/fetchBranch(+spec)/checkoutRemoteBranch(-B)/resetHard | git.ts:270-299 | repository_cache.rs:272-320 | ✅ | |
| worktree create/remove/list（--detach、common-dir 执行、dirty→forceRequired、porcelain 首项 main） | git.ts:649-694 | project.rs:964-1095 + handlers/project.rs:20-22,190-291 | ✅ | strategy 注册表对应 worktree/strategies.ts |
| index.ignored（check-ignore --no-index） | git.ts:438-473 | snapshot.rs:490-492（git2 is_path_ignored） | ✅ | |
| Vcs info/branches/status/diff（provider 化、MAX_TOTAL_PATCH_BYTES、PATCH_CONTEXT_LINES=INT_MAX、jsdiff emptyPatch、branch 变更事件） | vcs.ts:128-251 + vcs/patch.ts + plugin/vcs/git.ts | handlers/project.rs:1252-1321,1330-1336,1788-1855 | ✅ | jsdiff emptyPatch 字符串逐字对齐；hg 适配（空分支表/status=diffAgainst）一并移植 |
| ripgrep 封装 find/glob/grep（参数链、limit+1 截断、exit 0/1/2、InvalidPattern、路径规范化、2000 截断、100 submatch） | ripgrep.ts:93-271 | tool/grep.rs:211-294 + glob.rs | ✅ | 参数与限流逐项一致 |
| ripgrep 二进制获取（版本 15.1.0、平台表、下载解压） | ripgrep/binary.ts | tool/ripgrep_runner.rs:14-118 | ✅ | 另加 OPENCODE_RG_PATH/禁下载开关（新增） |
| Formatter 服务（extension 匹配、$FILE、cwd、env、exit0→true、失败试下一个、WeakMap 缓存仅命中） | formatter.ts:48-94 | formatter.rs:385-489,529-778,803-858 | ✅ | 26 个内置 formatter 名称/扩展/argv/顺序逐字节一致；日志缺失→静默（已声明） |
| Formatter 检测（package.json 依赖、biome/clang/ruff/air/uv/ocamlformat/pint 探测、Npm.which 缓存代+自动安装） | builtins.ts:38-268 | formatter.rs:529-778（find_up/which/npm_which/probe） | ✅ | Arborist→bun/npm 安装为等价换轨 |
| Formatter 配置解析（布尔重置/对象合并/无 key 不注册） | config/plugin/formatter.ts | formatter.rs:428-489 | ✅ | |
| Image.normalize（2000/2000/5MiB 默认、ladder ×0.75 floor 32 级、PNG→JPEG[80,85,70,55,40]、base64 预算 ⌈n/3⌉×4、错误文案逐字） | image.ts:58-93 + photon.ts:33-107 | image.rs:133-236 | ✅ | photon→image crate Lanczos3；仅 PNG/JPEG 可解码与 photon 一致 |
| normalizeImages 工具内容降级（省略注释文案） | tool.ts:74-95（photon 引用） | image.rs:246-299 | ✅ | |
| MIME 推断 | mime.ts:3-33 | fs.rs:34-81 + image.rs 内 | ✅ | |

### 3. 关键差异与风险 Top 5
1. **find 索引换轨**（fs.rs:514-571 vs ripgrep.ts/fff）：① 仓库外裸 `.gitignore`/`.ignore` 不被咨询（rg 会）；② 空目录被收录为 directory 候选——REF 目录集合只来自文件路径的父目录（search.ts:77-82），空目录永不出现；③ 排名为贪心近似而非 fuzzysort/fff。前两条是可观察行为差异。
2. **snapshot diff 的 patch 文本**：REF 是 `git diff` 原文 + jsdiff 式 `emptyPatch`；Rust snapshot.rs:748-771 用自研 `tool::file_diff` 生成（VCS 处的 empty_patch 已按 jsdiff 逐字对齐，但 snapshot 处走的是另一实现 two_files_patch_ctx）。超预算/无 patch 时的头部格式可能两侧不一致，影响 UI diff 渲染与 golden 对比。
3. **fs.list 细节**（fs.rs:246-281）：REF 跳过 dirent 类型非 file/directory 的条目（symlink 在列），Rust 把 symlink 计为 file 列出；排序用 `to_lowercase` 比较 vs REF `localeCompare`（大小写混合/非 ASCII 序有差）。
4. **snapshot restore 非 blob 条目**：REF `hasEntry` 校验失败抛 `Invalid tree entry`（git.ts:587-593），Rust 直接按 absent 递归删除（snapshot.rs:796-840）——树 ID 被误指到子树时会静默删文件，风险方向更危险。
5. **capture 换轨的 ignore 语义**：Rust 用 libgit2 `is_path_ignored` + 源 index 的 tracked 判定，等价于 check-ignore，但 REF 的 staged 索引跨会话持久（拷贝源 index），Rust 每次全量重走 + stat 边车——首次/大仓库成本与 `.gitignore` 变更时机的可见性略有差异。

### 4. 缺失文件/功能汇总
- **fff 原生索引层**（ref `filesystem/fff*.ts`、search.ts:116-196）：未移植，以自建 walk 索引替代（契约级等价、排名近似、两项排除语义分歧见上）。
- **Git 单一服务**不存在对应模块，但全部 22 个接口函数在 project.rs / handlers/project.rs / repository_cache.rs / snapshot.rs 找到等价实现，无缺失项。
- 其余域内文件均有对应实现，无整文件缺失。

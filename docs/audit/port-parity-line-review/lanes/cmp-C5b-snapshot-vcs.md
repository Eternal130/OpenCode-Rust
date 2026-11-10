# C5b 结论: snapshot/file-access/file-mutation/job 内核为高保真移植（含 ref 的 cancel-persist 怪癖与 BOM/锁语义），但 `snapshots:false` 配置键在 Rust 无消费者（已证 ref ConfigSnapshotPlugin 确为开关）、VCS status/diff 的作用域与统计口径偏离 ref、快照仓库为 git2 直建而非 ref 的 CLI 管线（语义等价、个别边缘不同）。

## 对比表

| ref（file:line） | rust（file:line） | 判定 |
|---|---|---|
| 影子仓库路径 `global.data/snapshot/<project.id>/<Hash.fast(realPath(worktree))>` snapshot.ts:94-97 | snapshot.rs:354-358（`canonicalize` 后 sha1，data_root.join("snapshot")…） | ✅ 一致（util/hash.ts:5 = sha1 ↔ oc_util::hash::sha1_hex snapshot.rs:124-126） |
| 仓库存在判定 `HEAD` 存在即复用，否则 `repo.create`（seed: alternates+index 拷贝）snapshot.ts:98-102, git.ts:345-382 | `open_bare`/`init bare` + `apply_alternates`（写文件 + add_disk_alternate），无 index 拷贝，改用 stat-cache sidecar snapshot.rs:427-480, 526-615 | ✅ 语义等价（有界偏离，注释已声明 G-219） |
| 配置隔离 `opencode.gitconfig` include（autocrlf=false/longpaths/symlinks/untrackedCache/index v4）git.ts:23-38 | `apply_snapshot_config` 仅写 `core.autocrlf=false`、`core.quotepath=false` snapshot.rs:319-327 | ≈ 子集（见 D-6） |
| capture 门 `location.vcs?.type==="git" && enabled`，失败降级 undefined snapshot.ts:115-131 | `enabled_gate = enabled && source_repo().is_some()` snapshot.rs:423-425, 960-974 | ✅ |
| scope=relative(worktree, location)，越界报 "Location is outside the project" snapshot.ts:108-113 | `location_scope` 返回 None ⇒ 调用方兜底 `"."` engine/mod.rs:1806-1807, snapshot.rs:1016-1022 | ⚠ D-1 |
| refresh：scope 内 diff-files+ls-files others、ignored 过滤、>2MiB untracked 跳过、rm --cached+add --all git.ts:384-436 | 直接 fs 遍历 scope：tracked 必收、ignored-untracked 跳过、>2MiB untracked 跳过（MAX_UNTRACKED_FILE_BYTES）snapshot.rs:535-612 | ✅ 语义对齐（walk vs index 刷新） |
| 并发：`KeyedMutex(gitDirectory)` 包 capture/restore（读侧 tree.files/diff 未包）git.ts:170-172, 479-493, 596-623 | 进程级 `REPOSITORY_LOCKS`（DashMap+自驱逐）包 capture/files/diff/restore snapshot.rs:98-100, 386-413 | ✅（rust 更严：读侧也串行） |
| files：tree diff --name-only --no-renames，源仓库 check-ignore 过滤 git.ts:495-511, snapshot.ts:155-162 | `tree_diff_paths` 双 map 对比 + `filter_ignored`（libgit2 is_path_ignored）snapshot.rs:640-686 | ✅ |
| diff：3 批命令、context??3、binary→空 patch、10MiB 总额、paths 空选→[] git.ts:518-576, snapshot.ts:164-183 | `diff_blocking`：blob 对比 + file_diff_ctx、binary NUL 嗅探、MAX_TOTAL_PATCH_BYTES、`Some([])`→[] snapshot.rs:691-776 | ✅ |
| restore：contains 校验（"Path escapes the project"）、有 entry→checkout、无 entry→递归删除；门 `enabled()` snapshot.ts:185-202, git.ts:578-623 | `restore_blocking`：lexical 校验同消息、blob 写回（含 symlink 120000/exec 100755）、缺失→remove_recursive；门 enabled_gate snapshot.rs:779-845 | ✅ |
| TreeID=树 oid 字符串 git.ts:40 / snapshot ID=brand(String) schema/snapshot.ts:5 | `SnapshotId::parse(tree.to_string())` snapshot.rs:617 | ✅ |
| OperationError 10 类 operation + directory git.ts:43-59 | `SnapshotError::Operation{operation: &'static str}`（capture/files/diff/restore 四类，沿 Snapshot.Error 口径 snapshot.ts:18-22）snapshot.rs:133-137 | ✅（外层口径；git.ts 细分操作名不外泄） |
| file-access resolve：~ 展开、internal=location∪worktree(非 fs 根)、external 目录边界、save 锚 Project.root(.git/.hg) file-access.ts:67-123, project.ts:52 | file_access.rs:100-244（lexical_resolve/lexical_relative/path_contains/project_root [.git,.hg]） | ✅ |
| authorizeExternal 去重单 assert、authorizeRead sibling 跳外部审批、read save `["*"]` file-access.ts:125-161 | file_access.rs:249-312 | ✅ |
| file-mutation withLock：resolve+去重+排序、右折叠全局锁 file-mutation.ts:71-74 | `acquire_locks` BTreeSet+右到左嵌套 file_mutation.rs:267-293 | ✅ |
| write：per-target 锁 + existed 探测 file-mutation.ts:87-101 | file_mutation.rs:304-320 | ✅ |
| writeTextPreservingBom：读前 3 字节判 BOM、保留至多一个 file-mutation.ts:103-118 | `write_text_preserving_bom`（读整文件判 BOM）file_mutation.rs:323-345 | ≈ D-7 |
| syncTextBom / readText：Bom.decodeBytes/syncBytes 条件重写 file-mutation.ts:45-57 | read_text/sync_text_bom（canonical≠stored 才写）file_mutation.rs:117-151 | ✅ |
| file-retention cleanup：stat mtime<cutoff 删、错错忽略、并发 8 file-retention.ts:6-23 | 逻辑内联在 tool_output.rs:342-365（`cleanup_with`）+ 间隔设置 file_retention.rs | ✅（泛型 helper 未单独成文件） |
| job start/wait/block/background/backgroundAll/cancel/pending/complete job.ts:178-476 | jobs.rs:170-621（watch 通道替 Deferred、abort 替 scope close） | ✅ |
| 25 条 consumed 历史、首次消费排序、recovery 保留 job.ts:185-196, 311-314 | consume/wait 同口径 jobs.rs:217-239, 379-383 | ✅ |
| settle 跳过 cancelled 的 persist、cancel 却 persist job.ts:235, 428 | settle 同（jobs.rs:273）、cancel 同（jobs.rs:548）——ref 怪癖被忠实保留 | ✅ |
| vcs.init：仅 `git init`/`hg init`，**无 gitignore 处理**（plugin/vcs/git.ts:39-45, hg.ts:43-54）；错误分类 missing/conflict/unknown/unsupported/failed vcs.ts:24-26,177-194 | vcs_init 直接 `git init`/`hg init`，同样无 gitignore 处理；错误映射 400/409/503 handlers/project.rs:1862-1936 | ✅（"gitignore 处理"在 ref 本就不存在，双侧一致） |
| vcs.diff：git 插件每条命令带 `--no-optional-locks -c core.autocrlf=false …` cfg（plugin/vcs/git.ts:152-164）；列表命令 cwd=location.directory `-- .`，单文件命令 cwd=worktree（:61-64） | handlers/project.rs 全部 cwd=project.canonical、无 cfg 标志 | ⚠ D-2/D-3 |
| vcs.status：statusNames（--untracked-files=all，scoped）+ HEAD numstat + statUntracked，含 additions/deletions（plugin/vcs/git.ts:77-100） | vcs_status：canonical、无 pathspec、additions/deletions 恒 0 handlers/project.rs:1806-1860 | ⚠ D-4 |
| vcs.branch.list limit=min(50,100)（server/handlers/vcs.ts:87） | 默认 50、min 100 handlers/project.rs:1284 | ✅ |
| vcs.base reflog/default-branch 算法（plugin/vcs/git.ts:286-318） | vcs_base 同算法 handlers/project.rs:1043-1119 | ✅ |
| vcs.branch.updated HEAD watcher（vcs.ts:153-172） | location_watcher.rs:97-111（HEAD/branch 目标）+ bus registry VCS_BRANCH_UPDATED | ✅ |
| hg adapter（plugin/vcs/hg.ts） | handlers/project.rs:809-996（HGPLAIN、ancestor(., default)、?/! 合成 patch） | ✅ |

## 行为差异清单

- **D-1 location 越界兜底**：ref snapshot scope 对 `location.directory` 不在 worktree 内时返回 Error（snapshot.ts:110-111）；Rust `location_scope` 返回 None 后 engine 一律兜底 `"."`（engine/mod.rs:1807），即以整个 worktree 为 scope 静默捕获，无 "Location is outside the project" 错误路径。
- **D-2 vcs git 命令环境**：ref 每次 git 调用附 `--no-optional-locks -c core.autocrlf=false -c core.fsmonitor=false -c core.longpaths=true -c core.symlinks=true -c core.quotepath=false`（plugin/vcs/git.ts:152-164）；Rust handlers/project.rs 全部裸 `git`（如 :1522, 1832, 1956），继承宿主 `~/.gitconfig`（autocrlf/quotepath 影响补丁字节与非 ASCII 路径转义）。
- **D-3 vcs diff/status 作用域**：ref 列表命令 cwd=`location.directory` 且 pathspec `"."`（插件 :63-64），Rust 统一 cwd=`project.canonical` 且 diff 带 `"."`（=整个项目根）、status 无 pathspec（handlers/project.rs:1394, 1832）——location 为项目子目录时，Rust 返回全仓变更而非 location 范围。
- **D-4 vcs.status 统计缺失**：ref status 返回 additions/deletions（HEAD numstat + untracked --no-index statUntracked，plugin/vcs/git.ts:80-99）且 `--untracked-files=all`；Rust 恒 `additions:0, deletions:0`、untracked 默认档（handlers/project.rs:1832-1848）。
- **D-5 状态码映射偏差**：ref `kind("AM")→added`（git.ts:166-172，porcelain 双字母含 A 无 D）；Rust `status_code_to_kind("AM")→modified`（project.rs:1852-1860，match 臂漏 "AM"）。
- **D-6 快照仓库配置子集**：ref 写完整 `opencode.gitconfig`（longpaths/symlinks/untrackedCache/manyFiles/index v4/threads，git.ts:27-38）；Rust 仅 `core.autocrlf=false + core.quotepath=false`（snapshot.rs:319-327）。git2 路径下多数键无意义，但 Windows longpaths 语义缺失。
- **D-7 writeTextPreservingBom 读法**：ref 只读前 3 字节且非 NotFound 的读错误会**失败**该写（file-mutation.ts:107-109 仅 catch NotFound）；Rust `.ok()` 把 EACCES 等吞为“不存在”并继续覆盖写（file_mutation.rs:332-334）。
- **D-8 树的覆盖面**：ref 快照 index 由源 index 播种 + 分 scope 增量刷新，write-tree 覆盖整个项目（含 scope 外 tracked 文件，git.ts:369-380, 384-436）；Rust 每次从当前 scope 遍历重建整树（snapshot.rs:526-616），scope 外文件不入树。对 files/diff（双侧树口径一致）无影响；restore 口径也由调用方 file 集限定——实际风险低。
- **D-9 快照实现形态**：ref 走 git CLI（`--git-dir/--work-tree`），Rust 为 git2 bare odb + 自维护 stat-cache sidecar + 递归 build_tree（snapshot.rs:913-949）；无 ref 的 index/`rm --cached` 路径。语义等价但为独立实现（已在模块头声明，属已知有界偏离）。

## 正确性风险

- **R-1 (P1) `snapshots:false` 配置无消费者——确认 C4 发现，且 ref 侧确为开关**。证据链：ref `ConfigSnapshotPlugin` 读取配置键 `snapshots` 并调用 `snapshot.transform(editor => editor.configure(configured))`（config/plugin/snapshot.ts:15-19），`configure` 直写 `state.enabled`（snapshot.ts:83-87），`enabled()` 参与 capture/restore 门（snapshot.ts:115, 197）。Rust：schema 键存在（oc-schema config/mod.rs:85-87）、normalize 接受（oc-core config/normalize.rs:547-553），但 `GitSnapshotStore::configure` 全仓仅测试调用（snapshot/tests.rs:168-170）；engine 构造处未读取该键（engine/mod.rs:1806-1818）。后果：Rust 用户配置 `snapshots:false` 后 capture/restore 照常执行（磁盘与行为不尊重开关）。
- **R-2 (P2) D-2/D-3 组合**：宿主 `core.autocrlf=true` 时 vcs.diff/status 输出与 ref 系统性偏差（补丁行尾、renames 检测）；location≠项目根时 diff/status 范围放大到全仓。建议补 cfg 标志并把列表命令 cwd 换成 location.directory。
- **R-3 (P2) D-4**：vcs.status 的 additions/deletions 恒 0，消费方（TUI diff 面板等）拿不到行统计；与 ref wire 形状同但数据缩水。
- **R-4 (P2) D-7**：对不可读但存在的文件，Rust writeTextPreservingBom 会当作新建静默覆盖；ref 会报错。权限敏感目录下可能吞掉本应 surfaced 的失败。
- **R-5 (P2) D-1**：location 越界时 Rust 静默以 `"."` 全仓捕获，ref 报错；配合 R-1（无法关掉）放大为不受控的全仓快照写入。
- **R-6 (P3) D-5**：vcs_status 的 "AM"/"AD" 映射与 ref 的 kind() 不一致（added/deleted 误报为 modified）。
- **R-7 (P3) D-6**：Windows 长路径下快照 walk/restore 可能遇 ref（经 longpaths=true）能处理的路径失败；Linux 无影响。
- **R-8 (P3) 快照仓库锁自驱逐竞态**：`REPOSITORY_LOCKS.remove_if(strong_count==1)` 在 drop(lock) 后执行（snapshot.rs:403-411），注释已论证克隆期 count>1 保护；成立，但依赖 Arc 计数的窗口极窄，属“已论证的微妙正确”，非缺陷，记录备查。

## 覆盖声明

已逐行读完：ref 侧 snapshot.ts(233L)、vcs.ts(260L)、vcs/patch.ts(106L)、git.ts(758L)、file-access.ts(167L)、file-mutation.ts(134L)、file-retention.ts(23L)、job.ts(480L)、plugin/vcs/git.ts(612L)、plugin/vcs/hg.ts（grep 定位 init/ignore 段）、config/plugin/snapshot.ts(21L)、schema/snapshot.ts、server/handlers/vcs.ts(102L)、util/hash.ts(fast 段)、project.ts(root/markers 段)。Rust 侧 snapshot.rs(1061L)、file_access.rs(842L，实现段逐行+测试浏览)、file_mutation.rs(实现段逐行)、file_retention.rs(105L)、jobs.rs(1277L，job 内核 1-621 逐行；restart 段 622-1038 按 C2b 已审约定仅浏览结构)、oc-server handlers/project.rs 的 VCS 段(800-1959 逐行)+worktree 段仅定位、engine/mod.rs 快照构造段、tool_output.rs cleanup 段、snapshot/tests.rs 未逐行（仅引用行号核对）。测试文件未逐行（按任务约定按需查阅）。未覆盖：ref git.ts 的 clone/fetch/checkout/reset/worktree 族在 Rust 的对应（worktree.rs 属其他 lane；Rust 侧未发现 sync/clone 对应物——若属本 lane 应补审，grep 未在 oc-core/oc-server 找到 fetchRemotes/resetHard 的移植，可能位于 worktree 实现内，**此点未深究，声明为缺口**）。

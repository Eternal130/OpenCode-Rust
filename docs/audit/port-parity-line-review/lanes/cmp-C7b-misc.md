# C7b 结论: 核心杂项域整体移植质量高（form/image/websearch/skill/rpc/reference 逐语义对齐且有 G 编号回溯），主要缺口为 shutdown 序列缺 rpc.close、mime 检测三副本不一致、websearch progress 粒度与 worktree/表单校验顺序等小偏差。

## 对比表

| 域 | ref（v2.0.24） | rust | 判定 |
|---|---|---|---|
| image 限额/缩放 | image.ts:58-93 + image/photon.ts:33-107（photon WASM、Lanczos3、PNG→JPEG[80,85,70,55,40] 阶梯、×0.75 递减 ≤32 档、base64 预算 ⌈n/3⌉×4、默认 2000×2000/5MiB） | oc-core/src/image.rs:133-236（`image` crate Lanczos3、同阶梯/同预算公式 div_ceil(3)*4、错误文案 byte-identical）；ResizerUnavailable 分支按声明永不触发（image.rs:8-12）；仅收 PNG/JPEG 与 photon 解码集一致（image.rs:155-158） | ✅ |
| image normalizeImages | tool.ts:74-95 | image.rs:246-309（omitted 注释文案/单复数一致） | ✅ |
| form 服务 | form.ts:104-231（Cache：pending TTL∞/settled 10min；publish-first；create onError 回滚；ask onInterrupt→cancel；close 兜底 cancel） | oc-core/src/forms.rs（watch channel、RETENTION_MS=10min、G-276① create 回滚/reply/cancel publish 失败不 settle、AskGuard Drop→cancel、closed 后 create 即 cancel） | ✅（顺序差 D-1） |
| form 校验 | form.ts:235-370（when 级联、闭合 options、email/uri/date/date-time、multiselect） | forms.rs:391-839（逐条对齐，含 date-time 偏移形状、external 必须 `true`） | ✅ |
| mime detect | mime.ts:3-33（PNG 8 字节/JPEG/GIF"8"/BM/PDF/RIFF·WEBP/ftyp avif|avis + isText：NUL、strict UTF-8、控制字节 ≤30%） | 三副本：session/prompt.rs:82-133（**逐字节全对**）；tool/read.rs:126-151（PNG 只查 4 字节、GIF 只查 GIF87a/GIF89a）；oc-ai/media/utils.rs:236-289（provider 侧另一表，属 ref media-type.ts 非本域） | ⚠️ D-2 |
| location key/canonical | location-service-map.ts:39-44（仅 win32 path.normalize） | location.rs:54-90（cfg(windows) 门控，G-276⑤） | ✅ |
| location 服务图 | location-services.ts（LayerMap、idleTTL∞、boot 失败逐出+invalidate、invalidate=detach→close、旧图存活） | location_services.rs:219-342（DashMap+Arc 旧图存活、boot 失败不发布可重试、invalidate→shutdown；get_or_build 竞争取 incumbent） | ✅ 换形保义 |
| location activity | location-activity.ts（TTL 60min/sweep 1min、durable 会话事件 touch 仅活位置、expired→interrupt"inactivity"→recheck→新主人则保留） | location_services.rs:139-201, 370-447（sweep_once 三步逐条对应；attach_activity_bus_touch=RW-12） | ✅ |
| reload-all | location-service-map.ts:20-36（任一重建失败→整体 failCause） | location_services.rs:316-326（返回失败列表，不整体失败） | ⚠️ D-3 |
| location lifecycle | location-lifecycle.ts:32-56（boot project.activate + 会话事件重激活 + shutdown：permission→forms→**rpc.close**→publish location.shutdown） | location.rs:978-992（permission→forms→publish，**无 rpc.close**）；boot 侧 project.updated 由 oc-server/announce.rs:395-424 覆盖；会话事件重激活 project 无对应 | ⚠️ D-4/R-1 |
| instance 每位置层 | instance.ts:59-181（55 节点图、vanilla discovery 开关、DirectoryNotFoundError 分类） | location.rs LocationGraph（精简具体图 + BootError::MissingDirectory/Database probe）；完整服务装配在 engine/mod.rs（C2a 已审） | ✅（本 lane 范围内） |
| app 元信息/useragent | app.ts:16-26（name/version/channel；UA=`opencode/{channel}/{version}/{name}`） | session/runner/model_request.rs:55-89（OPENCODE_CLIENT??"cli"、CARGO_PKG_VERSION=2.0.24、channel 单源 oc_util::channel；UA 格式逐字一致） | ✅ |
| rpc.ts 语义 | rpc.ts:35-289（多注册后写优先、call 失败类型 rpc.unavailable/method_not_found/invalid_input/invalid_output/internal、DeclaredError 未声明→die、race closed、client events=`rpc.{id}.{name}` ephemeral + location 定域） | handlers/misc.rs:446-480 + plugins.rs:79,127-215 + oc-plugin-host host.js:717-775,1483-1528 + G-295 rpc_event_slot（失败通道/文案逐字；`rpc.{id}.{event}` ephemeral 按位置定域） | ✅（closed 语义见 D-4） |
| websearch 服务 | websearch.ts:85-221（selection=id/random/false、KV `websearch:provider`+脏条目 kv.remove、cooldown 仅 random+429+Retry-After(60s 默认)、per-session affinity、session.deleted/moved 清 affinity、providers localeCompare 排序） | tool/websearch.rs:46-462（config_selection 优先且 gate 注册、G-282② 脏 KV 删除、cooldown_ms/http_date_ms、G-224② affinity 订阅清除、G-266① 均匀随机 picker、G-282① localecmp 排序、Drop 清 preferred） | ✅ |
| websearch provider | plugin/websearch/*（HTTP tools/call、256KiB、SSE/direct 解析） | tool/websearch_providers.rs:48-233（MAX_RESPONSE_BYTES=256KiB、MCP 25s、Exa 文本块解析 + G-282④ Date.parse 宽集合） | ✅ |
| websearch 工具挂接 | plugin/websearch.ts（onProvider **每次尝试前** progress、Disabled 三钩子删工具） | websearch.rs:457-489,683-807,834-869（DisableHook 三钩子在；progress 仅结果边界一次） | ⚠️ D-5 |
| skill 服务/输出 | skill.ts:33-68（available 权限过滤、toModelOutput、prepare 采样 10 文件） | engine/skills.rs（表+merge_plugin_skill PLUG-D35；工具侧 C1 已审） | ✅ |
| skill URL 发现 | skill/discovery.ts:13-205（index.json 每次、isSafeSegment/RelativePath、同源、contains、`.opencode-version` 同版本零下载、暂存 tmp/old+原子 swap+回滚、并发 4 skill/8 file） | engine/skills_url.rs（守卫逐条、versioned refresh + ensuring 清理；**串行**执行 + **新增 30s 总超时**：119-122）；skills.rs:284-304（内建先种、config 无条件覆盖=editor.add 语义） | ✅（D-6） |
| worktree create/remove/refresh | worktree.ts:106-339 + strategies.ts（默认 `<data>/worktree/<id[:6]>`、slug+后缀 ≤10、row replace、commands.start（win 用 cmd）、refresh 根=canonical+无主行、策略逆序、单事务） | oc-server/handlers/project.rs:300-629 + oc-core/project.rs worktree_strategy + GuestWorktreeStrategy（插件策略、claim_worktree replace、事务、G-322 排序、G-270⑦） | ✅（D-7） |
| reference 服务 | reference.ts:44-122（add/remove→refresh fork+`reference.updated`、list 丢弃非法 git/branch、每 1h forever 刷新、并发 4） | reference/mod.rs（sources_from_config 逐条镜像 validAlias/localPath/裸字符串判定、spawn_refresh、hourly sweep:426） | ✅ |
| reference instructions | reference/instructions.ts | reference/instructions.rs（`core/reference-guidance`、delta 渲染逐行；排序用 toLowerCase 近似 localeCompare，自述偏差） | ✅（D-8） |
| util/wildcard | util/wildcard.ts（`" *"`→`( .*)?`、win32 大小写不敏感） | oc-util/wildcard.rs:17-41（同重写、cfg(windows)） | ✅ |
| util/slug | util/slug.ts（29 形容词+31 名词） | handlers/project.rs:254-273（29+31 词表一致） | ✅ |
| util 其余 | token(4 chars/token)/which/git-executable/lazy/iife/NamedError | token.estimate 无共享实现（散落各处估算）；which 仅 formatter.rs:554 私有；git-executable Windows 解析无独立对应（直接用 "git"）；lazy/iife 由语言构造覆盖；NamedError→thiserror 枚举体系 | ⚠️ D-9 |
| announce/boot 级联 | v2 State reload/transform notify 多事件集 | announce.rs（SERVICE_FRAMES + CONFIG_CASCADE_FIRST/LATER + 首变更补 skill.updated + G-218 filesystem.changed 单发布者→branch 事件驱动） | ✅ |

## 行为差异清单

- **D-1 form.create 校验顺序**：ref form.ts:127-131 先查 `AlreadyExistsError` 再 `validateFields`；rust forms.rs:160-185 先 `validate_fields` 再查存在性。同一请求同时命中两条件时返回错误类型不同（InvalidForm vs AlreadyExists）。仅影响带显式 id 重放非法表单的通路。
- **D-2 mime 检测三副本不一致**：ref 是单一 `Mime.detect`。rust：(a) prompt.rs:82-133 全对齐；(b) read.rs:126-151 PNG 只查前 4 字节（ref 查 8 字节签名）且 GIF 限 GIF87a/GIF89a（ref 匹配 "GIF8" 前缀）；(c) oc-ai/media/utils.rs 是另一域的表。同域内 (a)(b) 漂移未收敛（oc-util/mime.rs 的 G-92 收敛只做了扩展名表，未收敛 magic-byte）。
- **D-3 reload_all 失败语义**：ref `LocationServiceMap.reload` 任一重建失败即整体 fail（location-service-map.ts:33-34）；rust `reload_all` 收集失败返回 `Vec`（location_services.rs:316-326），调用方拿到的是部分成功。POST /api/location/reload 的错误面与 ref 不同。
- **D-4 location shutdown 缺 rpc.close 与 closed-race**：ref shutdown 序列 permission.close→forms.close→**rpc.close**→publish（location-lifecycle.ts:44-54），且 `Rpc.call` 与 closed Deferred `raceFirst`（rpc.ts:133-137）——关闭后在途/后续 RPC 调用立刻翻 `rpc.unavailable`。rust `LocationGraph::shutdown`（location.rs:978-992）无 rpc 关闭步骤，插件宿主为进程级。另：ref 每条本位置会话事件 fork `project.activate`（:33-39，60s 限流），rust announce 只在 boot/reload/config 变更时发 project.updated，无会话事件重激活。
- **D-5 websearch progress 粒度**：ref `query` 循环内**每次 provider 尝试前**调 `options.onProvider`（websearch.ts:191）；rust 仅结果边界一次（websearch.rs:765-771 自述）。429 轮换中间的 provider 上报丢失。
- **D-6 skill URL 拉取执行模型**：ref 并发 ≤4 skill/≤8 file；rust 全串行（skills_url.rs:44-53 自述）。另 rust 新增 30s 总超时（:119-122），ref 无此上限——慢网络下行为不同（防 Engine::assemble 卡死，有意偏差）。
- **D-7 worktree create 细节**：(a) 空/空白 `name`：rust 直接报错（project.rs:342-345），ref 不 trim 不拒绝；(b) `commands.start`：rust 恒用 `bash -lc`（:486-492），ref win32 用 cmd+shell（worktree.ts:238-250）；(c) ref create 时从**目标位置**加载策略（worktree.ts:121-128），rust 用进程级注册表 + 插件更新时重注册（plugins.rs:289-299）——多位置不同插件策略时 rust 不区分位置。
- **D-8 reference instructions 排序**：ref `localeCompare`；rust toLowerCase+码点近似。非 ASCII 名称排序可能不同。
- **D-9 util/token 无共享对应**：ref `Token.estimate = len/4` 是共享工具；rust 未统一实现。一致性风险低。

## 正确性风险

- **R-1（中，D-4）**：location 关闭后 RPC 仍可调。ref 的 rpc.close + race 保证「location 下线 ⇒ 其 RPC 域不可用」；rust 中插件宿主存活于整个 server 进程，已被 invalidate 的 location 注册的 RPC 仍可经 `/api/rpc/:id/:method` 命中。
- **R-2（低，D-2）**：read 工具的 PNG 4 字节签名可能把 `89 50 4E 47` 开头的非 PNG 误判为 image/png 进入 base64 直出（ref 8 字节签名会判 text/binary）。实际碰撞概率低。
- **R-3（低，D-1）**：表单重放（MCP elicitation 固定 id 重放路径存在，forms.rs:208-211 自述）时，非法 fields 的重放返回 InvalidForm 而非 AlreadyExists——若调用方以 AlreadyExists 作幂等信号会误重试。
- **R-4（低，D-3）**：`reload_all` 吞掉失败后，`POST /api/location/reload` 调用方无法区分全量成功与部分失败；与 ref 的 fail-fast 契约不同。
- **R-5（信息）**：skills_url 30s 总超时是 rust 独有护栏；合法慢源（>30s）下 rust 放弃整源（返回空），ref 会等。boot 期间表现为「URL skill 消失」而非慢。

## 覆盖声明

- 已逐行读完 ref：image.ts、image/photon.ts（加载壳略读）、form.ts、mime.ts、app.ts、rpc.ts、location.ts、location-services.ts、location-service-map.ts、location-activity.ts、location-lifecycle.ts、instance.ts、instance/service.ts（重导出壳）、websearch.ts、skill.ts、skill/discovery.ts、skill/instructions.ts（略读，属 C1 交叠）、worktree.ts、worktree/strategies.ts（worktree/{directory,git,sql}.ts 按需核对 canonical/claim 语义）、reference.ts、reference/instructions.ts、util/ 全部 9 文件。
- 已逐行/大块读完 rust：oc-core image.rs（全文）、forms.rs（全文）、location.rs（全文）、location_services.rs（全文）、reference/{mod,instructions}.rs（repository/repository_cache 按需抽查）、engine/skills.rs（前 330 行全文+结构）、engine/skills_url.rs（前 400 行全文+注释）、tool/websearch.rs（全文）、tool/websearch_providers.rs（结构+Exa 全文）、oc-util/mime.rs、oc-util/wildcard.rs、session/runner/model_request.rs（App/UA 段）、oc-server/announce.rs（全文）、handlers/project.rs worktree 段（300-629）、handlers/misc.rs rpc/websearch 段、plugins.rs rpc 段、host.js rpc 段（grep 定位）。
- 交叠域未深审：skill 工具侧与 SKILL.md 扫描细节（C1）、session/event/sql（C5a）、subagent 非循环挂接（C2a）、rpc 的 schema wire 定义（oc-schema）。worktree remove/refresh 后半（project.rs 630+）仅结构性核对未逐行。测试仅在验证 G 编号声明时引用。

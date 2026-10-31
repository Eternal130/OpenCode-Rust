# OpenCode Rust 移植 vs 参考版（/opt/opencode-ref）全功能审查总表

审查日期：2026-10-31。方法：9 个并行深度审查通道（session、配置/认证、工具、core 基础设施、AI、TUI、server、CLI、client/schema/plugin 等），以两侧源码为准，未采信任何 PORT_*.md / FINDINGS.md。

## 一、按域总览

| 域 | 参考版 | 移植版 | 完整度 | 总评 |
|---|---|---|---|---|
| core/session（生命周期/投影/收件箱/消息状态机/协调器/bus/job/permission/subagent） | core/src session* | oc-core session/ engine/ bus | 高（无高危） | 关键分支与边界语义逐点对齐；多处有意增强 |
| core/配置/认证/凭据/模型目录 | config* credential* models-dev | oc-core config/ integration/ provider_catalog | 高 | 主要风险在事件驱动联动时机 |
| core/工具 | core/src/tool/ | oc-core/src/tool/ | 高 | 模型可见文案基本字节级一致 |
| core/基础设施（fs/snapshot/shell/formatter/mcp/plugin/pty/integration） | 对应目录 | 对应 .rs | 高（个别 Critical/High） | 错误文案/常量/锁序大量逐字节对齐 |
| AI/provider | packages/ai (31k) | oc-ai (63k) | 高 | 协议锚点逐字对齐 + parity 测试；评测 lane 缺失 |
| TUI | packages/tui (497 文件) | oc-tui + oc-tui-core | 高 | 231/231 键位覆盖；composer 选区/IME 视觉未完成 |
| server/HTTP | packages/server + protocol | oc-server + oc-http | 很高（140/141 路由一致） | 缺口集中在 persistent-pty WS |
| CLI | packages/cli | oc-cli | 很高 | 命令/标志/退出码近乎 1:1 |
| schema/theme/util/client | 对应包 | oc-schema/oc-theme/oc-util/oc-client | 高 | 常量级一致 + parity 测试 |
| plugin-host / codemode | packages/plugin, codemode | oc-plugin-host + Bun guest | 中（架构重写） | 缺 4 个 Context 域；codemode 换 Bun 真执行 |
| function（云分享服务端）/ sdk / 评测 | packages/function, sdk, ai/experimental | 无对应 | 缺失 | 有意裁剪需确认 |

## 二、必须修复（Critical / High）

1. **[Critical] snapshot restore 丢失文件模式与符号链接** — snapshot.rs:489-530：symlink（0o120000）被还原为含目标文本的普通文件，可执行位丢失；回滚后仓库损坏。ref 经 git checkout 保留。
2. **[High][安全] persistent-pty WS connect 缺 Origin 前置检查** — persistent_pty.rs:293-310（`let _ = headers;`）：跨源页面拿到 ticket 即可绕过 origin 政策；pty 路由已有正确实现可复用。
3. **[High][安全] read 工具丢失 realpath 越界防护** — filesystem.ts:132-139 vs tool/read.rs:382+：词法检查可被目录内指向外部的符号链接绕过；file_access.rs:209-219 注释自认 lexical。
4. **[High][安全] `debug config` 脱敏弱于参考版** — misc.rs:448-471：漏 credential/private key/authorization/cookie 键与 URL 凭据/headers 传播规则，可能明文打印密钥。
5. **[High] skill 工具输出格式错位** — skill.rs:21-47：把目录全部文件内容拼进输出；ref 是 `<skill_content>` 信封 + ≤10 个采样路径列表。会显著改变模型可见输出并可能撑爆上下文。
6. **[High] run 非交互：非 question 表单取消退出码 0** — noninteractive.rs:309-335 vs noninteractive.ts:188-207（ref exit 1）；脚本漏检失败。
7. **[High] shell create 缺省 cwd 不落 location.directory** — shell.rs:562-563：空 cwd 时子进程继承服务进程 cwd（需确认引擎侧总是显式传 cwd，否则目录错写）。
8. **[High] 插件 Context 缺 provider/reference 域** — host.js 无 "provider.list"/"reference.list" 等：依赖插件运行时 method_not_found。
9. **[High] function（云同步/分享服务端）整体缺失** — packages/function 无对应；`SessionShare.share_url` 仅为数据库字段。若属有意裁剪请确认。
10. **[High] schema 缺 filesystem 非事件类型（Entry/Submatch/Match/FindInput/Write）与 rpc.ts 体系** — oc-schema/src/lib.rs:80-81 自认；wire 演化无金丝雀。

## 三、应修复（Medium）

| # | 域 | 问题 | 位置 |
|---|---|---|---|
| M1 | session | step.failed 的 finish 被强制映射为 ContentFilter（ref 原样透传） | message_updater.rs:490-494 |
| M2 | session | Session.create 项目解析仅目录身份，非 VCS/worktree 感知的 projects.resolve | store.rs:604-609 |
| M3 | session | engine.prompt 默认绕过 SessionPrompt.prepare（附件/@skill 依赖未武装 seam） | engine/mod.rs:1635-1664 |
| M4 | 配置 | 凭据/wellknown 变更不驱动 config 重载与 location 失效；WatchedConfig loader 未接 wellknown 源 | engine/mod.rs:570-596, boot.rs:242-261 |
| M5 | 配置 | 删除非 active 凭据多发 credential.switched(credentialID:null) | integration/credential.rs:331-360 及 oc-server 镜像 |
| M6 | 工具 | list_mcp_resources 模型内容固定 "Listed MCP resources"（ref 为 stringify 目录） | mcp_resource.rs:213 |
| M7 | 工具 | 工具输入校验为 JSON-Schema 子集，MCP 外部 schema（$defs/allOf/pattern…）不校验 | registry.rs:88-232 |
| M8 | infra | prettier/oxfmt 用项目 node_modules/.bin 而非 Npm.which（无全局缓存/自动安装）→ 不格式化 | formatter.rs:585-598 |
| M9 | infra | snapshot store 不做 worktree realpath → 符号链接项目另开空 store | snapshot.rs:230-233 |
| M10 | infra | MCP stdio 帧 16MiB 上限与退出 stderr 尾巴诊断缺失（rmcp 传输） | mcp/client.rs:1150-1175 |
| M11 | infra | aisdk hook 仅 2 个点，ref AISDKHooks 流/参数变换粒度未覆盖（需专项核） | hooks.rs:916-919 |
| M12 | infra | git.ts 服务未统一移植（clone/fetch/resetHard/reference 仓库同步语义不同） | 分散 |
| M13 | TUI | composer 选区高亮/IME 合成视觉未实现（模型在） | composer/mod.rs:9-14 |
| M14 | TUI | 剪贴板读错误全折叠为 None（ref 抛错 + toast） | clipboard.rs:383-399 |
| M15 | TUI | mini 子代理流/fragment/image 无独立对应，transport 密度约 42%（未证伪，需逐行核） | mini/transport.rs |
| M16 | server | persistent-pty connect：不存在的 pty 返回 HTTP 404（ref 升级后 4404 关闭帧） | persistent_pty.rs:264-470 |
| M17 | server | 空页 cursor 输出显式 null（ref 省略键）——需以对 ref 实测复核 | message.rs:115-128 |
| M18 | server | persistent-pty 二进制输入经 from_utf8_lossy 污染 | persistent_pty.rs:430-436 |
| M19 | CLI | auth login 无 target 时缺交互集成选择器（直接报错） | auth.rs:280-285 |
| M20 | CLI | plugin check/update 不支持 TUI(npm) 插件，输出格式不同 | plugin.rs:425-538 |
| M21 | AI | 评测/实验 lane 整块缺失（evaluation/system-one/opencode-zen/typesafe-ai） | oc-ai |
| M22 | AI | SDK 公共 API 面缺失（AIClient/promise/generation/tool builder） | oc-ai |
| M23 | plugin | Context 缺 experimental.terminal.read、integration.oauth 四方法/connection 查询 | host.js |
| M24 | schema | v1 遗留 schema（Part 联合/permission/legacy-event）类型层未移植，迁移自行内联 | oc-schema |
| M25 | codemode | 沙箱架构差异：自研受限解释器 → 依赖 Bun 真执行（无 Bun 即不可用） | oc-plugin-host |

## 四、低危/信息（摘要）

- session：moved 投影拆两条 UPDATE；terminal snapshot 空 files 不写；中断路径新增 5s/60s 超时；plugins.awaitActivation 缺失；Job errorText 无 cause 链。
- 配置：json5 超集解析；legacy pin 条件收窄；all() 保留 NULL integration_id 行；update_shell 无重载联动/锁；`${VAR}` 校验弱化。
- 工具：glob 跨 location 显示路径基准不同；patch 写前 create_dir_all；shell/websearch/codemode progress 事件缺口；webfetch permission metadata 不含 timeout。
- TUI：OSC52 100KB 自加上限；非 Linux 音频走系统播放器；diff 行号 gutter 有意分歧；8 条自有命令；storybook/simulation 未移植。
- server：persistentPty.update payload 放宽（size 可缺省）；ticket 作用域用进程 cwd；Rust 独有 POST /api/plugin/install；location.get project.directory 被 canonical 覆盖；session.log 400 文案差异。
- CLI：session export 取消退出码 0（ref 130）且 UI 走 stdout；import 弱校验；时间戳/localeCompare 差异；run TTY 权限交互为有意扩展。
- AI：classify_provider_failure 丢弃 cause 链；provider overlay 合并单文件的维护性提示。
- infra：shell cleanup 正则放宽；forms create 错误优先级互换；UTF-16 vs chars 长度；mime 扩展名小表；watcher parcel→notify 粒度差异；is_tracked 不查 HEAD。
- plugin/schema：OTLP 遥测缺失；npm.ts 包管理域部分移植；pty/persistent-pty 输入类型落在生成代码（双源漂移）。

## 五、建议二轮专项核对（本轮未逐行覆盖的深层域）

persistent-pty 全量 wire 协议（~40k 行 ref）、mcp elicitation/sampling 细节、plugin 内置插件逐个对齐、v1-migration 全量 DDL、location-watcher 策略、filesystem/search.ts、oc-tui/store.rs（solid 等价状态机）、mini 子代理流。

## 六、总体结论

移植完整度很高：CLI/server/工具/AI/schema/theme 常量与文案大量逐字节一致并配 parity/golden 测试锁定；session 域无高危。真正需要动手的是：两处安全漏洞（persistent-pty Origin、read realpath 逃逸）、一处数据保真 Critical（snapshot restore）、skill 工具输出、debug config 脱敏、run 表单退出码、插件 Context 四个缺失域，以及确认 function/评测 lane 是否有意裁剪。

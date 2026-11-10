# L13 CLI — 审查报告

> 审查方式：深读文件逐行 + 结构性核验（函数清单 1:1 对照+关键段抽读）。证据格式 file:line。
> 参照：/opt/opencode-ref (v2.0.24 TS) · 移植：/opt/opencode-rust

### 1. 覆盖证明

**深读（逐行）两侧**：index.ts↔main.rs；commands.ts(554)↔cli.rs(1038)；run/run.ts↔run/mod.rs；run/noninteractive.ts(859)↔run/noninteractive.rs(1859)+tool_render.rs+run_ui.rs；session-target.ts↔run/mod.rs 内嵌实现；services/{server-connection,standalone}.ts↔services/{server_connection,standalone}.rs；server-process.ts↔handlers/serve.rs；env.ts/version.ts/util/* ↔ util.rs；ssh-askpass.ts↔ssh_askpass.rs；mini.ts(部分)↔mini.rs；handlers/{acp,upgrade,uninstall,pair,api}.ts↔handlers/{misc,api}.rs；acp/{agent,service,turn,connection,child,permission}.ts↔acp/{mod,agent,service,turn,transport,permission}.rs；commands.ts 全 flag 面逐项比对。

**结构性核验**：acp/{translate,elicitation,sessions,catalog,config-option,replay,content,tool,compaction,directories,capabilities,error}.ts↔同名 .rs（接口逐函数对上）；services/{service-config,updater}.ts↔service_config.rs/updater.rs；handlers/{stats,auth,auth_form,mcp,plugin,session,service_cmds,models,reload}.rs 均存在且命令面完整。mini UI 本体在 oc-tui（按约定不在本域）。

### 2. 逐项对比表

| 功能点 | Ref 实现 (file:line) | Rust 实现 (file:line) | 状态 | 差异说明 |
|---|---|---|---|---|
| 根命令 flags(directory/-c/-s/--prompt/--print-logs/--standalone/--server/--auto/--yolo/--dangerously-skip-permissions) | commands.ts:15-58 | cli.rs:27-78 | ✅ | rust 将 permission 三旗标提为全局（clap 限制），语义等价 |
| upgrade[target/-m, alias update, 7 methods] | commands.ts:60-74 | cli.rs:257-265,91-121 | ✅ | rust 新增 local-file/github 两 method（🔵扩展） |
| uninstall(-c/-d/--dry-run/-f) | commands.ts:75-98 | cli.rs:266-280; handlers/misc.rs:111-254 | ✅ | 行为对齐（含"不删自身二进制"语义 G-288⑤） |
| acp 命令 | commands.ts:99 | cli.rs:282; acp/mod.rs:112-167 | ✅ | |
| api(变参 request/-d/-H≤100/--param) | commands.ts:100-116 | cli.rs:284-297,479-522; handlers/api.rs | ✅ | 上限100、OpenAPI 插值均有 |
| debug agents/config/paths(10 键) | commands.ts:117-143 | cli.rs:526-536; handlers/misc.rs:426+ | ✅ | rust 在解析期拒绝非法键（ref 是运行期崩） |
| auth list/login/logout/export/import/switch | commands.ts:144-221 | cli.rs:538-581; handlers/auth.rs(1525)+auth_form.rs | ✅ | --answer≤100 校验对齐 |
| mcp list/add/auth/logout | commands.ts:222-260 | cli.rs:583-620; handlers/mcp.rs(1011) | ✅ | rust add/remove 多出 --force(JSONC 重写)🔵 |
| plugin list/add/check/update/remove | commands.ts:261-304 | cli.rs:622-656; handlers/plugin.rs(866) | ✅ | 同上 --force 🔵 |
| models / reload | commands.ts:305,513-518 | cli.rs:319,433; handlers/misc.rs:276,312 | ✅ | |
| stats(11 flags, year 1970-9999, limit≥1) | commands.ts:309-339 | cli.rs:321-352; handlers/stats.rs(979) | ✅ | 边界校验对齐 |
| mini(12 flags, --no-replay) | commands.ts:340-375 | cli.rs:354-388; main.rs:134-177 | ✅ | replay 三级回退(flag→cli.json→true)对齐 |
| run(16 flags, -f≤100) | commands.ts:376-417 | cli.rs:390-421 | ✅ | |
| session list/delete/export/import | commands.ts:418-470 | cli.rs:658-690; handlers/session.rs | ✅ | -n≥1、format table/json 对齐 |
| service start/restart/status/stop/get/set/unset | commands.ts:471-512 | cli.rs:692-726; service_cmds.rs; service_config.rs | ✅ | |
| pair --url(校验协议/凭据/query/fragment) | commands.ts:519-536; handlers/pair.ts | cli.rs:435-439; handlers/misc.rs:326,642 | ✅ | 校验与错误文案精确对齐（有测试） |
| serve(--hostname/--port/--cors/--service/--stdio) | commands.ts:537-550 | cli.rs:446-459; main.rs:220-233 | ✅ | service+stdio 组合报错为 rust 显式检查 |
| completion(4 shells) | 无 | cli.rs:440-445,662-677 | 🔵 | rust 扩展 |
| 入口: askpass 隐藏分发/uncaught 处理/--version | index.ts:20-105 | main.rs:26-75 | 🟡 | rust 无 Heap(SIGUSR1)/CpuProfile(SIGPROF) 快照、无 OTEL Observability 层；新增 v2→rust 数据迁移与后台更新检查(🔵) |
| run: --fork 校验/PWD chdir/stdin 管道合并 | run.ts:71-81,156-174 | run/mod.rs:367-397 | ✅ | PWD 陈旧时不采用（有记录的刻意偏差） |
| run: 附件准备(10MiB/MIME/文本内联\<file\>) | run.ts:185-224 | run/mod.rs:78-116 | 🟡 | rust MIME 探测为魔数+扩展名子集（ref FSUtil.mimeType 全表） |
| run: title 设置(new session, 截断50) | run.ts:130-135 | run/mod.rs:412-432 | ✅ | |
| session-target 解析(find/latest/fork/env/目录校验) | session-target.ts:32-171 | run/mod.rs:197-349 | ✅ | SessionNotFound→新建、5xx 传播等价（RD-10 测试） |
| 非交互 consume 循环(全部事件类型/promotion/finalizing) | noninteractive.ts:209-548 | noninteractive.rs:738-1165 | ✅ | 逐事件对齐（含 inbox 促升、prePromotionError、exit_code 语义） |
| 权限自动应答(非auto拒绝+反馈) | noninteractive.ts:158-186 | noninteractive.rs:257-305 | ⚠️ | rust 在 TTY 上弹出 once/always/deny 交互选择（CI/OPENCODE_NO_TTY_PROMPT 可关）——ref 恒自动拒绝；默认路径行为不同（脚本下等价） |
| 表单取消(question 反馈继续/非question 退出1/global哨兵+目录头) | noninteractive.ts:188-235,740-757,795-808 | noninteractive.rs:309-340,592-623 | ✅ | x-opencode-directory=encodeURIComponent 自实现，测试覆盖 |
| reconcile 投影回放/exit 语义(130中断/1失败/0) | noninteractive.ts:550-788 | noninteractive.rs:635-712,1186-1369 | ✅ | found=false 丢弃消息列表的 quirk 也保留 |
| v1 兼容层(run/v1.ts 桥) | run/v1.ts:1-86 | grep 无对应 | 🔴 | runV1Bridge 仅供 v1 遗留入口使用，v2 CLI 行为无影响 |
| server-connection(--server 探测5s/版本警告/standalone/disabled/managed 三态 mismatch) | server-connection.ts:21-98 | server_connection.rs:129-263 | ✅ | 错误文案逐条对齐；rust 增 build-stamp 判定🔵 |
| standalone 私有服务器(随机密码/ready 行/stdin 租约/TERM→KILL 3s/PRINT_LOGS) | standalone.ts:25-70 | standalone.rs:146-260 | ✅ | 无超时等待 ready（与 ref 一致，G-158） |
| serve 三模式(chdir $HOME/incumbent 幂等/EADDRINUSE 15s 重试/注册/密码策略/公告行) | server-process.ts:56-183 | handlers/serve.rs:76-267 | ✅ | stdio 下清除 OPENCODE_PASSWORD、service 不打印密码(G-213)均对齐 |
| serve 启动 env 旋钮(SIMULATE/MODELS_URL/CONFIG_*/FFF 等) | server-process.ts:118-152 | serve.rs:161-169(BootOptions) | 🟡 | CLI 侧未见逐项透传，需在 oc-server boot 域确认 |
| service-config(文件名/端口/legacy 迁移/get/set/unset) | service-config.ts:17-60+ | service_config.rs:33-349 | ✅ | |
| updater(7 methods/检测/锁/removal) | updater.ts:14-538 | updater.rs(2328) | 🟡 | 结构重组+扩展；七种 v2 方法均保留 |
| upgrade handler(clack UI/已是最新跳过) | upgrade.ts:13-40 | handlers/misc.rs:96-102→updater::upgrade | 🟡 | UX 为纯文本路径，核心逻辑在 updater.rs 内 |
| ssh-askpass(端口/token/confirm/5min 超时/null→1) | ssh-askpass.ts:16-47 | ssh_askpass.rs:12-63 | ⚠️ | prompt 取 argv[1]（ref 取 argv.slice(2)）——有注释的刻意纠偏，与 TS 源码字面不同 |
| env(OPENCODE_PASSWORD/SERVER_PASSWORD/session 剔除) | env.ts:8-19 | util.rs:21-34 | ✅ | |
| database-path(渠道 DB 名/:memory:) | database-path.ts:3-12 | oc-server/DB 域(channel) | ✅ | 经 oc_util::channel 共源 |
| config v1→v2 迁移(copy_on_select/keybind 改名) | config/migrate.ts:22-268 | oc-core/src/cli_config/v1.rs + keymap v1 别名 | 🟡 | v1 config 迁移为别名/子集实现 |
| ACP: 入口(stdout/stdin ndjson+上限+服务器退出竞争) | handlers/acp.ts:13-51 | acp/mod.rs:112-167, transport.rs | ✅ | MiB 上限提示、exit(1)/exit(0) 直退均对齐 |
| ACP: initialize/authenticate(capabilities/terminal-auth meta) | service.ts:106-135 | service.rs:34-88 | ✅ | |
| ACP: new/load/list/delete/resume/close/fork session | service.ts:136-221 | service.rs:91-284 | ✅ | loadSession 失败释放附件、fork 重授权、目录不匹配错误均对齐 |
| ACP: set_config_option/set_mode(withReload 重试) | service.ts:80-85,222-233 | service.rs:287-311,340-369 | ✅ | |
| ACP: prompt/cancel/close(turn 注册、一 prompt 一 turn) | turn.ts:366-399 | turn.rs:94-104,596-612; service.rs:314-332 | ✅ | cancel→stopReason cancelled 语义保留 |
| ACP: turn 执行(server.connected 门/submit 三形态/consume/windDown 5s/abandon) | turn.ts:221-288 | turn.rs:135-278 | ✅ | rust submit 在后台 race（HTTP 语义差异，注释已声明） |
| ACP: 权限 ask(once/always/reject、diff 预览、title 表、locations) | permission.ts:38-141 | permission.rs:18-176+ | ✅ | 统一 diff apply 为自实现并配测试 |
| ACP: elicitation(schema 能力门/取消未展示/答案回填) | elicitation.ts:35-217 | elicitation.rs:20-525 | ✅ | 函数级 1:1 |
| ACP: translate fold/abandon/response/usage_update | translate.ts:76-471 | translate.rs:17-380 | ✅ | |
| ACP: sessions attach/select/watch/detach、MCP 过滤(仅 http) | sessions.ts:42-195 | sessions.rs:20-334 | ✅ | |
| ACP: catalog/config-option/replay/content/tool/child/compaction/directories | 各文件 | 同名 .rs | ✅ | 模块与公开函数逐一对上 |
| 退出码语义(错误1/中断130/serve 0) | run.ts:249; noninteractive.ts:536,684 | run/mod.rs:352-365; noninteractive.rs:709-712 | ✅ | |

### 3. 关键差异与风险 Top 5

1. **⚠️ 非交互权限 TTY 交互提示**（noninteractive.rs:267-295 vs ref 恒自动拒绝）：rust 在交互终端弹 once/always/deny。CI 中默认关闭，但脚本+真 TTY 场景（expect、tmux）会挂起等待输入；靠 `CI`/`OPENCODE_NO_TTY_PROMPT` 兜底。属有意增强但与 ref 默认行为不同，是本域最大行为偏差。
2. **🟡 serve 启动 env 旋钮未在 CLI 侧透传**（OPENCODE_SIMULATE / OPENCODE_MODELS_URL/PATH / OPENCODE_CONFIG_CONTENT / OPENCODE_DISABLE_FFF 等）：若 oc-server boot 内部未读取这些变量，则等价性依赖另一域，建议交叉验证。
3. **🔴 run/v1.ts 桥缺失**：`runV1Bridge` 无对应实现。v2 命令树内无调用方，影响限于嵌入 v1 入口的下游消费者。
4. **⚠️ ssh-askpass prompt 参数位置**：rust 取 argv[1]，ref 读 `argv.slice(2)`。这是对"编译后二进制被 OpenSSH 直接 exec"的纠偏，与 TS 源码字面语义不同。
5. **🟡 附件 MIME 探测简化**：魔数+5 个扩展名。非文本、非图片/pdf 的具名格式（zip 等）ref 给具体 mime 而 rust 给 octet-stream——对模型可见性影响很小，但非等价。

### 4. 缺失文件/功能汇总

| 缺失项 | Ref 位置 | 说明 |
|---|---|---|
| run/v1.ts v1 桥 | run/v1.ts | 无对应（v2 命令树不需要，标🔴备档） |
| heap.ts / cpu-profile.ts | index.ts:17-18,82-83 | V8 专属，Rust 无运行时等价物，合理缺省 |
| Observability/OTEL 层 | index.ts:126-133 | 未在 oc-cli 实现 |
| update-preflight.tsx（opentui 版替换提示） | services/update-preflight.tsx | 以 stderr 纯文本替代，语义近似非渲染等价 |
| web-ui.ts / retained-image.ts / service-registration.ts 的 CLI 侧文件 | services/ | 功能移至 oc-server；RetainedImage 仅 win32 且未实现 |
| api 命令 `param` 多次语义 | commands.ts:114 | rust 为可重复 Vec（ref 不可表达多次），轻微超集 |

**总评**：本域移植完整度非常高——CLI 全 flag 面、run 非交互路径、exit-code 语义、server 连接/降级、serve 三模式、ACP 全 13 方法+turn/permission/elicitation/translate 均达到语义等价，且大量 ref 边界 quirk 被刻意保留并有测试锚定。主要风险集中在五项，其中仅 TTY 权限提示属默认路径行为差异。

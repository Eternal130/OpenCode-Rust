# OpenCode Rust 移植版 全量代码审查汇总报告（复核修正版 v2）

审查基线：`/opt/opencode-rust`（13 crates，约 39 万行）vs `/opt/opencode-ref`（TypeScript v2 锚定版）。
方法：9 条审查线逐文件/逐函数初审 + 4 条复核线对全部报告缺口做二次验证（全 workspace grep 排除"移到别处实现"的误报），未采信任何文档。

## 一、模块完成度总表（维持初审定级）

| 模块 | 完成度 | 主要缺口（经复核修正后） |
|---|---|---|
| oc-core session/engine/tool | ~92% | 无确认缺失 |
| oc-core mcp/integration/config/db/bus | ~90% | MCP protocol 协商未生效；stdio 直启宿主；OAuth 刷新无去重 |
| oc-core 其余子系统 | ~88-92% | persistent-pty 无终端仿真；environment 驱动抽象缺失；/api/fs/find 降级为朴素 walk |
| oc-ai | ~95%（端到端 90%） | alibaba/zai/meta/minimax/moonshot messages 包装缺失；experimental 未移植 |
| oc-tui | ~88-92% | btw 插件缺失；storybook 为 ADR 登记的不移植；动画仅 sustain 段（ignition/release 留 C2） |
| oc-cli | ~92-95% | auth 不开浏览器；UTC 时间戳；无 less 分页/export 选择器；stats 日历注册过的简化 |
| oc-client / oc-server / plugin-host / pty | 85/90/80/85% | effect/solid 客户端变体；workerd；pty 外部二进制 env 路径无校验 |
| oc-schema | ~88-92% | PersistedRevert 解码缺失；3 处结构体 rename 卫生问题（非 wire 缺陷） |
| oc-util | ~50-60%（复核上调） | 真缺失仅 cross-spawn、observability、open 完整版；多数为换机制实现 |

**总体约 90%。**

## 二、复核结论总表

### 确认成立（高优先）
1. **oc-ai Anthropic 系包装缺失**：facade.rs:288-292 把 alibaba/meta/minimax/moonshot/zai messages 全并入通用 AnthropicMessages family；anthropic_messages.rs:319 对无预算 thinking 一律报错，无豁免；meta web_search 重映射无对应。
2. **persistent-pty 无终端仿真**：persistent_pty.rs:1395 仅处理 \n/\r/0x08，CSI/OSC 当可打印字节写入 rows；read/snapshot 基于此模型（oc-tui 的 vt100 未复用）。
3. **environment/workspace 驱动抽象缺失**：LocationRef 保留 workspace_id 字段但无任何按 workspace 路由的驱动层；memory/unavailable 驱动不存在；stdio MCP 直启宿主并继承环境。
4. **MCP config.protocol 不生效**：client.rs:457 固定 pin V_2025_11_25。
5. **MCP OAuth 并发刷新无去重**：oauth.rs:840 无状态自由函数，无 per-token in-flight 锁，rotation 下并发刷新会 invalid_grant。
6. **/api/fs/find 降级**：rust handlers/fs.rs:285 为朴素子串 walk；fff 模糊索引、ripgrep 后备层、protected.ts 敏感目录表（home/Desktop/.ssh 排除）均缺失。
7. **OTel 完全缺失**：全 workspace 无 opentelemetry，连 tracing crate 都未引入。
8. **表单 TTL 死代码 + is_date_time 恒真**：RETENTION_MS 定义后零使用，已结算表单永久驻留；is_date_time(_)->true。
9. **biome formatter 解析差异**：node_modules/.bin/biome vs npm.which("@biomejs/biome")+自动安装；有 biome.json 但未装包的项目 rust 静默不格式化。
10. **CLI auth login 不开浏览器**（全 workspace 无 OAuth URL 打开路径）；**session list 硬编码 UTC**（函数名 format_local_time 与行为相反）。
11. **oc-pty 外部二进制 env 路径无校验**：OPENCODE_PTY_BIN 直接采用，比初审所述更弱（连可执行位都不查；exe-sibling/PATH 候选仅查可执行位；无 sha256/属主/symlink 校验）。
12. **workerd（Cloudflare DO）profile 无对应**（仅注释引用）。
13. **tool_output 截断预算 char 计数 vs ref UTF-16 length**：emoji/代理对下裁剪点不一致。
14. **btw 插件缺失**（keymap table.rs:615 自认 side-question mode absent）。

### 部分确认（措辞修正）
15. **3 个 schema rename 问题降级为"结构体卫生"**：projectID/uriTemplate/forceRequired 在 oc-schema 结构体上确缺 rename，但所有线上路径均被 handler 手工 json!/生成端 rename/契约测试兜底（wire_fidelity.rs、parity_http_contract.rs、parity_vcs_wire.rs 钉死），无实际 wire 缺陷；forceRequired 结构体本身是死代码。
16. **v1 兼容层**：主体（v1_migration.rs 1959 行 + /api/experimental/migration/v1 + battery 测试）存在；缺的是 PersistedRevert 解码（revert 列原样透传、聚合恒 None）。
17. **run attached:false**：字面差异属实，但 standalone run 独占服务器的语义辩护成立且有测试；--server 共享路径组合待核。
18. **LocationActivity**：bus 自动 touch、清扫期 re-touch 复查确认缺失；但 "project.activate 心跳" 子项推翻（ref 本就无此行为）。
19. **util 2.4k 行**：npm→bun 子进程委托、patch→oc-core/tool/patch.rs、bom→file_mutation、calendar→注册过的简化、title fallback→title.rs；真缺失仅 cross-spawn-spawner、observability、open 完整版。
20. **reqwest 0.12/0.13 双版本**：确认（0.13 = rmcp + oc-core MCP 客户端，进发布闭包）；但"双 TLS 栈"说法不准——两版均 rustls，是同一 TLS 栈编译两份 reqwest。
21. **opentui 双轨**：双份 *_solid.rs 约 9585 行属实、其余 lane 仍有 ~10 个 env flag；但"默认关"结论反转——footer 等默认 solid ON（env=0 才回退），OC_TABS_OPENTUI 已退役、tabs 已单轨化（solid 是唯一渲染器）。收敛方向正确，债务在剩余 lane。

### 推翻（从缺口清单剔除）
22. rpc.ts / event-manifest / durable-event-manifest"缺失"——均有对应：oc-server/plugins.rs:140（rpc dispatch）、oc-server/event_manifest.rs:1（镜像+drift 测试）、oc-core/bus/registry.rs:1（durable manifest 镜像）。
23. "TUI 无 /update 或更新 UI"——oc-tui/components/update.rs 完整实现（横幅+对话框+SSE 通道），autoupdate 默认 notify 非关闭。
24. "SSE 编码失败只踢受影响订阅者"——与 ref 一致，编码失败踢全部当前订阅者（sse.rs:208 注释+测试）；仅 lag 溢出个别终结，与 ref 一致。
25. "glob 无 brace 导致与 ref 分叉"——权限匹配两侧走同样的 wildcard 转义（ref 本身不支持 brace）；ripgrep 调用两侧参数完全一致。
26. "shell kill 无条件 SIGKILL"——TERM→3s→KILL 与 ref 一致（仅理论上 pid 复用微差）。
27. "@octokit/@pierre 无对应"——使用点在 packages/function（SST 云函数）与 session-ui（Web UI），不在移植范围。
28. chromiumoxide/defmt"进入发布闭包"——chromiumoxide 仅 tools/parity dev-dep；defmt 是 jiff 未启用的 optional feature；cmake 经 aws-lc-sys 是 build-dep（会出现在构建链，非运行期）。
29. "ULID ID 格式可能不兼容"——手写 identifier.rs 逐位兼容 ref 自定义 62 字母表格式（长度/时间位/字符表/降序取反全对齐）。

### 维持确认的低风险项
- serde_yaml 停维、schemars 0.8、notify 6、dirs 5、dialoguer 0.11 落后主版本（版本号全部核实）。
- flock 降级为 flock(2)：仅 Unix、无 stale-break 破锁（ref 为用户态 mkdir 锁+60s 破锁）。
- session list 无 less 分页、export 无交互选择器、models 排序差异。
- 实验性：oc-ai experimental/（evaluation、system-one）未移植。

## 三、修复优先级（修正版）

1. **立即**（真实缺陷、低成本）：
   - MCP OAuth per-token in-flight 刷新去重（registry.rs prepare_token 加锁）；
   - forms.rs RETENTION_MS 淘汰任务接上 + is_date_time 实现日期解析；
   - session list 时区（项目已有 jiff，非零成本借口不成立）；auth login 开浏览器（xdg-open 等）；
   - oc-pty OPENCODE_PTY_BIN 路径校验。
2. **短期**：oc-ai customization 增加 anthropic body-wrap hook（一次修复 alibaba/zai/meta/minimax/moonshot）；MCP protocol 协商；/api/fs/find 补 protected 目录排除（哪怕先不做模糊索引）；biome 按包名解析。
3. **中期**：persistent-pty 终端仿真（vt100 crate 已在 workspace 内，可复用）；environment/workspace 驱动层决策（做或明确不做）；PersistedRevert 解码；LocationActivity touch 源。
4. **长期**：opentui 剩余 lane 单轨化（方向已明确且进行中）；reqwest 0.13 统一窗口；OTel/tracing 骨架；serde_yaml 迁移；cross-spawn/open 补齐。

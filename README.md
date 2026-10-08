# opencode-rust

opencode v2 CLI 的 Rust 全量移植。

- **参照版本**: opencode `v2.0.24`（源码 worktree: `/opt/opencode-ref`，原仓库 `/opt/opencode`）
- **范围**: CLI + 服务端 + TUI + 插件系统（不含桌面端）
- **目标**: 功能与外观与 v2.0.24 一致，全线 Rust 重写，插件支持等同
- **方案**: 见 [MIGRATION.md](./MIGRATION.md)

## 工具链

- Rust stable（rustup 管理），clippy / rustfmt / nextest

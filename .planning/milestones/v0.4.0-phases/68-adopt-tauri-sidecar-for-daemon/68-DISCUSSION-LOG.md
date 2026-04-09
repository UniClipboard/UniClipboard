# Phase 68: Adopt Tauri Sidecar for daemon binary management - Discussion Log

> **Audit trail only.** Do not use as input to planning, research, or execution agents.
> Decisions are captured in CONTEXT.md — this log preserves the alternatives considered.

**Date:** 2026-03-28
**Phase:** 68-adopt-tauri-sidecar-for-daemon
**Areas discussed:** Sidecar vs 自定义打包, 开发构建自动化, CLI 路径解析策略, stdin pipe 兼容性

---

## Sidecar vs 自定义打包

| Option                         | Description                                                                               | Selected |
| ------------------------------ | ----------------------------------------------------------------------------------------- | -------- |
| Tauri externalBin              | 在 tauri.conf.json 声明 externalBin，Tauri 自动打包进 app bundle，需按 target-triple 命名 | ✓        |
| 自定义 CI + beforeBuildCommand | 保持 std::process::Command，在 CI 和 beforeBuildCommand 中手动 copy daemon                |          |
| Tauri externalBin + 自定义启动 | 用 externalBin 解决打包，但路径解析后继续用 std::process::Command                         |          |

**User's choice:** Tauri externalBin
**Notes:** None

### Follow-up: 启动方式

| Option                 | Description                                                                  | Selected |
| ---------------------- | ---------------------------------------------------------------------------- | -------- |
| Tauri sidecar API      | 完全用 app.shell().sidecar()，路径自动解析                                   | ✓        |
| 只用打包，保留 Command | 用 externalBin 打包但从 sidecar 路径解析出绝对路径后用 std::process::Command |          |
| 你来决定               | 让 Claude 研究后决定                                                         |          |

**User's choice:** Tauri sidecar API
**Notes:** None

---

## 开发构建自动化

| Option                          | Description                                                             | Selected |
| ------------------------------- | ----------------------------------------------------------------------- | -------- |
| beforeDevCommand 加 cargo build | 在 tauri.conf.json 的 beforeDevCommand 中加入 daemon 编译               |          |
| build.rs 拷贝脚本               | 在 uc-tauri 的 build.rs 中编译后将 daemon 二进制 copy 到 binaries/ 目录 | ✓        |
| 你来决定                        | 让 Claude 研究后决定                                                    |          |

**User's choice:** build.rs 拷贝脚本
**Notes:** None

---

## CLI 路径解析策略

| Option                    | Description                                       | Selected |
| ------------------------- | ------------------------------------------------- | -------- |
| 保持现状，后续 phase 处理 | Phase 68 只改 GUI 侧，CLI 的 sibling 解析逻辑不动 | ✓        |
| 抽取共享解析模块          | 顺便把路径解析抽到 uc-daemon-client               |          |

**User's choice:** 保持现状，后续 phase 处理
**Notes:** User envisions future `brew install uniclipboard` with single command entry point. Daemon invisible to users. This is out of Phase 68 scope.

---

## stdin pipe 兼容性

| Option               | Description                                                                            | Selected |
| -------------------- | -------------------------------------------------------------------------------------- | -------- |
| 迁移到 sidecar stdin | Tauri sidecar 支持 .write(bytes)，将现有 stdin pipe tether 迁移到 sidecar CommandChild | ✓        |
| 改用 kill PID        | 不依赖 stdin pipe，改用 sidecar kill() 或 SIGTERM                                      |          |
| 你来决定             | 让 Claude 研究后决定                                                                   |          |

**User's choice:** 迁移到 sidecar stdin
**Notes:** None

---

## Claude's Discretion

- build.rs 实现细节（target triple 检测、copy 逻辑）
- tauri-plugin-shell 权限配置
- supervision loop 从 std::process::Child 到 sidecar CommandChild 的适配

## Deferred Ideas

- Unified CLI distribution via Homebrew (single `uniclipboard` command)
- CLI daemon path resolution unification

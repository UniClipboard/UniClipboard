# 桌面启动装配

`uc-bootstrap` 准备桌面宿主能力。核心业务的依赖装配和生命周期属于独立 Engine。旧 Tauri 宿主的装配方式已退役；迁移记录见 [Tauri 桌面宿主退役记录](gui-go-tauri-retirement.md)。

## 责任边界

| 所有者 | 职责 | 源码入口 |
| --- | --- | --- |
| `uc-bootstrap` | 解析桌面目录，准备剪贴板、安全存储、文件访问与观测接入 | `crates/uc-bootstrap/src/wiring/desktop_host.rs` |
| daemon | 启动 Engine，消费事件，提供本机服务并处理进程退出 | `apps/daemon/src/daemon/host.rs` |
| Engine | 装配数据库、加密、搜索和网络，管理业务任务与生命周期 | `uc-engine` 公开接口 |
| Go/Wails 宿主 | 管理窗口、托盘和系统交互，通过 daemon 访问业务 | `apps/gui-go/` |

桌面代码只使用 `uc-engine` 的公开接口。不得在 `uc-bootstrap` 中重建 Engine 内部的 Port 装配或直接依赖 `uc-core`、`uc-application`、`uc-infra`。该边界由 [Engine 消费者检查](engine-repository-checks.md) 验证。

## 启动流程

1. daemon 调用 `prepare_desktop_engine_host()`，获得 `DesktopEngineHost`。
2. daemon 保留所需的进程路径、文件句柄与观测接入。
3. `DesktopEngineHost::into_engine_start()` 返回 `EngineConfig` 和 `HostCapabilities`。
4. daemon 调用 `Engine::start_with_progress()`，向客户端报告启动进度。
5. daemon 消费 Engine 事件，并把客户端操作交给 Engine。

资料升级与启动进度见 [桌面启动与资料升级进度](startup-upgrade-progress.md)。生命周期合同见 [跨平台核心接口](uc-engine-interface.md)。

开发工具通过 `build_cli_engine_runtime()` 启动进程内 Engine。该入口位于 `crates/uc-bootstrap/src/entrypoint/cli.rs`，仅服务开发工具；正式 `uniclip` 通过 daemon 访问业务。

## 修改规则

- 修改目录、安全存储或系统剪贴板接入时，修改桌面适配层。
- 修改身份、配对、存储、搜索或传输规则时，在 Engine 仓库实现。
- 修改 GUI 系统命令时，修改 Go `HostService` 并重新生成合同。
- 修改本机 API 时，同步 daemon 合同与客户端。

运行对应的端到端验证，并保留可重复的验证工件。宿主适配检查不能代替跨设备同步或原生界面验收。

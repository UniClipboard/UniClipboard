# 模块边界

本文列出桌面仓库当前的模块所有权。完整约束见 [架构规则](../agent/architecture-rules.md)；核心公开契约见 [uc-engine 接口](uc-engine-interface.md)。

## Engine 所有权

`uc-core`、`uc-application`、`uc-infra` 和 `uc-engine` 属于独立仓库 `UniClipboard/Engine`。数据库迁移、加密存储、搜索、P2P 协议和核心后台任务均在 Engine 中维护。

桌面仓库通过根 `Cargo.toml` 的唯一固定提交消费 `uc-engine`。桌面代码只能使用其公开导出。不得依赖 Engine 内部 crate、内部模块路径或源码位置。需要改变核心能力时，先在 Engine 实现并验证，再更新桌面的固定提交。

## 桌面模块

| 模块 | 职责 | 边界 |
| --- | --- | --- |
| `apps/gui-go` | Go/Wails 窗口、托盘、快捷键、宿主命令和本机主题 | 通过 daemon API 使用业务能力，不打开业务数据库 |
| `apps/gui-go/frontend` | React 界面、交互和显示状态 | 通过宿主桥接与 daemon API 操作，不复制核心业务规则 |
| `apps/cli-go` | 用户 CLI `uniclip` | 作为 daemon 客户端运行 |
| `apps/daemon` | `uniclipd` 进程入口和生命周期 | 不依赖 GUI 框架 |
| `crates/uc-bootstrap` | 准备宿主能力、启动 Engine、接入观测能力 | 不承担核心业务决策 |
| `crates/uc-platform` | 系统剪贴板、安全存储等平台能力 | 实现 Engine 公开宿主契约，不分叉核心协议 |
| `crates/uc-webserver` | daemon HTTP/WS 路由、鉴权和传输映射 | 通过 `uc-engine` 执行业务操作 |
| `crates/uc-daemon-contract` | daemon 请求、响应、事件和稳定错误 | 保持客户端可用，不引入 Engine 和 GUI 重依赖 |
| `crates/uc-daemon-client` | Rust daemon 客户端 | 不直接访问核心或数据库 |
| `crates/uc-daemon-process`、`crates/uc-daemon-local` | daemon 进程控制、本机发现与连接元数据 | 保持客户端可用，不引入 GUI 框架 |
| `crates/uc-desktop-settings` | 桌面产品设置及旧设置迁移 | 不扩展为核心用户内容存储 |
| `crates/uc-app-paths` | 平台目录解析 | 不持有业务状态 |
| `crates/uc-observability` | 桌面日志、诊断和产品分析接入 | 核心观测契约从 `uc_engine::observability` 导入 |
| `apps/quick-panel`、`crates/quick-panel-core` | 原生快捷面板与共享面板逻辑 | 通过 daemon API 读取历史与执行操作 |
| `tools/uc-dev-cli` | 开发诊断 CLI | 不进入生产构建 |

## 依赖方向

```text
Go/Wails GUI / Go CLI / native quick panel
    -> daemon HTTP/WS API
    -> uc-webserver -> uc-engine -> Engine internals

uc-bootstrap -> uc-engine
uc-platform -> Engine public host contract
```

进程边界、启动和关闭顺序见 [启动架构](bootstrap.md)。修改各模块前，按根 [AGENTS.md](../../AGENTS.md) 加载对应规则。旧架构决策保留在 ADR 中，不作为当前依赖清单。

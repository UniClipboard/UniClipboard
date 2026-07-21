# Plan 003：建立唯一的跨平台核心入口并让桌面先使用

> **执行要求**：先在当前仓库内完成，不要在本计划中拆 Git 仓库。外部宿主只能依赖新入口，不得继续透出 `AppFacade`、`CliAppRuntime` 或内部 port。
>
> **漂移检查**：`git diff --stat 1c229e9e1..HEAD -- crates/uc-core crates/uc-application crates/uc-infra crates/uc-bootstrap crates/uc-desktop apps src-tauri`

## 状态

- **优先级**：P0
- **工作量**：L
- **风险**：HIGH
- **依赖**：`plans/002-prove-four-platform-full-node.md`
- **类别**：tech-debt
- **计划基线**：`1c229e9e1`，2026-07-19

## 为什么必须做

鸿蒙桥接当前直接持有 `CliAppRuntime` 和 `AppFacade`，自行恢复会话、维持连接、订阅入站、解码内容、组装文件和管理任务。现有 `AppFacade` 又公开大量内部子入口。若把这些原样发布，四个平台会各自复制编排逻辑，核心仓库仍然没有稳定入口。

## 目标形态

新增唯一公开 crate（最终名称在 ADR 中确定，本文暂用 `uc-engine`），对宿主只提供：

```rust
Engine::start(config, host) -> (Engine, EventStream)
Engine::execute(Operation) -> OperationResult
Engine::quiesce(deadline)
Engine::suspend()
Engine::resume()
Engine::shutdown(deadline)
```

`Operation` 覆盖创建/加入空间、邀请、设备、发送文本/图片/文件、查询历史与显式导出。事件覆盖状态、入站内容、传输进度、需要重新查询和不可恢复错误。

生命周期遵守 ADR-005 的唯一状态机：`quiesce` 停止接收新操作并在期限后取消未完成工作；`suspend` 释放节点；`resume` 保持同一实例、身份和事件流。系统杀进程后的恢复调用 `start`。被取消的收发必须报告失败，不能在恢复后自动继续。

## 当前事实

- `crates/uc-bootstrap/src/entrypoint/non_gui.rs:529-675` 的 `CliAppRuntime` 固定装配桌面路径、分析、搜索、移动 LAN 和完整 iroh 能力。
- `crates/uc-application/src/facade/app_facade.rs:85-138` 向调用方公开众多内部字段。
- `crates/uc-application/Cargo.toml:14-19` 正式依赖 `uc-infra` 与 `uc-observability`，阻碍独立分层。
- `crates/uc-bootstrap/src/layer/platform.rs:83-150` 直接创建桌面剪贴板。

## 范围

**允许修改**：

- 新建 `crates/uc-engine/`
- `crates/uc-core/` 中由真实用例拉出的宿主能力接口
- `crates/uc-application/` 的依赖方向与公开入口
- `crates/uc-infra/` 的内部实现拆分
- `crates/uc-bootstrap/` 降级为桌面宿主装配
- `crates/uc-desktop/`、`apps/daemon/`、`apps/cli/`、`src-tauri/` 的调用迁移

**禁止修改**：

- UI 功能和视觉行为
- P2P 协议字节形态
- 数据库明文规则
- 移动 LAN 兼容实现
- 本计划内创建独立 Git 仓库

## 提交顺序

1. `arch:` 固化宿主动作、结果、错误、事件和生命周期契约。
2. `arch:` 只增加上述用例实际需要的宿主能力接口。
3. `refactor:` 清除 `uc-application -> uc-infra` 正式依赖。
4. `feat:` 新增 `uc-engine` 与生命周期测试。
5. `refactor:` 桌面 daemon 改用 `uc-engine`。
6. `refactor:` CLI 与 Tauri 调用改用 daemon client 或 `uc-engine`，删除旧入口。

接口与具体平台实现不得放在同一提交。

## 步骤

### 1. 先固化用例与生命周期契约

从宿主真实动作开始，逐项定义创建、加入、邀请、发送、查询、显式导出、用户主动重发、暂停、恢复和关闭的输入、输出、错误与事件。先写接口级契约测试，再由这些用例反推边界；不得先设计未来可能使用的 port 或领域类型。

**验证**：每个公开操作都有调用前置、成功结果、失败结果和事件断言；生命周期测试覆盖合法与非法状态转换、deadline 到期取消、系统杀进程后重新 `start`，以及恢复后不自动续投。

### 2. 按用例收敛宿主能力

只有步骤 1 的用例实际需要外部系统时，才增加对应能力。宿主最终只提供私有数据/缓存/临时目录、系统安全存储、剪贴板读写与通知、文件导入/导出句柄、生命周期与网络变化通知。核心生成和解释设备身份，并拥有数据库、blob、搜索索引、传输临时区和所有持久化格式；文件内容可按原始字节落盘，其他业务内容仍须加密。

**验证**：`cargo check -p uc-core` 通过，且新增接口不引用 Tauri、JNI、Swift、ArkTS、SQLite 或 iroh 类型；公开 `uc-engine` 边界不出现内部 port、facade 或并发实现类型。

### 3. 清除应用层反向依赖

把搜索投影和文件清理等当前从 `uc-application` 直接调用的具体实现改为真实用例要求的小接口，或移回拥有该规则的层。分析事件改为宿主可选观察者，不让应用层依赖产品分析实现。

**验证**：`cargo tree -p uc-application -i uc-infra` 和 `cargo tree -p uc-application -i uc-observability` 均无正式依赖路径；`cargo test -p uc-application` 全部通过。

### 4. 实现唯一核心入口

核心入口内部拥有装配、任务、事件回压和状态机。事件消费者落后时必须收到“需要重新查询”，不能静默跳过。错误只暴露稳定类别、编号和可重试性，详细原因进入脱敏日志。

**验证**：新增接口级测试覆盖未初始化、创建、加入、发送、接收、暂停、恢复、重复启动、事件落后、关闭 deadline，以及所有被取消操作都进入终态且不会在恢复后自动继续。

### 5. 让桌面先迁移

daemon 是第一个生产消费者。迁移后桌面行为保持不变，`uc-bootstrap` 只负责日志、分析、桌面路径和系统适配器，再调用 `uc-engine`。完成切换后删除被替代的公开入口，不保留运行时双轨。

**验证**：

```bash
cargo test -p uc-engine
cargo test -p uc-bootstrap
cargo check -p uc-daemon -p uc-desktop -p uc-tauri -p uc-cli
rg -n 'CliAppRuntime|pub app_facade|pub .*AppDeps' apps crates/uc-desktop src-tauri/crates/uc-tauri
```

预期前三条通过；最后一条在生产调用路径中无匹配。

### 6. 固化接口契约

为公开操作、事件、错误、生命周期顺序和线程要求写中文参考文档，并加入兼容性测试。内部类型不得出现在生成的 Swift/Kotlin/ArkTS 接口中。

## 完成标准

- [x] 桌面生产路径只通过 `uc-engine` 使用核心能力。
- [x] `uc-application` 不再正式依赖 `uc-infra` 或产品分析实现。
- [x] 核心可反复暂停、恢复、关闭和重新启动。
- [x] 平台不接触业务明文持久化。
- [x] 旧的 `CliAppRuntime` 外部路径已删除，而不是并存。
- [x] 接口级和现有双节点测试全部通过。

## 停止条件

- 新入口需要向宿主公开内部 port、SQLite、iroh 或任务句柄。
- 为兼容桌面而把 Tauri、daemon HTTP 或产品分析放入核心。
- 事件丢失只能通过无限队列解决。
- 迁移需要长期保留旧、新两套装配。

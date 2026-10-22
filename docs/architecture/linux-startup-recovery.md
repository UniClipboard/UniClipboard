# Linux 登录启动与失败恢复

## 权责与安全边界

Desktop 平台适配器负责系统密钥库探测。Engine 继续通过 `HostCapabilities` 使用该适配器。此修复不修改 Engine，也不修改 Engine 固定提交。

密钥库探测不得把 System 存储切换为文件存储。原有严格二进制拒绝回退已删除。既有的文件存储选择与旧文件条目协调仍由 `secure_storage/legacy_file.rs` 管理。此修复不迁移、不重置、不删除任何密钥或资料。

严格拒绝二进制数据的后端仍会失败。此时需要修复后端兼容性。不得通过更换存储身份掩盖失败。

## 有界探测

Linux 探测在单个工作线程中执行。所有尝试共享三秒预算。两次尝试之间等待 200 毫秒。

仅在 D-Bus 服务名称尚不可用时重试。支持 `NameHasNoOwner`、`ServiceUnknown` 及其已知消息形式。权限拒绝、锁定、取消提示、二进制拒绝、字节不一致及其他错误不自动重试。

外层等待同样受三秒预算限制。同步 D-Bus 调用超时后，原工作线程可能等待底层调用返回。当前进程缓存最终失败，不再创建重叠探测线程。日志记录尝试次数、耗时及终止原因。宿主不自动循环重启 daemon。

## 启动实例归属

`daemon-startup.conn` 与 `/startup` 同时携带 `instanceId`。连接文件已有的 `pid` 与新增的响应 `pid` 必须一致。Go 创建 daemon 子进程前生成随机运行标识，并通过 `UC_DAEMON_STARTUP_ID` 传递。直接启动的 daemon 自行生成运行标识。

Go 仅接受存活 daemon 的快照。读取前后必须保持同一连接记录。响应的 PID 和运行标识必须匹配该记录。等待自己启动的子进程时，还必须匹配该子进程的 PID 与运行标识。时间戳仅用于诊断，不参与归属判定。

缺少运行标识的旧版启动记录不能提供可归属的失败快照。存活旧进程仍阻止竞争启动。宿主在等待预算耗尽后显示恢复入口。

复用探测同时检查业务连接、启动连接和进程元数据。健康探测超时不能证明进程已经退出。Go 子进程设置 `UC_DAEMON_NO_TAKEOVER=1`，防止并发启动驱逐另一个所有者。受控模式切换必须等到旧进程退出后再启动替代进程。

## 恢复与退出

可重试的失败保留原生恢复对话框。用户可以重试或退出。宿主在 daemon 就绪前不创建业务窗口。

宿主保留实际创建的子进程句柄。仅当连接记录匹配该子进程时，宿主才通过认证的 `POST /startup/exit` 请求释放终止状态的实例。服务端同时校验 bearer token、运行标识与终止状态。启动中的实例和就绪实例拒绝该请求。

宿主必须等待该子进程退出，然后才能重新启动。宿主不向发现的旧进程发送信号。复用的失败 daemon 保持原所有权；重试只重新检查它。启动未完成时退出宿主，也不得通过普通停止路径终止未连接的 daemon。

PR #2078 的常驻宿主继续负责上述流程。UI 子进程不获得启动、停止或重试 daemon 的独立所有权。

## 非视觉复跑

从仓库根目录执行：

```bash
python3 packages/desktop-host-go/e2e/startup/run.py
python3 packages/desktop-host-go/e2e/startup/readiness_model.py
python3 packages/desktop-host-go/e2e/startup/secret_service.py
```

第一个驱动运行真实子进程与 HTTP 协议替身，保存 JSON 结果及构建来源。第二个驱动在独立进程中对生产探测策略注入故障。两者都不访问真实密钥库、剪贴板或 GUI。

第三个驱动需要 Linux、`dbus-run-session` 和 `gnome-keyring-daemon`。它在私有 D-Bus、独立 HOME 和独立资料中使用真实系统存储提供者。缺少条件时返回退出码 77。它覆盖延迟启动与持续缺失，不证明 KDE 登录项和 KWallet 的实际行为。

额外验证真实 Rust 启动服务：

```bash
cargo build -p uc-webserver --example startup_service
UC_STARTUP_E2E_REAL_SERVICE="$PWD/target/debug/examples/startup_service" \
  python3 packages/desktop-host-go/e2e/startup/run.py
```

`crates/uc-webserver/examples/startup_service.rs` 提供真实 Rust 启动 HTTP 服务。该例子不启动 Engine、密钥库、剪贴板或 GUI。通过 `UC_STARTUP_E2E_ROOT` 指定临时目录。

真实 KDE 登录、自启动、原生对话框操作和 PR #2078 的跨平台视觉生命周期仍须人工验收。

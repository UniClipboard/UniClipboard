# Plan 002：证明完整节点可在四个平台构建并反复启停

> **执行要求**：本计划是可行性硬门槛，不得用删掉文件传输、关闭 relay、替换协议或调用 LAN HTTP 来获得绿色结果。完成后更新 `plans/README.md`。
>
> **漂移检查**：`git diff --stat 1c229e9e1..HEAD -- Cargo.toml crates/uc-infra crates/uc-platform crates/uc-bootstrap tests .github/workflows`

## 状态

- **优先级**：P0
- **工作量**：L
- **风险**：HIGH
- **依赖**：`plans/001-record-four-platform-p2p-decision.md`
- **类别**：migration
- **计划基线**：`1c229e9e1`，2026-07-19

## 为什么必须先做

HarmonyOS 已证明完整节点方向可行，但 Android 和 iOS 尚未跑过同一完整栈。当前实测显示 `iroh`、加密、SQLite 与 blob 依赖已能分别编译到 Android 和 iOS 的 `uc-infra`，两端都在 `crates/uc-infra/src/fs/atomic_publish.rs:185` 的 Linux 专用 errno 调用处失败；继续构建还会在仅支持桌面系统的 `LocalClipboard` 处失败。这些是明确的可修复平台假设，但真机网络、文件和生命周期仍未证明。

## 当前事实

- 根 `Cargo.toml:71-77` 通过本地路径使用定制 `iroh-blobs`；独立发布必须可复现这份修复。
- `crates/uc-infra/src/fs/atomic_publish.rs:177-186` 在宽泛 Unix 分支里调用 Linux 专用函数。
- `crates/uc-platform/src/clipboard/platform/mod.rs:14-24` 只定义 macOS、Windows、Linux 剪贴板。
- `crates/uc-infra/src/network/iroh/node.rs:516-554` 在生产构建中禁止同一进程第二次启动节点。
- 现有双节点测试模式在 `crates/uc-bootstrap/tests/slice1_handshake_e2e.rs`、`slice2_phase1_presence_e2e.rs`、`slice2_phase2_clipboard_e2e.rs`。

## 命令基线

| 目的 | 命令 | 成功结果 |
|---|---|---|
| 桌面核心 | `cargo test -p uc-core -p uc-application -p uc-infra -p uc-bootstrap` | 全部通过 |
| iOS 编译 | `cargo check -p uc-infra --target aarch64-apple-ios` | 退出 0 |
| Android 编译 | `cargo ndk -t arm64-v8a check -p uc-infra` | 退出 0 |
| 完整节点测试 | `cargo test -p uc-bootstrap --test slice1_handshake_e2e --test slice2_phase1_presence_e2e --test slice2_phase2_clipboard_e2e` | 全部通过 |

## 范围

**允许修改**：

- `Cargo.toml`、`Cargo.lock`、`.gitmodules` 与定制网络依赖交付所需文件
- `crates/uc-infra/src/fs/atomic_publish.rs`
- `crates/uc-infra/src/network/iroh/`
- `crates/uc-platform/` 中为宿主注入做准备的代码
- `crates/uc-bootstrap/` 中生命周期与测试装配
- 新增四平台构建检查、最小宿主和端到端测试

**禁止修改**：

- P2P 线协议和加密算法
- 通过 feature 删除图片、文件、relay 或 blob 能力
- 正式路径中的明文密钥或业务数据回退
- 现有 LAN HTTP 客户端代码

## 步骤

### 1. 让定制网络依赖可独立复现

选择一种单一来源：把修复合入可固定版本的上游/官方 fork，或把 fork 作为核心仓库拥有的正常依赖。禁止桌面使用本地补丁而移动使用 crates.io 原版。记录固定提交与许可证。

**验证**：在一个无父仓库、无预初始化子模块的临时检出中运行 `cargo metadata --locked --format-version 1`，预期退出 0。

当前证据：从提交 `1c48d7e37` 使用 `git archive` 生成独立临时检出，未初始化任何子模块，`cargo metadata --locked --format-version 1` 退出 0。iOS 与 Android 依赖树均解析到 `iroh 1.0.0-rc.1` 和 `iroh-blobs` 固定提交 `b33af91e8a4bd189cc0f954fc6584feb5ffd1823`。

### 2. 修复移动目标的源码平台假设

- 把 no-replace 文件发布实现按真实系统能力划分；Android、iOS 不得调用 Linux 私有 errno 符号。
- 不给 `uc-platform` 伪造移动剪贴板。最小节点使用宿主注入的 no-op/测试适配器，真正移动剪贴板留到计划 004。
- 把产品分析和桌面日志实现从移动最小节点依赖图中排除，只保留结构化日志入口。

**验证**：依次运行 iOS、Android 编译命令，预期均退出 0，且 `cargo tree` 中包含同版本 `iroh` 与同一 `iroh-blobs` 来源。

### 3. 支持同进程反复启停

把永久 `OnceLock` 改成由节点运行实例拥有的互斥与状态管理：同一时刻第二次启动返回稳定错误；完成关闭后允许再次启动。所有后台任务必须收到取消信号并在 deadline 内停止。

新增生产形态测试：同一进程执行十次 `start -> shutdown -> start`，设备身份不变、端口释放、任务数和文件句柄不持续增长。

**验证**：运行新增测试，预期十轮全部成功；测试不得启用绕过生产守卫的 `test-util` 行为来伪造结果。

当前自动证据：`cargo test -p uc-engine --test host_adapter_contract production_engine_restarts_ten_times_with_the_same_network_identity -- --exact`。该测试使用生产 `Engine::start`，连续十轮创建或解锁后关闭，并核对安全存储中的网络身份不变。

### 4. 建立四平台最小真机实验

每个平台的最小宿主只做：注入私有目录和系统安全存储、启动节点、创建或加入空间、发送和接收文本/图片/文件、暂停、恢复、关闭。不得加入产品 UI。

验证桌面分别与 iOS、Android、HarmonyOS：

- 创建与加入空间
- 本地直连与 relay
- 双向文本、图片、文件
- 切换 Wi-Fi/蜂窝网络
- 前后台切换、锁屏、系统回收后恢复
- 恢复后设备身份不变

### 5. 加入明文探针

每次实验写入唯一测试正文、文件名、标签和预览。关闭节点后扫描数据库、缓存、搜索索引、临时目录和日志；文件内容本体允许按原始字节存在于 blob store 或核心导入目录，但文件名、路径、标签、预览和其他内容在任何位置发现明文仍视为失败。

**验证**：探针脚本退出 0，并在测试报告中列出扫描过的目录与文件类型，不记录真实用户内容。

扫描器与当前自动覆盖见 [`docs/development/plaintext-probe.md`](../docs/development/plaintext-probe.md)。桌面文本、图片、预览和正式日志目录已进入自动测试；文件名、文件路径、标签及三种移动真机目录仍未验收，文件内容本体不属于明文扫描失败项。

## 完成标准

- [ ] 四平台构建使用相同网络、协议、加密与 blob 依赖。
- [ ] 四平台均完成真实 P2P 配对和双向文本、图片、文件传输。
- [ ] 移动暂停时可离线，恢复时以原身份重新成为节点。
- [x] 同一进程反复启停十次通过。
- [ ] 明文探针扫描通过。
- [ ] 没有依靠 LAN HTTP 完成任何验收项。

## 停止条件

- 任一移动平台必须改线协议或关闭文件传输才能运行。
- iOS 目标被定义为永久后台在线，而不是系统允许窗口内的完整节点。
- 定制网络依赖无法以固定、可审计来源发布。
- 任何正式移动实现只能使用明文文件保存密钥。
- 真机发现同一协议在平台间产生不同字节结果。

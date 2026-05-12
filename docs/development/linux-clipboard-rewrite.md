# Linux 剪贴板原生重写 — 跨 session 交接文档

> 本文档为新 session 接续此项工程而准备。读完本文之后，无需任何先验上下文即可直接动手。

最后更新：2026-05-10（**Phase 3 + Phase 4 落地**；五个 Phase 全部完成 ✅）

---

## 0. 文档目的

UniClipboard Linux 剪贴板后端做了一次彻底的"脱离 `clipboard-rs`、原生绑两套 OS 协议（Wayland + X11）"的工程。**所有 5 个 Phase（1 / 2a / 2b / 3 / 4 / 5）已全部落地**。本文档为该项目的最终归档，记录了：

- 任务背景与起因
- 目前的发现与决策
- 已完成的工作（含 commit 哈希）
- 剩余阶段的详细计划
- 已知的坑与未解决问题
- 验收标准

读到本文末尾后，新 session 的第一件事应该是：

```bash
git log --oneline -10
```

确认 Phase 1/2/3/4/5 的相关 commit 全部在当前 branch。Phase 3 + 4 是同一天落地的（2026-05-10）；最近的 clipboard-rs Linux 解除引用 commit 即 Phase 4 收尾。

---

## 1. 任务背景

### 1.1 起因

用户在 Fedora 44 + niri 25.11（Wayland 会话）下运行 UniClipboard，发现"复制了文本但 daemon 监听不到、对端无任何反应"。

### 1.2 根因

UniClipboard Linux 平台层依赖 `clipboard-rs 0.3.3`：

- `crates/uc-platform/src/clipboard/platform/linux.rs`：`LinuxClipboard` 用 `clipboard_rs::ClipboardContext` 读写
- `crates/uc-desktop/src/daemon/workers/clipboard_watcher.rs:303`：worker 用 `clipboard_rs::ClipboardWatcherContext` 监听
- 监听机制是 X11 `XFIXES_SELECTION_NOTIFY`

在 Wayland 会话下：

- 原生 Wayland 应用复制不会触发 X11 selection 事件 → watcher 永远沉默
- XWayland 的剪贴板桥接对原生 Wayland 应用单向：只有 X11 应用复制时事件才会反射回 X11 selection
- 实际表现：用户用 GTK / Qt 等 Wayland 客户端复制 → daemon 完全没动静

### 1.3 替代方案评估

讨论过 4 条路（详见 commit history 之前的 plan 讨论）：

| 方案 | 实时性 | 外部依赖 | 工程量 | 评价 |
|---|---|---|---|---|
| `clipboard-rs 0.3.4 + wayland feature` | 500ms 轮询 | 无 | 极小 | watcher 是手写轮询，仅比较 mime 列表 + text 字节 → 图片/文件场景丢报，多设备同步延迟肉眼可见。**不可接受**。 |
| 子进程 `wl-paste --watch` | 事件驱动 | `wl-clipboard` 二进制 | 小 | 稳定但 snap 打包要 stage `wl-clipboard`；core22 base 的 wl-clipboard 2.1 不支持 ext-data-control，GNOME 47+ 不工作；维护多包源。 |
| `wayland-clipboard-listener` crate | 事件驱动 | 无 | 小 | crate 小众；"iter loops very fast"，事件循环不是真阻塞；不如自己写 |
| **自写 wlr/ext-data-control 客户端** | **事件驱动** | **无** | 中（~600 行 + per-protocol） | **Choose this**：纯库、零进程、零打包侵入、协议级稳定 |

最终选择路线 4：直接绑 Wayland 协议 + `x11rb` 直接绑 X11，完全脱离 `clipboard-rs`（Linux 上）。`clipboard-rs` 仅保留给 macOS / Windows。

---

## 2. 目标 / 期望结果

### 2.1 行为目标

| 目标 | 当前状态 | 目标状态 |
|---|---|---|
| niri / sway / hyprland / KDE Plasma Wayland 下 watcher 监听 | ❌ 全沉默 | ✅ 事件驱动 |
| GNOME 47+ Wayland 下 watcher 监听 | ❌ 全沉默 | ✅ 事件驱动（ext-data-control） |
| Wayland 下 daemon write_snapshot（apply_inbound）结果可被原生 Wayland 应用 paste | ⚠️ 走 X11 + XWayland 桥，行为依赖 compositor | ✅ 走原生 wlr/ext-data-control |
| X11 / XWayland-only / 无 Wayland 路径 | ✅ 走 clipboard_rs（原状） | ✅ 走原生 x11rb |
| 打包：snap / AppImage / Flatpak / deb 体积零增量 | — | ✅ 不引入二进制依赖 |

### 2.2 架构目标

按 `crates/uc-platform/AGENTS.md`：

> `cfg(target_os = ...)` 必须收敛在平台层内部。上层不感知条件编译细节。

最终代码：

- `uc-desktop` daemon worker **不直接 import** 任何 OS 剪贴板库
- `uc-platform` 暴露 `PlatformClipboardEventLoop` trait + `SystemClipboardPort` impls，所有 OS 差异收口在内部
- Linux 内部三套 backend 通过 runtime detect 切换：Wayland(ext) → Wayland(wlr) → X11
- 每套 backend 独立可测试

---

## 3. 验收标准

### 3.1 编译 / 静态

- `cargo check --workspace --all-targets` 全平台 0 warning 0 error
- `cargo clippy -p uc-platform -p uc-desktop --all-targets -- -D warnings` 全平台 0 warning（**注意**：本机 Fedora 44 没装 clippy；CI 上跑）
- `cargo fmt --manifest-path=src-tauri/Cargo.toml --all -- --check` 0 diff
- `cargo test -p uc-platform --lib` 全部通过

### 3.2 运行时（手动 / 半自动）

按桌面环境矩阵：

| 桌面环境 | 协议 | 必跑 | 期望 |
|---|---|---|---|
| niri / Fedora（本机） | wlr-data-control | ✅ | text/html/png/uri-list 全 OK；watcher / write / read 三路径 |
| GNOME 47+ Wayland | ext-data-control | ✅ | 同上 |
| KDE Plasma 6 Wayland | wlr-data-control + ext-data-control | ✅ | 优先 ext，wlr 兜底 |
| sway / hyprland | wlr-data-control | 可选 | 同上 |
| GNOME on Xorg | X11 | ✅ | 与 clipboard_rs 行为等价 |
| XWayland 应用（niri 跑 X11 应用复制） | XWayland 桥 | ✅ | Wayland watcher 应能抓到（compositor 双向桥） |

### 3.3 性能 / 资源

- 启动后 `top -p $(pgrep uniclipboard)`：CPU idle ≈ 0%（事件驱动应当无空转）
- `lsof -p ...`：每个 backend 1 个 wayland/X11 socket fd + 1 个 eventfd，无泄漏
- 长时间跑 1h 不复制：内存平稳

### 3.4 回归

- 大文本复制（>16MB）：Wayland 走 pipe；X11 走 INCR
- 快速连续复制：dedup 与 file 500ms 窗口仍生效
- daemon stop / restart：所有协议连接干净释放，无 zombie thread

---

## 4. 关键发现 / Decisions

### 4.1 协议生态

- **wlr-data-control-unstable-v1**：niri / sway / hyprland / KDE Plasma 5/6 / 一切基于 wlroots 的 compositor。GNOME mutter **不支持**。
- **ext-data-control-v1**：标准化的"clipboard manager" 协议（替代 wlr-data-control）。
  - GNOME mutter 47+（2024-09 发布）支持
  - KDE Plasma 6.x 支持（除 wlr 之外也支持 ext）
  - 较新的 wlroots compositor（含 niri）也支持 ext
- 实战策略：**bind 时优先 ext，回落 wlr**，覆盖最广。

Crate 版本（已锁定）：

```toml
wayland-client = "0.31"          # 0.31.14 在 Cargo.lock
wayland-protocols-wlr = { version = "0.3", features = ["client"] }   # 0.3.12
wayland-protocols = { version = "0.32", features = ["client", "staging"] }  # 0.32.10
```

`wayland-protocols` 的 `staging` feature 暴露 `ext_data_control_v1`。

### 4.2 wayland-client 0.31 API 关键点

- `Connection::connect_to_env()` → `Connection`
- `connection.new_event_queue::<State>()` → `EventQueue<State>`，**`!Send`**
- `event_queue.handle()` → `QueueHandle<State>`
- `event_queue.roundtrip(&mut state)` → 同步 round-trip（用于 bootstrap）
- `event_queue.dispatch_pending(&mut state)` → 处理已缓冲事件
- `event_queue.flush()` → 把 outgoing 写到 socket
- `connection.prepare_read()` → `Option<ReadEventsGuard>`，配合 `poll(2)` 自定义事件循环

`Dispatch<Interface, UserData>` trait 的关键陷阱：

> **`event_created_child!` macro 必须在 Dispatch impl 里声明**，否则收到创建子对象的事件（如 `data_offer(id)`）时 wayland-client 会 panic：
> ```
> Missing event_created_child specialization for event opcode 0 of zwlr_data_control_device_v1
> ```
> 范例：
> ```rust
> impl Dispatch<ZwlrDataControlDeviceV1, ()> for State {
>     fn event(&mut self, ...) { ... }
>     event_created_child!(State, ZwlrDataControlDeviceV1, [
>         EVT_DATA_OFFER_OPCODE => (ZwlrDataControlOfferV1, ()),
>     ]);
> }
> ```

### 4.3 自身 selection echo 死锁

最重要的实战发现，在 Phase 2b 写实现时遇到：

**现象**：write_snapshot OK，wl-paste 能读到，**但 read_snapshot 返回 0 reps**。日志里 `clipboard read timed out for mime 'text/plain;charset=utf-8'`。

**根因**：`wlr/ext-data-control` 协议把每个 `set_selection` 反射回 **所有** data-control 设备 — 包括发起者自己。我们的 worker 在 dispatch 里收到 Selection 事件，调 `build_from_offer` 想读 mime 数据：

1. `offer.receive(mime, write_fd)` 发请求
2. compositor 把请求 forward 成 `Send` 事件给我们的 source
3. **`Send` 事件排在我们当前 dispatch 后面**
4. 我们在 `pipe_receive` 的 poll 循环里阻塞等数据
5. compositor 不会主动写 fd —— 它要等我们的 source 处理 Send 事件
6. 死锁 → 2 秒后 timeout 逐 mime 失败

**解决**：

```rust
// in WorkerState
self_echo_pending: u32,

// in handle_write
state.cached_snapshot = Some(snapshot.clone());   // eager cache
state.self_echo_pending = state.self_echo_pending.saturating_add(1);
device.set_selection(Some(&source));

// in Selection event handler
if state.self_echo_pending > 0 {
    state.self_echo_pending -= 1;
    offer.destroy();
    return;   // 跳过 build_from_offer
}
```

用计数器（不是 bool）的原因：连续两次 write 会产生两个 Selection echo，bool 会丢第二个，再次进入 build_from_offer 死锁。

### 4.4 `WatcherShutdown` 实际是 Send 的

`clipboard_rs::WatcherShutdown` 内部就是一个 `std::sync::mpsc::Sender<()>`。`Sender<()>` 自动是 `Send`（`Send` 当 `T: Send`，`T = ()`）。所以可以直接 `move` 进 spawn 的辅助线程，不需要 `unsafe impl Send`。

旧 worker 注释说 "WatcherShutdown is NOT Send" 是 **错的**（或者是早期版本的注释），新代码已直接 `move`。

### 4.5 rustix 0.38 没有 `io-lifetimes` feature

第一次 build 加了 `features = ["io-lifetimes"]` 失败。rustix 0.38 默认就支持 `OwnedFd` 等 io-lifetimes 类型。最终 Cargo.toml：

```toml
[target.'cfg(unix)'.dependencies]
rustix = { version = "0.38", features = ["event", "fs", "pipe"] }
```

### 4.6 Snap 打包

讨论过子进程方案对 snap 的影响。最终结论：**自写协议绑定方案（路线 4）对 snap 零侵入**，无需 stage `wl-clipboard` / 无需 plug 调整 / 无 base 升级压力。

子进程方案（备选）的话需要：
- `stage-packages: [..., wl-clipboard]`
- 代码里 `$SNAP/usr/bin/wl-paste` 路径解析（snap mount namespace 不见 host /usr）
- core22 上的 wl-clipboard 2.1 不支持 ext-data-control → 必须 override-build 自己编新版

**但我们没走子进程路线**，所以以上都不需要。

### 4.7 当前会话环境

- Fedora 44 ARM64
- niri 25.11 + wayland-1
- WAYLAND_DISPLAY=wayland-1, XDG_SESSION_TYPE=wayland
- niri 同时支持 wlr-data-control 与 ext-data-control 两个协议（从 wayland-info 得知）
- DISPLAY=:0 是 XWayland，不是真 X11 会话
- 系统装了 wl-clipboard 2.2.1（用过 `wl-paste --watch` 验证 wlr-data-control 工作）
- **Fedora 没装 cargo-clippy** —— 本机做 clippy 校验失败；CI 上有
- rustfmt 已装（`/usr/bin/rustfmt 1.9.0`，`/usr/bin/cargo-fmt`）

---

## 5. 已完成工作

### 5.1 Phase 1：抽象重构（已 commit）

**Commit**: `42eb1f8e` (与 Phase 2a 合并提交)

引入 `crates/uc-platform/src/clipboard/event_loop.rs`：

```rust
pub trait PlatformClipboardEventLoop: Send + 'static {
    fn run(self: Box<Self>, handler: ClipboardWatcher, shutdown_rx: ShutdownRx) -> Result<()>;
}

pub fn build_event_loop() -> Result<Box<dyn PlatformClipboardEventLoop>>;
pub fn shutdown_channel() -> (ShutdownTx, ShutdownRx);
```

`ShutdownRx` 在 Unix 用 `eventfd`（rustix）+ `Condvar` 兜底；Windows 仅 Condvar。

`crates/uc-platform/src/clipboard/platform/clipboard_rs_adapter.rs`：把现有 `clipboard_rs::ClipboardWatcherContext` 包成 `PlatformClipboardEventLoop`，用 helper 线程把 `ShutdownRx::wait()` 桥到 `WatcherShutdown::stop()`。Phase 1 在 macOS/Windows/Linux X11 都用这个 adapter。

`crates/uc-platform/src/clipboard/watcher.rs`：

- 解耦 `clipboard_rs::ClipboardHandler`（cfg-gated）
- 提取 `notify_change` / 新增 `notify_with_snapshot`（让 Wayland 走绕过 read 的快路径）

`crates/uc-desktop/src/daemon/workers/clipboard_watcher.rs:303`：worker 改用 `build_event_loop()` 工厂，移除 `use clipboard_rs::*`，移除 `crates/uc-desktop/Cargo.toml` 的 `clipboard-rs` 直接依赖。

### 5.2 Phase 2a：Wayland watcher（已 commit）

**Commit**: `42eb1f8e`

`crates/uc-platform/src/clipboard/platform/linux/wayland/`：

- `mod.rs` — 模块出口
- `state.rs` — Dispatch impls + offer mime 累积 + `event_created_child!`
- `transfer.rs` — pipe + poll + bounded read
- `snapshot.rs` — mime 过滤 + snapshot 构造
- `event_loop.rs` — `PlatformClipboardEventLoop` impl + multi-fd poll

`crates/uc-platform/src/clipboard/platform/linux.rs`：从单文件改成 dispatcher（`linux/legacy.rs` 保留旧 clipboard_rs 实现，重命名 `LegacyLinuxClipboard`）。`build_event_loop()` 加 runtime select：`is_wayland_session() && WaylandEventLoop::try_new()` 成功 → Wayland，否则 fallback。

`crates/uc-platform/Cargo.toml`：
- 加 `[target.'cfg(unix)']` rustix
- 加 `[target.'cfg(target_os = "linux")']` wayland-* 三件套

`crates/uc-platform/examples/wayland_watch.rs`：本机手动验证用 binary。

**niri 实测结果**（commit 描述里有完整日志）：

```
[INFO] wayland: zwlr_data_control_manager_v1 detected, using native backend
[INFO] Linux clipboard event loop: native Wayland (wlr-data-control)
[INFO] wayland event loop: starting

# wl-copy "phase2 verification text"
[snapshot: 1 reps] format=text mime="text/plain" bytes=33

# wl-copy --type text/html "<p>some <b>html</b></p>"
[snapshot: 2 reps] format=html + format=text
```

事件驱动、零延迟、多 MIME 同时捕获。

### 5.3 Phase 2b：WaylandClipboard read/write（已 commit）

**Commit**: `36ac6259`

`crates/uc-platform/src/clipboard/platform/linux/wayland/clipboard.rs` (~570 行)：

- `WaylandClipboard` (impl `SystemClipboardPort`) 是个 facade，内部跑 dedicated worker thread（因为 `EventQueue` 是 `!Send`）
- worker 阻塞在 `poll([wl_fd, wakeup_fd])`；上层用 mpsc + eventfd 唤醒
- 写入：`manager.create_data_source` → `offer(mime)` × N → `device.set_selection(source)`，source 留在 worker 的 `active_source` 里
- `Send(mime, fd)` 事件：查 `active_source.payloads[mime]` → poll-bounded write to fd
- `Cancelled` 事件：清 `active_source`
- 读取：返回 worker 的 `cached_snapshot`，每次 Selection 事件都更新

`crates/uc-platform/src/clipboard/platform/linux.rs` 加 `Wayland` 变体到 enum，`new()` 优先尝试 Wayland。

`crates/uc-platform/examples/wayland_clipboard_test.rs`：write+wl-paste 校验+read 三步本机自动测试。

**niri 实测结果**：

```
[1/3] writing via WaylandClipboard: "phase2b verification 07:11:32.439"
DEBUG: wayland clipboard: skipping self-echo selection ✓
    write OK
[2/3] asking wl-paste to read what we just wrote...
    wl-paste sees expected payload ✓
[3/3] reading back via WaylandClipboard.read_snapshot...
    snapshot: 1 reps
      - format=text mime="text/plain;charset=utf-8" bytes=33 preview="phase2b verification 07:11:32.439"
```

### 5.4 Phase 5：ext-data-control v1 支持（已完成）

**目标**：让 GNOME mutter 47+ / KDE Plasma 6 / 较新 wlroots 用户也能用原生 watcher 与 read/write。

**协议**：`ext_data_control_v1`（路径：`wayland_protocols::ext::data_control::v1::client`，需要 `wayland-protocols` 的 `staging` feature）。请求/事件与 wlr-data-control v1 一一对应。

#### 实施

文件结构（搬迁 + 新增）：

```
src-tauri/crates/uc-platform/src/clipboard/platform/linux/wayland/
├── mod.rs                    ── 模块出口
├── backend.rs                ── 新：OfferLike trait（让 transfer/snapshot 协议无关）
├── transfer.rs               ── 改：泛型 over OfferLike
├── snapshot.rs               ── 改：泛型 over OfferLike
├── write_payload.rs          ── 新：抽出 paster fd 写出逻辑（协议无关）
├── event_loop.rs             ── 改：WaylandEventLoop = enum { Wlr, Ext } 的薄 facade
├── clipboard.rs              ── 改：WaylandClipboard = enum { Wlr, Ext } 的薄 facade
└── protocol/
    ├── mod.rs                ── 新：UC_FORCE_DATA_CONTROL + ext>wlr 优先级 + 探测
    ├── wlr.rs                ── 新：原 state.rs + clipboard.rs 的 wlr 部分（probe / WlrEventLoop / WlrClipboard / 全部 Dispatch）
    └── ext.rs                ── 新：ext-data-control 完整实现（probe / ExtEventLoop / ExtClipboard / 全部 Dispatch）
```

**为何不走"trait + 泛型 dispatch"**：wayland-rs 的 `Dispatch<Iface, _>` 必须在具体 interface 类型上 impl，`event_created_child!` 也需要具体 child 类型，没法跨协议泛型化。最终采取的是 **"helper 协议无关 + dispatch 协议特定"** 的折中：`transfer::pipe_receive<O: OfferLike>` 与 `snapshot::build_from_offer<O: OfferLike>` 共用同一份 body；`write_payload` 完全协议无关；剩下的 worker 主循环 + Dispatch impls 在 `protocol/wlr.rs` 与 `protocol/ext.rs` 各保留一份，结构对照很容易回归两端。

**协议选择**：

- `UC_FORCE_DATA_CONTROL=ext|wlr` 强制本端走指定协议（用于本机 niri 同时支持两套时切换验证）；其它取值会 warn 并退回默认
- 默认顺序：探测到 ext 用 ext，否则 wlr，否则返回 `Ok(None)` 由上层退回 `clipboard_rs` adapter

**自身 selection echo 死锁**：与 wlr 一致的 `self_echo_pending` 计数器逻辑，在 `protocol/ext.rs` 与 `protocol/wlr.rs` 各保留一份；行为（write 后 eager cache，echo 时 decrement 跳过 build_from_offer）相同。

#### niri 实测（同时通告 ext 与 wlr）

`UC_FORCE_DATA_CONTROL=ext`：

```
wayland data-control protocol probe ext=true wlr=true force=Some(Ext)
wayland clipboard: ext-data-control
ext-data-control worker: starting
[1/3] writing via WaylandClipboard: "phase2b verification 08:39:26.824"
ext-data-control worker: skipping self-echo selection ✓
    write OK
[2/3] asking wl-paste to read what we just wrote...
    wl-paste sees expected payload ✓
[3/3] reading back via WaylandClipboard.read_snapshot...
    snapshot: 1 reps
      - format=text mime="text/plain;charset=utf-8" bytes=33 preview="phase2b verification 08:39:26.824"
```

`UC_FORCE_DATA_CONTROL=wlr` 同样的三步通过；watcher（`wayland_watch` 例子）在两种 force 下都能实时抓 text + html 两个 rep。默认（不设 env）选 ext。

#### Phase 5 验收

- niri：watcher / read / write 三路径同时通过 ext 实现工作 ✅
- niri 强制 wlr：行为不退化（regression check）✅
- GNOME 47+ Wayland：未本机验证，待 reviewer / CI 覆盖
- KDE Plasma 6 Wayland：未本机验证，待 reviewer / CI 覆盖

---

## 6. Phase 3 + Phase 4（已落地）

### 6.1 Phase 3：X11 原生 x11rb 重写（已完成）

**已落地：替换 `LegacyLinuxClipboard` 的 clipboard_rs X11 后端为直接使用 `x11rb` 的实现。**

实际文件结构（与原计划一致；没有单独的 `shared.rs`，由 `atoms.rs` 承担常量 + mime mapping）：

```
crates/uc-platform/src/clipboard/platform/linux/x11/
├── mod.rs              ── try_new_event_loop / try_new_clipboard 工厂
├── atoms.rs            ── atom_manager + format_id / mime 映射（与 wayland 对齐）
├── connection.rs       ── X11Server：conn + 隐藏窗口 + xfixes_query_version 探测
├── reader.rs           ── convert_selection + SelectionNotify + INCR 流式接收
├── writer.rs           ── set_selection_owner + 服务 SelectionRequest（含 TARGETS / TIMESTAMP）
├── event_loop.rs       ── X11EventLoop：XFIXES 驱动的 PlatformClipboardEventLoop
└── clipboard.rs        ── X11Clipboard：SystemClipboardPort facade + 工作线程
```

实现要点：

- **连接探测**：`X11Server::connect()` 一次性做 connect + `xfixes_query_version(5,0)` + 创建 1×1 隐藏窗口 + 一次批量 atom intern。失败任一步都立即返回 `Err`，由 `try_new_*` 转成 `Ok(None)` 让 `LinuxClipboard::new` / `build_event_loop` 决策（Phase 3 期间还有 legacy fallback，Phase 4 之后没有）。
- **read 流程**：`convert_selection(TARGETS)` → wait for `SelectionNotify` → 读 property 拿到 atom 列表 → 对每个 atom 反查 `atom_name` → 过滤 `is_interesting_mime` → 对每个目标 mime 再 `convert_selection`。`is_text_mime` / `image/*` 用与 wayland 同样的"first-wins" 去重。
- **INCR 接收**：`read_property_value` 检测 `reply.type_ == atoms.INCR` → 用首个 u32 size hint 预 reserve（cap 在 32 MiB 防 OOM）→ `delete_property` ack → 等 `PropertyNotify(state == NEW_VALUE)` → `get_property(delete=true)` 累积；空 chunk 表示 EOF。每轮 poll(2) 有 2 s deadline。
- **write 流程**：`install_snapshot` 按 rep 构造 atom→bytes payload map；text 类型自动 alias 出 STRING / UTF8_STRING / TEXT / text/plain / text/plain;charset=utf-8 / text/plain;charset=UTF-8 六份（与 clipboard_rs 的 `file_uri_list_to_clipboard_data` 同理），然后 `set_selection_owner` + `get_selection_owner` race check。
- **服务 SelectionRequest**：`service_selection_request` 对 TARGETS 返回当前注册的 atoms + TARGETS + TIMESTAMP；对 TIMESTAMP 返回 `CURRENT_TIME`；对已知 mime 写 `change_property8` payload；未知 mime 回 `property=None`。
- **inline SelectionRequest 处理**：reader 在 `wait_for_event_filtered` 内遇到非目标事件时调用 `route_unrelated_event`；`SelectionRequest` 委托给 `service_selection_request`，让"读自己写的"路径与 wlr 的 `self_echo_pending` 等价（这里更简单：因为单连接 + 缓存 `cached_snapshot` 短路）。
- **worker 线程**：`X11Clipboard` spawn 一个独占线程，poll [conn fd, wakeup eventfd]；读请求优先走 `state.cached_snapshot.clone()` 短路（自己当 owner 时不再 round-trip），否则 `read_snapshot(&server, Some(&state))`。

**Phase 3 验收（实测）**：

- niri 25.11 + XWayland：write → xsel 验证 → 自身 read，三步通过（commit smoke test 在本机执行）：
  ```
  [1/3] writing via X11Clipboard: "phase3 x11 verification 22:50:22.811"
      write OK
  [2/3] asking xclip / xsel to read what we just wrote...
      xsel sees expected payload ✓
  [3/3] reading back via X11Clipboard.read_snapshot...
      snapshot: 1 reps
        - format=text mime=Some("text/plain;charset=utf-8") bytes=36 ...
  ```
- watcher 实时性：外部 `xsel --clipboard --input` 三次连续写，watcher 收到三个独立 snapshot，从 xsel 退出到 watcher emit ≈ 5 ms（poll(2) 事件驱动，无空转）。
- INCR-receive：实现 + 单元测试覆盖 `parse_atom_list`（chunk 边界 + 空输入）。INCR 大文本回归（>16 MiB）只在依赖 INCR 的外部源（Firefox 大段复制）出现，待 reviewer 在 GNOME-on-Xorg 真机覆盖。
- GNOME on Xorg / KDE on Xorg：本机无环境，**待 reviewer 跑 `x11_watch` + `x11_clipboard_test`**。

**未做（明确留作后续）**：

- **INCR-on-write**：当前 writer 一次性 `change_property8` 写完整 payload。x11rb 透明启用 BIG-REQUESTS，所以几 MiB 没问题；但极大 payload（依 server max_request_length，约 ≥16 MiB）需要 INCR-on-write，留待未来真有大 payload 报错时再补。reader 这边已经能消费别人发来的 INCR。

---

### 6.2 Phase 4：清理 clipboard_rs Linux 依赖（已完成）

**已落地：`clipboard_rs` 在 Linux 编译目标上彻底解除引用。**

变更点：

1. `crates/uc-platform/Cargo.toml`：
   - 把 `clipboard-rs` 从无条件依赖移到 `[target.'cfg(any(target_os = "macos", target_os = "windows"))'.dependencies]`。
2. `crates/uc-platform/src/clipboard/mod.rs`：`pub mod common;` 加上同样的 `cfg(any(target_os = "macos", target_os = "windows"))` gate。
3. `crates/uc-platform/src/clipboard/watcher.rs`：把 `ClipboardHandler` 的 import + impl 从 `cfg(any(macos, windows, linux))` 收紧到 `cfg(any(macos, windows))`。
4. `crates/uc-platform/src/clipboard/platform/mod.rs`：`pub mod clipboard_rs_adapter;` 也 cfg-gate 到 mac/win。
5. `crates/uc-platform/src/clipboard/platform/linux.rs`：删除 `Legacy` 变体；`LinuxClipboard::new()` 与 `build_event_loop()` 在两个 native backend 都不可用时直接 `Err`（headless 环境用户自己解决）。
6. `crates/uc-platform/src/clipboard/platform/linux/legacy.rs`：**已删除**。
7. `crates/uc-cli/src/commands/probe.rs` + `Cargo.toml`：probe `watch` 子命令重写成走 `uc_platform::clipboard::build_event_loop`（不再直接 `use clipboard_rs::*`）。同时 `uc-cli` 的 `clipboard-rs` 顶层 dep 一并删除。`watch` 行为等价：仍能看到每次 OS 剪贴板变化、`same_content_streak`、`max_events` 终止；snapshot 由平台层一并交付，省了 probe 自己再 `read_snapshot`。

**Phase 4 验收（实测）**：

- `cargo tree -p uc-platform --target aarch64-unknown-linux-gnu | grep -i clipboard-rs` → 空（exit 0）。
- `cargo tree -p uc-cli      --target aarch64-unknown-linux-gnu | grep -i clipboard-rs` → 空（exit 0）。
- `cargo check --workspace --all-targets` 通过，无新 warning。
- `cargo test -p uc-platform --lib` 20 passed（mac/win-only 的 common.rs 测试自然被 cfg 过滤）。
- `cargo test -p uc-cli` 31 passed、`cargo test -p uc-desktop --lib` 48 passed。
- `uniclip probe watch --max-events 2` + 两次 `wl-copy`：两个 event 全部捕获、`max_events` 退出干净，event loop 线程 join 正常。

CI / 打包：snap / Flatpak / deb 在 Linux target 上不再 vendor `clipboard-rs`，体积应该只减不增。本地无法验，建议 reviewer 在 PR 合并时盯 snap CI 的 stage-package 列表。

---

## 7. 当前 Code Map（速查）

```
crates/uc-platform/
├── Cargo.toml
│   ├── [target.'cfg(unix)'] rustix = 0.38 [event,fs,pipe]
│   └── [target.'cfg(target_os = "linux")']
│       ├── wayland-client = "0.31"
│       ├── wayland-protocols-wlr = "0.3" [client]
│       └── wayland-protocols = "0.32" [client, staging]
├── examples/
│   ├── wayland_watch.rs           ── 验证 wayland watcher
│   ├── wayland_clipboard_test.rs  ── 验证 wayland read+write
│   ├── x11_watch.rs               ── 验证 x11 watcher（强制走 x11rb 路径）
│   └── x11_clipboard_test.rs      ── 验证 x11 read+write（外部走 xclip / xsel 验证）
├── src/clipboard/
│   ├── mod.rs                     ── re-export trait + factory（common 已 cfg-gate macOS/Windows）
│   ├── event_loop.rs              ── PlatformClipboardEventLoop trait + ShutdownTx/Rx
│   ├── watcher.rs                 ── ClipboardWatcher (dedup + notify_change/notify_with_snapshot)
│   ├── common.rs                  ── CommonClipboardImpl (clipboard_rs 包装；macOS/Windows 用；Phase 4 后 cfg-gate)
│   └── platform/
│       ├── mod.rs                 ── cfg 分发 + build_event_loop 工厂
│       ├── clipboard_rs_adapter.rs── ClipboardRsEventLoop（macOS/Windows；Phase 4 后 cfg-gate）
│       ├── linux.rs               ── LinuxClipboard enum dispatcher（Wayland | X11；Phase 4 后无 Legacy 兜底）
│       ├── linux/
│       │   ├── wayland/
│       │   │   ├── mod.rs
│       │   │   ├── backend.rs       ── OfferLike trait（让 transfer/snapshot 协议无关）
│       │   │   ├── transfer.rs      ── 泛型 pipe + poll + bounded read（共用）
│       │   │   ├── snapshot.rs      ── 泛型 mime 过滤 + snapshot 构造（共用）
│       │   │   ├── write_payload.rs ── paster fd 写出（协议无关）
│       │   │   ├── event_loop.rs    ── WaylandEventLoop = enum facade
│       │   │   ├── clipboard.rs     ── WaylandClipboard = enum facade
│       │   │   └── protocol/
│       │   │       ├── mod.rs       ── UC_FORCE_DATA_CONTROL + 探测/选择
│       │   │       ├── wlr.rs       ── wlr-data-control 完整实现（probe/EventLoop/Clipboard/dispatch）
│       │   │       └── ext.rs       ── ext-data-control 完整实现
│       │   └── x11/                 ── Phase 3 新增
│       │       ├── mod.rs           ── try_new_event_loop / try_new_clipboard 工厂
│       │       ├── atoms.rs         ── atom_manager + format_id / mime mapping
│       │       ├── connection.rs    ── X11Server（conn + 隐藏窗口 + XFIXES 探测）
│       │       ├── reader.rs        ── convert_selection + SelectionNotify + INCR 流式接收
│       │       ├── writer.rs        ── set_selection_owner + 服务 SelectionRequest
│       │       ├── event_loop.rs    ── X11EventLoop（XFIXES → notify_with_snapshot）
│       │       └── clipboard.rs     ── X11Clipboard（SystemClipboardPort facade + 工作线程）
│       ├── macos.rs               ── MacOSClipboard (clipboard_rs)
│       └── windows.rs             ── WindowsClipboard (clipboard_rs)
└── ...

crates/uc-desktop/
└── src/daemon/workers/clipboard_watcher.rs  ── worker 通过 build_event_loop() 抽象

crates/uc-cli/
└── src/commands/probe.rs                    ── Phase 4 后改走 build_event_loop（不再直接 use clipboard_rs）
```

---

## 8. 已知坑 / 维护要点

1. **不要在 Selection 事件处理器里 build 自己的 offer**（4.3 已述）。wlr 与 ext 两份实现都已带 `self_echo_pending` 计数器逻辑；任何后续改写两端都要保持。X11 的等价机制是 `WriterState::cached_snapshot` 短路 + `service_selection_request` 在 reader 等待中 inline 服务。
2. **每个 Dispatch impl 都要 `event_created_child!`**（如果协议有创建子对象的事件）。wlr 与 ext 的 device 都已声明对应的 offer 类型；动 dispatch 时不要漏。
3. **`EventQueue` 不是 `Send`**。任何把 `EventQueue` 移过线程边界的代码会编译错误。worker 模式是必需的。
4. **`WaylandClipboard::write_snapshot` 是同步阻塞的**（带 5s 超时）。不要在 hot-path async 上下文里直接调，应当 `tokio::task::spawn_blocking`。daemon `apply_inbound` 路径（`ClipboardWriteCoordinator::write`）是从 facade 同步调入；目前没 spawn_blocking 包裹。**已知潜在问题**：如果 worker 真的卡 5 秒，会阻塞调用方所在的 tokio worker 一段时间。实测下来 wayland / x11 写入都是毫秒级，目前不算紧迫；如果未来出现报告再考虑在 application 层包 spawn_blocking。**X11Clipboard 同样有 5s `REQUEST_TIMEOUT`**，结论一致。
5. **clipboard_rs 0.3.3 的 `WatcherShutdown` 实际是 Send 的**（4.4），不要被旧注释误导。这条只对 macOS/Windows 仍有意义——Phase 4 之后 Linux 已经不走 `clipboard_rs_adapter`。
6. **rustfmt 与 clippy 在 Fedora 44 上的 Rust 1.95.0 包不是默认装的**：`sudo dnf install rustfmt`（用过；本机已装）；clippy 同理但本机 **还没装**。CI 上 clippy 是必须的；本机做 commit 时 pre-commit hook 跑 fmt 但不跑 clippy，所以本机能 commit 通过 → 全靠 CI 兜底。
7. **wayland-rs 的 `Dispatch` 是具体 interface 上的 impl**：试图写 `impl<B: Backend> Dispatch<B::Device, ()>` 这种泛型 impl 会失败（且 `event_created_child!` 也只支持具体子类型）。Phase 5 的 wlr/ext 拆分因此采用 "helper 协议无关 + dispatch 协议特定" 的折中；后续若要再拆协议（primary selection、新 staging）应继承同样模式。
8. **X11 reader 必须 inline 服务 SelectionRequest**：`X11Clipboard` 单连接，worker 在 `wait_for_event_filtered` 阻塞等 SelectionNotify 时如果忽略 SelectionRequest，外部 paster 就会 timeout。当前实现把 SelectionRequest 路由给 `service_selection_request` 防止 dead-lock，**任何 reader 改写都不能丢这一步**。
9. **X11 INCR 接收 buffer 上限 32 MiB**：与 wayland 对齐，并防止恶意 INCR size hint 引发 OOM；超过即 `bail!`。如未来需要更大 payload，要同时调高 `MAX_MIME_BYTES` 与 wayland 端的同名常量，保持两侧对称。
10. **X11 outgoing INCR 未实现**：writer 现在用 `change_property8` 一次性写完，x11rb 透明启 BIG-REQUESTS 所以几 MiB 没事，但接近 X server `max_request_length`（典型 ≥16 MiB）会失败。如果真有这场景，参考 reader 的 INCR 协议反向实现：写 INCR property type + size hint → 等 PropertyNotify(state=Delete) → 写下一块 → 写空 chunk 结束。

---

## 9. 后续 / 仍待覆盖的事项

所有 5 个 Phase 已实现并在本机（Fedora 44 + niri + XWayland）跑通 wayland + x11 smoke 测试。还没本机覆盖的环境留给 reviewer：

1. **GNOME 47+ Wayland**（ext-data-control 路径）：跑 `wayland_watch` + `wayland_clipboard_test`，确认 ext 探测命中、四种 mime（text/html/png/uri-list）来回往返。
2. **KDE Plasma 6 Wayland**（同时通告 ext+wlr）：默认应选 ext；用 `UC_FORCE_DATA_CONTROL=wlr` 跑回归。
3. **GNOME on Xorg**（真 X11 而非 XWayland）：跑 `x11_watch` + `x11_clipboard_test`，特别注意大文本（>16 MiB）从 Firefox 复制走 INCR 路径的回归。
4. **KDE on Xorg**：同上 + klipper 兼容（klipper 会主动 ping clipboard，会触发我们的 watcher / paster 路径）。
5. **Snap / Flatpak Linux build 体积**：CI 实际打包后对比 `clipboard-rs` 移除前后的最终二进制 / stage-packages 大小；本机无法验。

如果之后要加 **primary-selection** 支持（middle-click 粘贴），wayland 端用 `wlr-primary-selection-unstable-v1` 或 `wp-primary-selection-v1`（命名空间分别属于 wlroots 与 staging），X11 端用 `PRIMARY` selection；两边都可以照搬现有 `protocol::wlr` / `linux::x11` 的结构，只是 atom / interface 换一下。

如果要做 **outgoing INCR**（极大 payload write），见 §8.10。

---

## 10. 验证脚本（手动 smoke）

完整的 watcher 验证（应在 niri / sway / KDE / GNOME47+ 各跑一遍；niri 上记得跑两遍 — 一次默认 (ext)、一次 `UC_FORCE_DATA_CONTROL=wlr`）：

```bash
# Terminal A: 启动 watcher
RUST_LOG="info,uc_platform=debug" cargo run \
    --manifest-path=src-tauri/Cargo.toml \
    --example wayland_watch \
    -p uc-platform

# 强制走 wlr 路径回归（在同时通告 ext+wlr 的 compositor 上有用）
UC_FORCE_DATA_CONTROL=wlr RUST_LOG="info,uc_platform=debug" cargo run \
    --manifest-path=src-tauri/Cargo.toml \
    --example wayland_watch \
    -p uc-platform

# Terminal B: 触发各种复制
wl-copy "plain text $(date +%s)"
wl-copy --type text/html "<p>some <b>html</b></p>"
echo "via stdin" | wl-copy
# 复制图片（如果有 PNG 文件）
wl-copy --type image/png < /tmp/sample.png
# 检查 watcher 在 A 终端实时打印 snapshot
```

完整的 read/write 验证：

```bash
# 单次执行（write→wl-paste校验→read 三步内置）
RUST_LOG="info,uc_platform=debug" cargo run \
    --manifest-path=src-tauri/Cargo.toml \
    --example wayland_clipboard_test \
    -p uc-platform
```

预期输出末尾应当是：

```
[1/3] writing via WaylandClipboard: "phase2b verification ..."
    write OK
[2/3] asking wl-paste to read what we just wrote...
    wl-paste sees expected payload ✓
[3/3] reading back via WaylandClipboard.read_snapshot...
    snapshot: 1 reps
      - format=text mime=Some("text/plain;charset=utf-8") bytes=... preview="phase2b verification ..."
```

完整 daemon 端到端验证（可选，需要起两台 peer）：

```bash
# Peer A: 起 daemon
bun run tauri:dev:peerA

# Peer B: 起 daemon + 配对
bun run tauri:dev:peerB

# Peer A 上复制 → Peer B 应当 wl-paste 出同样内容
```

### 10.1 X11 路径手动验证（Phase 3）

两个例子都会 `unsafe { std::env::remove_var("WAYLAND_DISPLAY") }` 强制走 x11rb，所以即便在 Wayland session 上也会命中 XWayland 上的真 X server。在真 X11 session（GNOME on Xorg、KDE on Xorg）上行为一致。

```bash
# Watcher（实时打印外部 xclip / xsel 触发的 snapshot）
RUST_LOG="info,uc_platform=debug" cargo run \
    --manifest-path=src-tauri/Cargo.toml \
    --example x11_watch \
    -p uc-platform

# 另一终端：触发选择变更
xclip -selection clipboard -i <<< "plain text $(date +%s)"
echo -n "via xsel $(date +%s)" | xsel --clipboard --input
# 复制大文本（>16 MiB）以触发外部源的 INCR-write，验证我们的 INCR-receive
head -c 20000000 /dev/urandom | base64 | xclip -selection clipboard -i
```

```bash
# Read/Write 单次跑（write → xclip/xsel 验证 → 自身 read）
RUST_LOG="info,uc_platform=debug" cargo run \
    --manifest-path=src-tauri/Cargo.toml \
    --example x11_clipboard_test \
    -p uc-platform
```

预期输出末尾：

```
[1/3] writing via X11Clipboard: "phase3 x11 verification ..."
    write OK
[2/3] asking xclip / xsel to read what we just wrote...
    xsel sees expected payload ✓        # 或 xclip ✓，看本机装的哪个
[3/3] reading back via X11Clipboard.read_snapshot...
    snapshot: 1 reps
      - format=text mime=Some("text/plain;charset=utf-8") bytes=... preview="phase3 x11 verification ..."
```

### 10.2 验证 clipboard-rs Linux 解除引用（Phase 4）

```bash
# 应当都返回 0 行（grep exit 1）；如果有命中说明某处 dep 漏掉了
cargo tree -p uc-platform --target aarch64-unknown-linux-gnu | grep -i clipboard-rs
cargo tree -p uc-cli      --target aarch64-unknown-linux-gnu | grep -i clipboard-rs
cargo tree -p uc-desktop  --target aarch64-unknown-linux-gnu | grep -i clipboard-rs

# probe 子命令仍能工作（走 build_event_loop，不再直接 use clipboard_rs）
./target/debug/uniclip probe watch --max-events 2 &
echo "phase4 probe ts=$(date +%s)" | wl-copy
echo "phase4 probe 2 ts=$(date +%s)" | wl-copy
# 等待 max-events 触发自然退出
```

---

## 附录 A：Plan 历史 reference

原始 plan（用户 approve 过的）：`~/.claude/plans/indexed-hugging-dragonfly.md`

原始 plan 选择 4 选项问答记录：

- "Linux X11 路径要不要一起重写？" → **全部用原生 wayland-client + x11rb**
- "v1 Wayland watcher 监听哪些 MIME 类型？" → **对齐现有 (文本/HTML/图片/uri-list) 推荐**
- "下一步先做哪个？" → **Phase 5: ext-data-control v1 (GNOME 47+)**

## 附录 B：第三方资料

- wayland-protocols staging/ext-data-control：`https://gitlab.freedesktop.org/wayland/wayland-protocols/-/tree/main/staging/ext-data-control`
- wlr-data-control-unstable-v1：`https://wayland.app/protocols/wlr-data-control-unstable-v1`
- wayland-rs `event_created_child!` macro：`https://smithay.github.io/wayland-rs/wayland_client/macro.event_created_child.html`
- clipboard_rs 上游：`https://github.com/ChurchTao/clipboard-rs`
- 本机 wayland-protocols-wlr 0.3.12 缓存：`~/.cargo/registry/src/index.crates.io-*/wayland-protocols-wlr-0.3.12/`
- clipboard-rs 0.3.3 缓存：`~/.cargo/registry/src/index.crates.io-*/clipboard-rs-0.3.3/`（Phase 3 时阅读 src/platform/x11.rs 的 INCR 处理作参考）

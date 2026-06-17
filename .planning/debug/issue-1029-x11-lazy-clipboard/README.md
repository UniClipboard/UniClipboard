# #1029 — X11 剪贴板 lazy 供数竞态复现 harness

手动复现 + 回归 harness，用于 issue #1029 的 **文本主线**:Linux 桌面 (GNOME/Wayland
无 data-control,daemon 回落 XWayland 的 X11 reader) 下，复制 Chrome 地址栏 URL
**间歇性** 同步失效。

> 这是手动 dev 工具，**不是 CI 测试**:需要一个 X11/XWayland 显示和 `python3-xlib`。
> 纯逻辑 (退避判定) 的可移植单测在
> `crates/uc-platform/src/clipboard/platform/linux/x11/event_loop.rs` 的 `tests` 模块里。

## 竞态根因 (一句话)

X11/ICCCM 下，「同一个 owner 已持有 selection、之后 **追加** target」**不发任何事件**。
Chrome(经 XWayland 桥) 复制后立刻 own CLIPBOARD(发一次 XFIXES),但那一瞬只
advertise 私有 target(如 `chromium/x-source-url`),`text/plain` 晚几十~几百 ms
才补上——**补的时候不重新 own，所以没有第二次 XFIXES**。晚到的 `text/plain` 再也
不会触发读取 → 这次复制永久丢失。

`#1054` 加的「固定 3×150ms 重试」(≈300ms 窗口) 只能救「窗口内补上」的;窗口外补上
的救不了。本目录的 harness 就是把这个时序竞态做成可控、可回归的最小模型。

## 修复 (本 PR)

`event_loop.rs::read_changed_selection` 把固定 3×150ms 重试换成**绑 owner 生命周期的
自适应退避轮询**:记录触发本轮的 owner，只要它仍持有 selection 就以指数退避
(150→300→500ms，封顶) 持续重读，直到：

1. 读到非空 → 成功;
2. owner 变成别的 client → 放弃 (新 owner 会自己发 XFIXES 触发新一轮);
3. owner 变 NONE(清空)→ 放弃 (合法清空);
4. 撞到 3s 硬上限 (`CHANGE_POLL_DEADLINE`)→ 放弃 (owner 对这次 ownership 始终只给
   私有/无法解码的格式)。

诚实边界:3s 仍是上限，但语义从「赌 300ms 内补上」变成「陪等到 owner 放手或 3s」,
鲁棒性高一个量级。供数延迟 > 3s 仍会丢——这是正确行为。

## 文件

| 文件 | 作用 |
| --- | --- |
| `lazy_owner.py` | python-xlib 写的「慢供数」X11 selection owner:own CLIPBOARD，先只 advertise 私有 `chromium/x-source-url`,`DELAY_MS` 后才补 `UTF8_STRING` + `text/plain;charset=utf-8`,**补时不重新 own**。Chrome lazy 供数的最小模型。 |
| `repro.sh` | 编排：起 `uniclip probe watch` → 起 `lazy_owner.py <DELAY>` → 等待 → 收尾 → 判定捕获/丢失。无参跑默认矩阵，或传 `<DELAY_MS>:<WAIT_S>`。 |

## 运行环境

- 一个 X11/XWayland 显示 (`DISPLAY=:0`)。开发验证用的是 **niri(wlroots 系)+
  `xwayland-satellite`** 提供的 `:0`(无需 X auth,`Gdk.Display.open(":0")` 直连)。
  GNOME-on-Wayland 本身也是经 XWayland 桥跑同一条 `x11/reader.rs`,所以同样能复现。
- `python3-xlib`(开发机为 0.33)。
- debug 版 `uniclip`,带 `dev-tools` feature:

  ```bash
  cargo build -p uc-cli --features dev-tools
  ```

  `probe watch` 内部用的就是 daemon 同款 `build_event_loop()` + 本次修改的 watcher,
  所以 **不用跑完整 daemon** 就能验证读取行为。

> `repro.sh` 会强制 `unset WAYLAND_DISPLAY; export DISPLAY=:0`,把 daemon 逼上 X11
> reader(#1029 的真实路径)。若保留 `WAYLAND_DISPLAY`,wlroots 桌面会走原生
> Wayland data-control(`wayland/snapshot.rs`),那不是本竞态要测的路径。

## 用法

```bash
# 默认矩阵:100 1000 2000 2500 3500 ms
.planning/debug/issue-1029-x11-lazy-clipboard/repro.sh

# 指定用例:<DELAY_MS>:<收尾前等待秒数>
.planning/debug/issue-1029-x11-lazy-clipboard/repro.sh 1000:5 3500:6

# repo 路径 / 二进制路径可覆盖
UC_REPO=~/projects/UniClipboard UC_PROBE=/path/to/uniclip repro.sh
```

**判定信号**:`uniclip probe watch` 会为每次捕获的 snapshot 打印自己的 `event #N` 行;
`lazy_owner.py` 供的就是固定 URL `http://example.com/lazy-chrome-url-AABBCC`,所以日志里
**出现该 URL = 捕获成功，没出现 = 丢失**。注意 `probe` **不输出** `uc_platform` 的
tracing 日志，所以 grep `clipboard change lost` / `recovered after retry` 是抓不到的，
要看 URL 标记。

## 实测 before/after(Fedora 44 aarch64,niri + xwayland-satellite,`:0`)

同一台机、同一 harness，只换二进制：

| 供数延迟 DELAY | BEFORE(`origin/main`,3×150ms 固定重试) | AFTER(本 PR,owner 生命周期退避 + 3s 上限) |
| --- | --- | --- |
| 100 ms  | ✅ 捕获 (落在 ~300ms 窗口内) | ✅ 捕获 |
| 1000 ms | ❌ **丢失** | ✅ **捕获** |
| 2000 ms | —(必丢) | ✅ 捕获 |
| 2500 ms | ❌ **丢失** | ✅ 捕获 |
| 3500 ms | —(必丢) | ❌ 丢失 (> 3s 上限，诚实边界) |

结论：修复把可救回窗口从 ~300ms 拉到 ~3s(且全程盯同一 owner，不再赌 XFIXES),
3500ms 仍丢恰好证明上限是真实边界而非无限轮询。

## 已知限制 / 后续

- 供数延迟 > `CHANGE_POLL_DEADLINE`(3s) 仍丢。真实 Chrome 的补数延迟远低于此;若现场
  日志出现 3s+ 的极端 case，再考虑调大上限或做非阻塞重构。
- **非阻塞重构**(把长轮询移出主 event-loop 阻塞路径) 本次 **未做**:阻塞期间 (≤3s)
  新 owner-change 的 XFIXES 在 X 连接缓冲排队，读完即处理;「owner 变更提前退出」也能
  缓解快速连续复制时的阻塞。如需进一步降低快速连复制的延迟，这是下一步。
- 结构性根因 (GNOME 不给 data-control → 被迫走 XWayland) 不在本修复范围;这里是把
  XWayland 这条既有路径加固到鲁棒。

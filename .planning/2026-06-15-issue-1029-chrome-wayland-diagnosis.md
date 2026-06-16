# Issue #1029 复诊分析（2026-06-15）

> 状态：分析完成，待处理。**2026-06-15 复核更正**：Windows 图片写入失败的根因不在 Windows writer、也不是「空/截断数据」，而在 **上游 Linux 捕获侧把图片选成了 `image/xpm`（`image` crate 无 XPM decoder）**。原 A 节假设（`database is locked` → 空 blob）已被日志推翻——见下方「根因更正」。#1029 文本主线仍待更精确复现日志。

## 背景

- Issue #1029：Linux 桌面（Debian 13 + GNOME + Wayland）复制 Chrome 地址栏 URL **间歇性失效**，Firefox 正常。用户无法稳定复现。
- 之前的修复 **#1054**（`c86960f52`，2026-06-12 合并）：修 X11 watcher 的竞态 + 空读重试。
- 用户反馈：升级到 **0.15.0**（已含 #1054）后 **仍然存在**，并补充新症状「从 Linux 复制的内容 GUI 显示已同步，但 Windows 端没有显示内容」。
- 本次拿到两端 0.15 日志：`linux.zip` / `windows.zip`（issue 评论附件，已下载解压到 `/tmp/uc1029/`，临时目录，重启可能丢失）。

## 核心结论（一句话）

> #1054 修的 X11 watcher 竞态/空读重试 **方向正确、在 0.15 里确实生效**；但 #1029 在这台机器上的 **结构性根因是 GNOME/Wayland 不提供 data-control，daemon 被迫走 XWayland 桥**——这一层 #1054 没动。用户最近看到的「同步了却没内容」经日志确认是 **另一个独立的 Windows 图片写入失败 bug**，与 URL 文本无关。

## 证据链

### 1. #1054 在 0.15 里在工作（不是修错了）

Linux daemon 日志 `uniclipboard-daemon.json.2026-06-15`：

```text
06:45:18.093  reader: "x11 read: owner advertised no interesting mimes — empty snapshot"
              targets = chromium/x-source-url, chromium/x-internal-source-rfh-token,
                        chromium/x-web-custom-data, TARGETS, TIMESTAMP   # 没有任何 text/plain！
06:45:18.245  watcher: "selection read recovered after retry"  attempt=2   # 重试救回
06:45:18.259  Windows 端 "inbound clipboard applied" hash=1a748b4a        # 成功送达
```

- 全天 `clipboard change lost`（重试耗尽彻底丢失）= **0 次**。
- Windows 端 26 条 `inbound clipboard applied` 文本全部成功，**无文本丢失实例**。
- 即：这份日志没抓到任何一次 URL 文本彻底丢失。

### 2. 真正的结构性根因：GNOME 无 data-control

```text
wayland data-control protocol probe   ext:false   wlr:false   force:None
Linux clipboard event loop: native X11 (x11rb + XFIXES)   wayland_session:true
```

- 这台 Debian 13 GNOME（Wayland）的 mutter **既不暴露 `ext-data-control-v1` 也不暴露 `wlr-data-control`**（GNOME 安全策略一贯拒绝 data-control）。
- 所以 daemon 回落到 **XWayland 桥** 抓 X11 剪贴板。
- 只要走桥，Chromium 复制后那一瞬 advertise 的 targets 就是抖动的（见上：第一次读只有私有格式，没有 `text/plain`）。#1054 的重试是在赌 `text/plain` 稍后补上——把失败概率压低了，**没从根上消除**。

> 注意：那一刻 URL 其实已在私有 `chromium/x-source-url` 里可读，但 **不能** 直接拿它当文本——它是「页面来源 URL」，在网页里复制一段文字时它指向页面地址而非选中文字，直接用会复制错内容。重试仍是正解，这条捷径走不通。

### 3. 唯一的真实失败 = 图片 `image/xpm` 全链路无法解码（独立 bug，非 #1029；根因更正）

> **根因更正（2026-06-15 复核）**：原假设「`database is locked` → 拿到空/截断 blob → 解码失败」**错误**。日志显示那条 `database is locked` 是 **另一条文本内容**（`content_hash=b65c34ba…`，`plaintext_len=247`），与图片只是同一毫秒巧合。图片字节是完整的（2.9MB，成功 publish 成 blob）。真正根因是 **格式**：这张图是 `image/xpm`，`image` crate 没有 XPM decoder，所以源侧、对端都解不了。

证据——Linux 端（源设备 `879d46fb`）那一刻连发三张图：

| 时间 | mime | 大小 | 结果 |
|---|---|---|---|
| 34.226 | **`image/xpm`** | 2.9MB | Linux 转 PNG 失败→发原始 xpm；Windows 解码失败 ❌ |
| 34.237 | `image/png` | 603KB | 正常 ✓ |
| 34.563 | `image/bmp` | 5.8MB | 正常 ✓ |

失败链（三处都解不了 xpm）：

```text
# 源侧 Linux：归一化转 PNG 失败，退而存原始 xpm 字节发出去
01:34:34.215 WARN background_blob_worker "Failed to convert image to PNG; storing original bytes"
             original_mime=image/xpm  (image::load_from_memory 无 XPM decoder)
# 对端 Windows：clipboard-rs 主路径失败 → native Bitmap fallback
01:34:34.354 WARN windows "write_snapshot: failed to decode image bytes"
01:34:34.355 WARN windows "Primary clipboard-rs image write failed; using Windows native Bitmap fallback"
# native fallback 再次 image::load_from_memory(xpm) 失败
01:34:34.355 ERROR clipboard_write::coordinator "OS clipboard write failed"
             error="Failed to decode image for Windows native write: The image format could not be determined"
01:34:34.355 ERROR apply_inbound::usecase "inbound: OS clipboard background write failed after capture"
```

- 这正是用户说的「GUI 已同步但 Windows 没内容」：Windows **收到了**（→ WS 广播 → GUI 显示同步），但 **xpm 解码失败**，写不进系统剪贴板，所以粘不出来。
- 与 Chrome 地址栏 URL 文本 **无关**。

**为什么会选到 `image/xpm`**：Linux 读取侧（X11 `x11/reader.rs` + Wayland `wayland/snapshot.rs`）按 `text_mime_priority` 排序候选 mime，但该排序 **只重排文本 mime**，图片 mime 一律落 `u32::MAX` 保持源 advertise 顺序——于是「抓到的第一个 image mime」完全取决于源 app 把哪个 image target 排在前面。源把 `image/xpm` 排在 `image/png` 前面，reader 就抓 xpm，并因 `image_captured=true` 丢弃后续的 png/bmp。**缺的是图片格式偏好**。

附带发现（latent）：`uc-infra` 的 `image` crate 只开了 `["png","jpeg","webp","tiff"]` feature（无 bmp/gif），而 `uc-platform` 用默认全 feature。所以源侧 `convert_image_to_png` 对 bmp 也会失败、退而发原始 bmp；bmp 之所以「能用」是因为 Windows 写入侧默认 feature 含 bmp decoder，纯属侥幸。

### 4. 传输层噪音（次要）

- Linux 端 `Lost connection to relay server ... peer closed connection without sending TLS close_notify` 出现 **13 次**。中继反复断，靠 iroh P2P 直连兜住，但不稳。

## 用户已确认的方向

1. 失败内容主要是 **Chrome 地址栏 URL 文本**（#1029 本体仍是文本问题）。
2. **下一步先修已实锤的 Windows 图片写入 bug**（不依赖更多日志）。

## 待办（按优先级）

### A. 修图片 `image/xpm` 同步失败【用户选定，优先；根因已更正到 Linux 读取侧】

**结论已定**：不是「数据损坏/为空」，是 **格式**——Linux 读取侧把图片选成了 `image` crate 解不了的 `image/xpm`。修 Windows writer 用 mime 选 decoder **无效**（根本没有 XPM decoder）。

**根治方向：在 Linux 读取侧给图片 target 加格式偏好**（与现有 `text_mime_priority` 完全同构）：

- 新增 `image_mime_priority(mime)`（放 `crates/uc-platform/src/clipboard/platform/linux/mime.rs`），可解码的规范格式排前：`image/png`(0) < `webp`/`jpeg`/`tiff`（infra 也能转 PNG）< `bmp`/`gif`（仅 platform 默认 feature 可解）< `xpm`/`svg+xml`/未知（垫底）。
- 两个 reader 的排序键改成元组 `(text_mime_priority(mime), image_mime_priority(mime))`：
  - `crates/uc-platform/src/clipboard/platform/linux/x11/reader.rs:137`
  - `crates/uc-platform/src/clipboard/platform/linux/wayland/snapshot.rs:55`
  - 这样源同时 offer png/bmp/xpm 时优先抓 png；首选 fetch 失败仍能优雅回落到下一个 image。
- 加单测：png 必须排在 xpm 之前、bmp/jpeg 在 xpm 之前。
- 残留风险：源 **只** offer xpm（无 png/bmp）时排序救不了——但真实 app（GIMP/GTK）几乎都会同时 offer png/bmp，此情形极罕见，先记为已知限制。

**可选加固（防御性，非必须）**：
- `uc-infra` 的 `image` crate feature 补上 `bmp`/`gif`，让源侧 `convert_image_to_png` 能把 bmp 也归一化成 PNG（当前靠 Windows 侧侥幸解码）。
- `background_blob_worker::convert_image_to_png` 失败时目前 **退而发原始字节**（poison blob）；可改为对「彻底解不了的格式」跳过/标失败而非外发，避免把对端解不了的字节同步出去。

**已澄清不需修**：`database is locked`（`seed receiver context failed`）是 **另一条文本内容** 的 lifecycle projection 写入失败（只 warn、不阻塞主路径），与图片失败无关，不在本 bug 范围。若要单独排查 SQLite 写并发另开条目。

### B. #1029 文本主线【等更精确复现】

- 这份日志没抓到文本丢失实例。需向用户索要 **失败的精确时间戳**（复制失效的那一刻的本地时间），才能在日志里定位 `clipboard change lost` 或 targets 时序异常。
- 根治方向（调研，未定）：GNOME 下绕开 XWayland 的可行路径，例如
  - 缩短/加密重试窗口、reader 层提前重试；
  - 评估 XFixes + 多轮 TARGETS 轮询的更鲁棒读法；
  - 长期：GNOME 是否有 portal 类机制可替代 data-control。

### C. relay 反复断连【次要】

- 13 次 `peer closed connection without sending TLS close_notify`。排查中继握手/keep-alive，确认是否影响同步可靠性。

## 关键代码位置速查

- 后端选型：`crates/uc-platform/src/clipboard/platform/linux.rs`（`is_wayland_session()` L112；event-loop 选型 L133-165）
- X11 重试逻辑（#1054）：`crates/uc-platform/src/clipboard/platform/linux/x11/event_loop.rs`
  - `CHANGE_READ_ATTEMPTS=3`、`CHANGE_READ_RETRY_DELAY=150ms`；`read_with_retry()` 区分 `EmptyNoOwner`（合法清空不重试）/ `EmptyWithOwner`（重试）。
- X11 reader 空读判定：`crates/uc-platform/src/clipboard/platform/linux/x11/reader.rs` L101/L125（"no interesting mimes"）
- Windows 图片写入：`crates/uc-platform/src/clipboard/platform/windows/writer.rs` L170/L175/L208

## 日志原始位置

- `/tmp/uc1029/linux_logs/uniclipboard-daemon.json.2026-06-15`（13328 行）
- `/tmp/uc1029/windows_logs/uniclipboard-daemon.json.2026-06-15`（5143 行）
- 两端 device_id：Linux `879d46fb-...`、Windows `00af67ad-...`
- 注意：`/tmp` 是临时目录，如需保留请尽快归档。

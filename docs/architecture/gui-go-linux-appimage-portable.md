# Go GUI 的 AppImage portable 模式（切片 17c5）

本文是真实 AppImage portable 模式的契约：Wails 与成熟机制审计、规则、失败方式、验收口径、明确不证明的边界。实现之前先写，结果在文末「验证结果」章节补录。AppImage 打包本身见 [gui-go-linux-appimage.md](gui-go-linux-appimage.md)。

## 问题

portable 数据根的既有规则是「可执行文件所在目录下的 `data`」（`crates/uc-app-paths`、`packages/desktop-host-go/apppaths`，Windows 的 zip 与 `portable.dat` 标记同源）。AppImage 内可执行文件位于只读 FUSE 挂载 `/tmp/.mount_*`，所以数据根落在只读文件系统，daemon 起不来（17c4 `probe5`）。17c4 因此只验证了非 portable 的 XDG 数据根，后者需要 Secret Service。

portable 同时承担另一个语义：`is_portable()` 为真时 daemon 选择 **文件 keystore**（`crates/uc-platform/src/secure_storage.rs`），不碰用户的 Secret Service / 钥匙串。所以只把 `$HOME` 挪走不够，Rust 与 Go 必须用同一规则算出同一个可写数据根，并且 `is_portable()` 为真。

## 审计

### Wails（固定版 `v3.0.0-beta.28`，核对模块缓存源码）

| 需求 | Wails 提供 | 结论 |
| --- | --- | --- |
| 数据根 / portable / AppImage 可写目录 | `pkg/application` 中没有 portable 或 `APPIMAGE` 的任何引用（`grep -i "portable\|APPIMAGE"` 只命中一条无关注释） | 无对应 API，**自有适配**（数据根属于业务路径策略，本来就在 `apppaths`） |
| 启动期失败向用户展示 | `app.Dialog.Error()`，需要运行中的事件循环 | 采用：失败时创建最小 app，在 `ApplicationStarted` 事件里弹出错误对话框再退出；同时写 stderr |
| 自启动 | `app.Autostart` | AppImage 内已由 17c4 的最小适配替换（`Exec=$APPIMAGE`）；本片只改它 **写入的目录**，见 F7 |

### 成熟 AppImage 机制（实测，不是读文档）

固定的 `appimagetool 1.9.0` 嵌入 `type2-runtime`（`8f39b89`）。`strings` 在 AppImage 内确认了 `--appimage-portable-home`、`<AppImage>.home` 约定。在无 GTK 的运行镜像里用同一 `appimagetool` 做了最小探针 AppImage（`apps/gui-go/e2e/linux/probe_portable_home.sh`，输出保存在证据目录），事实：

- `<AppImage>.home` 目录存在时，runtime 启动 AppRun 之前打印 `Setting $HOME to …` 并把 `$HOME` 设为它；不存在时不改。
- `$APPIMAGE` 是 **解析符号链接后** 的绝对路径（经 `/t/link` 启动得到真实文件路径）；路径含空格与非 ASCII 字符正常。
- 只改 `$HOME`：`XDG_*` 不变；`.home` 目录 `chmod 555` 时 runtime 照样把 `$HOME` 指向它（root 用户另当别论，见 F8）。
- `--appimage-portable-home` 在目录已存在时报错 `File exists`；`--appimage-extract-and-run` 也设置 `$HOME`，`$APPDIR` 指向可写的解包目录（不是只读挂载）。

采用决定：**portable 的激活标记与可写根使用 runtime 的 `.home` 约定**，而不是另造一个标记，这样用户已知的 AppImage portable 做法（`mkdir X.AppImage.home` 或 `X.AppImage --appimage-portable-home`）直接生效，且 runtime 同时把 WebKit/GTK 的杂项数据（`$HOME` 下）也关进该目录。

## 规则（Rust `uc-app-paths` 与 Go `apppaths` 同一规则）

「在 AppImage 内」= 环境变量 `APPDIR` 非空且当前可执行文件（解析符号链接后）位于解析后的 `APPDIR` 之下。仅有 `APPIMAGE` 而可执行文件不在 `APPDIR` 下（例如从另一个 AppImage 继承了变量的普通进程）**不算**，按既有规则（`<exe 目录>/portable.dat`、`UC_PORTABLE`）解析，行为与现状一致。

在 AppImage 内：

1. 「请求 portable」= `UC_PORTABLE` 为真值，或 `<APPIMAGE>.home` 是目录。
2. `APPIMAGE` 必须是绝对路径且指向普通文件。不满足而用户请求了 portable（`UC_PORTABLE` 为真）→ **无效**；未请求 → 非 portable（`APPIMAGE` 缺失意味着没有经过 runtime，例如直接运行解包的 AppRun，标准 XDG 路径）。
3. 请求了 portable 且 `APPIMAGE` 有效：`<APPIMAGE>.home` 必须已经是目录，否则 **无效**（`UC_PORTABLE=1` 不会静默创建它：目录不是在进程启动前存在，runtime 就没有重定向 `$HOME`，数据会部分落在真实 HOME，得到的是半个 portable）。有效时数据根 = `<APPIMAGE>.home/data`，日志 = `<APPIMAGE>.home/data/logs`。
4. **无效状态没有回退**：Rust 的 `base_data_local_dir`、`base_cache_dir`、`app_log_dir` 在无效状态返回 `None`（调用者映射为数据目录不可用），`is_portable()` 为假但 `portable_error()` 返回原因；Go 的 `AppDataRoot` 等同理。绝不回退到非 portable 的 XDG/共享用户 profile。
5. GUI 启动期（release 形态）在其他一切之前检查 `portable_error`，并预检数据根可写（建立并删除探针文件）；失败时弹出 Wails 错误对话框、写 stderr、退出码 1，说明原因与修复办法。

### `$APPIMAGE` 的可信度

结论：**校验形态，不校验来源**。runtime 从 `/proc/self/exe` 的 realpath 设置它，但任何父进程都能伪造；能伪造环境的人同样能设置 `HOME`、`XDG_DATA_HOME`，所以它用于数据根不引入新的信任边界，并且现有代码（`install_kind_linux.go`、`restart_exe_linux.go`、`autostart_linux.go`、Tauri updater）已经这样使用它。校验项：绝对路径、普通文件、可执行文件位于 `APPDIR` 下（挡住继承来的陈旧变量）、`.home` 存在且可写。无法证明的部分（伪造的 `APPIMAGE` 指向另一个存在的文件）写入「不证明的边界」。

### 与 daemon 的契约

GUI 启动 daemon 时继承环境（`daemonproc/spawn.go`：`os.Environ()` 加 `UC_DAEMON_SPAWN_ORIGIN`），所以 `APPDIR`、`APPIMAGE`、重定向后的 `HOME` 与 `UC_PORTABLE` 原样到达 daemon；daemon 用同一份 `uc-app-paths` 规则算出同一个根。没有新增环境变量，也没有把测试旋钮挪作产品机制。因为 `uc-app-paths` 是 daemon 的输入，必须重新构建 release daemon，并以新身份（构建证据）打包，不沿用 17c4 的 SHA。

### 自启动

`.home` 把 `$HOME` 指向 portable 目录，而桌面会话读取的是真实用户的 `$XDG_CONFIG_HOME/autostart`（缺省 `<真实 HOME>/.config/autostart`）。若沿用 `os.UserHomeDir()`，注册会写进没人读取的 `.home/.config/autostart` 却报告成功。规则：在 portable AppImage 内，自启动目录 = 绝对的 `XDG_CONFIG_HOME`（会话同样读取它）或 **passwd 数据库中的用户主目录** 下的 `.config/autostart`；非 portable 路径不变。既有产品契约里 Linux 的 portable 并没有禁用自启动，所以不新增限制；写入真实用户目录是用户显式开启「开机启动」的效果，不是后台副作用。

## 失败方式（先于实现列出）

| # | 失败方式 | 如何被发现 |
| --- | --- | --- |
| F1 | portable 数据根仍在只读挂载，daemon 起不来 | portable E2E：GUI 与 daemon 的 `/proc/<pid>/environ`、daemon 实际打开的文件（`/proc/<pid>/fd`、`daemon.conn` 所在目录）都在 `<APPIMAGE>.home/data` 下，且 `.daemon-pid`、日志、数据库、加密文件都落在该处 |
| F2 | GUI 与 daemon 算出不同的根（规则漂移，或 `APPIMAGE` 没到 daemon） | GUI 读到 `daemon.conn`（`panelReady`）；`/proc/<daemon pid>/environ` 含同一 `APPIMAGE`；文件树里只有一个数据根 |
| F3 | portable 仍使用 Secret Service / 真实 HOME，用户秘密外泄到共享 profile | portable E2E **不启动任何 Secret Service**（没有 session bus），daemon 仍起来且加密空间可设置、可解锁；真实用户 HOME（passwd 目录）里除自启动条目外没有任何新文件；`master.key` 等文件 keystore 文件在 `data` 下 |
| F4 | 路径含空格、非 ASCII、符号链接启动 | AppImage 放在 `dir with space é/My App.AppImage`，经符号链接启动；`APPIMAGE` 为 realpath，`.home` 在真实文件旁而不是链接旁 |
| F5 | 设置与加密空间语义不持久 | 经 daemon API 写设置、初始化加密空间并写入一条剪贴板内容；退出（旧 daemon 退出）、再启动、读回相同且无需重新初始化；无明文落库（沿用 AGENTS 的持久化默认密文：在 `data` 下搜索写入的明文内容必须找不到） |
| F6 | 更新后数据丢失或旧 daemon 残留 | 17c3 的 fixture 更新替换 AppImage 并重启：数据仍在同一 `.home`，新进程的 `/proc/<pid>/exe` 在新挂载内、`$APPIMAGE` 的 SHA-256 等于 v2、旧 daemon pid 已退出（`/proc` 状态 + HTTP，不用 `kill 0`） |
| F7 | 自启动注册写入无人读取的位置，却报告成功 | 以 **非 root 用户** 运行，passwd 主目录与 `.home` 不同：启用后 `Exec=$APPIMAGE` 的条目必须在 `<passwd 主目录>/.config/autostart`，`.home/.config/autostart` 下没有任何条目；禁用后条目消失；`XDG_CONFIG_HOME` 为绝对路径时用它；UI 的开关状态（daemon `/settings` 与 `Status()`）一致 |
| F8 | `.home` 不可写却假装成功 | 以 **非 root 用户** 运行（root 下 `chmod 555` 不能证明不可写），`.home` 由另一用户拥有且 `0555`：GUI 预检失败、退出码 1、stderr 与对话框说明路径与原因；没有任何文件写入 `.home`；没有回退到 XDG 目录 |
| F9 | 请求 portable 却没有 `.home`（`UC_PORTABLE=1`，没有目录） | 明确失败，信息指出用 `--appimage-portable-home` 创建；磁盘上没有创建 `.home`，没有数据落到别处 |
| F10 | 未经 runtime 运行（解包的 AppRun，`APPIMAGE` 未设置）且请求 portable | 明确失败（无效状态），不落到 `<AppDir>/usr/data`；未请求 portable 时按非 portable 运行（回归） |
| F11 | 陈旧的 `APPIMAGE` 污染非 AppImage 进程 | 解包后的 `usr/bin/uniclipboard` 之外的普通可执行文件不受 `APPIMAGE` 影响：规则只在可执行文件位于 `APPDIR` 下时读取 `APPIMAGE`；用同一二进制在 `APPDIR` 不匹配时，退回既有 `exe 目录` 规则 |
| F12 | 破坏非 portable 与其他模式 | 17c4 的完整回归（full、negative、release smoke）原样复跑；macOS/Windows 的 `apppaths` 行为不变（该文件的 Linux 逻辑在 `paths_linux.go`，其他平台返回「不在 AppImage 内」） |
| F13 | daemon 身份被伪造或沿用 17c4 的二进制 | 重新构建，`build-evidence.txt` 记录新的 HEAD、`uc-app-paths` 输入、Engine 修订、SHA-256；`package_linux.py` 已有的校验链拒绝不匹配；运行时 `/proc/<daemon pid>/exe` 的 SHA 对照 |
| F14 | 容器证据被当成真实桌面证据 | 文档与报告分开陈述（见下） |

## 不证明的边界

- **真实登录会话发现自启动条目**：只证明条目写在 freedesktop 规范规定的位置并由规范格式写出，没有真实桌面环境读取它并在登录时启动（注销登录、更新后的自启动仍未验收）。
- **伪造的 `$APPIMAGE`**：指向另一个存在的普通文件时按其 `.home` 解析；没有来源验证，这是与既有用法相同的信任（见上）。
- **官方签名发布验证、真实发布服务端、原生桌面、Linux amd64**：沿用 17c4 的边界；本片的 AppImage 是 arm64。
- **release 标签 AppImage 的 UI**：release 构建没有控制面，只有冒烟级证据；portable 的功能验证使用 `release,e2e` 构建（与 17c4 一致）。
- **WebKit 在 portable 下的 HTTPS/glib-networking、dlopen 运行时依赖审计**：未变化，仍是 17c4 遗留。

## 验证结果

（实现与 E2E 完成后补录。）

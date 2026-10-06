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

17c5 打包时 `appimagetool 1.9.0`（SHA-256 固定）自己从 GitHub 下载 runtime，runtime 当时没有被固定（17c5 的打包尝试里出现过一次 `Failed to download runtime file`，见证据目录 `iter1/logs/package-v2.failed-network.log`）。17c5 实际测试的 runtime 是 `type2-runtime` revision `8f39b89`；`c27d5a2e…1684` 是 **嵌入后** 的前缀哈希（含 AppImage 自己的 MD5），清零 `.digest_md5` 节后与 `runtime-aarch64` 资产相同，哈希为 `b4ff0030…`。**17c6 已固定** 该 runtime（revision、来源、每架构 SHA-256、`--runtime-file`、嵌入校验），并对固定后的字节重跑了本文件的全部探针与场景，见 [gui-go-linux-appimage-runtime-pin.md](gui-go-linux-appimage-runtime-pin.md)。`.home` 行为与 `$APPIMAGE` 语义是 runtime 提供的，所以 portable 模式的前提是这个固定的输入。

`strings` 在该 AppImage 内确认了 `--appimage-portable-home`、`<AppImage>.home` 约定。在无 GTK 的运行镜像里用同一 `appimagetool` 做了最小探针 AppImage（`apps/gui-go/e2e/linux/probe_portable_home.sh`，输出保存在证据目录），事实：

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

### 测试适配：CLI 软链

`uniclip` 不在 AppImage 内，按它自己的 portable 规则（`UC_PORTABLE=1`，`<exe 目录>/data`）找 daemon。E2E 的做法是只含 `uniclip` 的目录加一个 `data` 软链指向 `<AppImage>.home/data`。**这只是客户端定位的测试适配，不是 GUI 与 daemon 数据根一致的证明**：后者由 GUI 与 daemon 各自的 `/proc/<pid>/environ`（同一 `APPIMAGE`、同一重定向后的 `HOME`）、`daemon.conn` 的位置、以及数据文件实际出现的位置独立证明。第一次尝试里 CLI 找不到 daemon，自己拉起了第二个 daemon 并写入真实用户目录（保留在 iter1），那是夹具问题而非产品泄漏，也因此新增了「没有第二个 daemon」「真实用户 HOME 无未知新增」两项断言。

另外观察到（本片不修复）：CLI 的 `UNICLIPBOARD_DAEMON_BASE_URL`/`TOKEN_PATH` 覆盖路径构造 WebSocket URL 时没有追加 `/ws`（`daemonclient.New` 覆盖分支与常规分支不一致），导致 `WS handshake failed: 404`。

### 自启动

`.home` 把 `$HOME` 指向 portable 目录，而桌面会话读取的是真实用户的 `$XDG_CONFIG_HOME/autostart`（缺省 `<真实 HOME>/.config/autostart`）。若沿用 `os.UserHomeDir()`，注册会写进没人读取的 `.home/.config/autostart` 却报告成功。规则：在 portable AppImage 内，自启动目录 = 绝对的 `XDG_CONFIG_HOME`（会话同样读取它）或 **passwd 数据库中的用户主目录** 下的 `.config/autostart`；非 portable 路径不变。既有产品契约里 Linux 的 portable 并没有禁用自启动，所以不新增限制；写入真实用户目录是用户显式开启「开机启动」的效果，不是后台副作用。

## 失败方式（先于实现列出）

| # | 失败方式 | 如何被发现 |
| --- | --- | --- |
| F1 | portable 数据根仍在只读挂载，daemon 起不来 | portable E2E：GUI 与 daemon 的 `/proc/<pid>/environ`、daemon 实际打开的文件（`/proc/<pid>/fd`、`daemon.conn` 所在目录）都在 `<APPIMAGE>.home/data` 下，且 `.daemon-pid`、日志、数据库、加密文件都落在该处 |
| F2 | GUI 与 daemon 算出不同的根（规则漂移，或 `APPIMAGE` 没到 daemon） | GUI 读到 `daemon.conn`（`panelReady`）；`/proc/<daemon pid>/environ` 含同一 `APPIMAGE`；文件树里只有一个数据根 |
| F3 | portable 仍使用 Secret Service / 真实 HOME，用户秘密外泄到共享 profile | portable E2E **不启动任何 Secret Service**。session bus 地址为空不是 portable 的契约（GLib 会为 GUI 自动拉起一个空总线，daemon 继承其地址），所以证据是三项：总线上 `ListNames` 没有、`ListActivatableNames` 也没有 `org.freedesktop.secrets`（且镜像里没有任何 secret/keyring 的 D-Bus service 文件），没有 keyring 进程；再加功能证明：文件 keystore（`data/…/keyring/*.bin`）存在，加密空间初始化、重启后读回、历史条目经加密索引搜回，真实用户 HOME 里没有 UniClipboard 数据 |
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
| F15 | 启动错误对话框本身崩溃（实测发现） | 干净宿主没有 `/usr/share/mime` 时，包内 gdk-pixbuf 认不出任何图片格式（`Couldn't recognize the image file format`），GTK 在 `gtkiconhelper.c` 断言处 `SIGABRT`：失败对话框在 F8/F9/F10 里让进程崩溃（保留的 iter2 `e2e-portable/fail-*.log`）。根因用 ctypes 直接调用包内 `libgdk_pixbuf` 复现：装上 `shared-mime-info` 即可加载，只放入 `mime.cache`（157 KB）并经 `XDG_DATA_DIRS`（钩子已把 `$APPDIR/usr/share` 放在最前）也可加载。打包脚本因此把构建镜像的 `/usr/share/mime/mime.cache` 放入 AppDir，缺失即失败，哈希写入清单；探针 `probe_pixbuf_mime.sh` 同时跑有/无该文件两种情形（后者必须失败，证明探针能区分） |

### 断言口径的更正

失败场景（F8–F10）里，没有 `.home` 时 GTK 对话框在真实 HOME 写 fontconfig 缓存，这是 toolkit 行为，不是应用数据回退。断言因此不是「真实 HOME 完全不变」，而是：新增文件清单必须 **全部** 匹配明确的白名单（`.cache`、`.cache/fontconfig`、`<32 位十六进制>-le64.cache-<n>`、`CACHEDIR.TAG`），任何未知新增都判失败；完整新增清单保存在 `appimage-assertions.json` 的 `newInRealHome`。同时断言 `.home` 内容、AppImage 旁目录、解包树不变，且没有 `uniclipd` 进程。第一次修订曾用「路径含 uniclipboard 的黑名单」，被指出会放过未知写入，已改为白名单。

## 不证明的边界

- **真实登录会话发现自启动条目**：只证明条目写在 freedesktop 规范规定的位置并由规范格式写出，没有真实桌面环境读取它并在登录时启动（注销登录、更新后的自启动仍未验收）。
- **伪造的 `$APPIMAGE`**：指向另一个存在的普通文件时按其 `.home` 解析；没有来源验证，这是与既有用法相同的信任（见上）。
- **官方签名发布验证、真实发布服务端、原生桌面、Linux amd64**：沿用 17c4 的边界；本片的 AppImage 是 arm64。
- **release 标签 AppImage 的 UI**：release 构建没有控制面，只有冒烟级证据；portable 的功能验证使用 `release,e2e` 构建（与 17c4 一致）。
- **WebKit 在 portable 下的 HTTPS/glib-networking、dlopen 运行时依赖审计**：未变化，仍是 17c4 遗留。

## 验证结果

最终运行：干净提交 `ad2f3ef2d`（`apps/gui-go/e2e/linux/run_17c5.sh`，约 6 分钟），daemon 是为本片重新构建的 release daemon（构建 HEAD `be709456e`，`uc-app-paths` 改动已包含，SHA-256 `ea0f0bcb53e949ba94fc71e97748103e4f7136b4a354924991d686cc4d29f6c6`；17c4 的 `e41904ca…` 只作为基线保留，**不是** 本片的行为证据）。随后在新提交 `9475c3497`（只增加测试场景）对同一个最终 AppImage 跑了补充场景。

| 运行 | 结果 |
| --- | --- |
| `e2e-portable`（非 root 用户、无 Secret Service、真实 AppImage、真实 release daemon） | 64/64 |
| 补充场景 `--supplement`（F11、`XDG_CONFIG_HOME`） | 13/13 |
| 17c4 非 portable 回归：`e2e-full`（含 Secret Service 与更新） | 34/34 |
| 17c4 回归：`e2e-negative`（无重定位对照包必须失败） | 2/2 |
| 17c4 回归：release 标签冒烟（无控制面） | 5/5 |
| `probe-pixbuf-mime`（有/无 `mime.cache` 对照） | 有：PNG 可加载；无：失败（探针能区分） |
| `probe-portable-home`（runtime `.home` 机制探针，原始输出） | 见证据目录 `logs/probe-portable-home.log` |

### 失败方式逐项证据

| # | 状态 | 证据（`appimage-assertions.json` 的检查名） |
| --- | --- | --- |
| F1 | 已验证 | `P1 daemon.conn is under <AppImage>.home/data/app.uniclipboard.desktop`；`P1 portable data root holds the daemon pid file…`；`P1 the file keystore lives in the portable data root`；GUI 与 daemon 实际写入的文件都在该目录 |
| F2 | 已验证 | `P1 GUI and daemon see the same real APPIMAGE path … and HOME = the portable home`（各自 `/proc/<pid>/environ`）；`P1 the panel reports ready`。这是 GUI 与 daemon 一致性的独立证明，**不依赖** CLI 软链 |
| F3 | 已验证（三项 + 功能） | `P1 no Secret Service is reachable`（`ListNames`、`ListActivatableNames`、无 service 文件、无 keyring 进程）；文件 keystore；`P1 the encrypted space was created …`、`P3 the encrypted space is still initialised and unlocked …`；`P1 the real user home (passwd) received nothing`。session bus 地址为空不是契约：GLib 为 GUI 自动拉起一个空总线，daemon 继承其地址 |
| F4 | 已验证 | AppImage 在 `dir with space é/My App.AppImage`，经符号链接启动；`P1 --appimage-portable-home … creates <AppImage>.home next to the real file`；`APPIMAGE` 为解析后的真实路径 |
| F5 | 已验证 | `P1 a user setting …`、`P1 a clipboard entry written through the daemon is found again through the encrypted index`；`P3`：重启后设置、空间状态（稳定字段一致）、条目读回；`P3 no plaintext of the written entry or the passphrase in any file of the portable data root` |
| F6 | 已验证 | `P4`：不受信任签名被拒绝且字节不变；可信更新后文件 SHA-256 等于 v2；新进程在新挂载（marker）；旧 daemon 退出（`/proc` 状态）；新 daemon 在同一 `.home/data`；设置、条目、空间在更新后读回；`.home` 文件未丢失 |
| F7 | 已验证（注册位置）；真实登录会话 **未验证** | `P2 enabling autostart writes <passwd home>/.config/autostart/UniClipboard.desktop`；`P2 the portable home has NO autostart entry`；`P2 the UI state is consistent`（偏好、注册状态、报告路径）；补充 `S2`：绝对 `XDG_CONFIG_HOME` 优先 |
| F8 | 已验证 | `F8 …owned by root and 0555 while the AppImage runs as the unprivileged uid`；消息、对话框窗口、退出码 1、`.home` 仍为空、无 daemon、真实 HOME 无未知新增（白名单） |
| F9 | 已验证 | `F9` 消息指出缺失目录与修复命令；对话框；退出码 1；没有创建 `.home`；无 daemon |
| F10 | 已验证 | `F10` 解包 AppRun 无 `$APPIMAGE` 且 `UC_PORTABLE=1`：消息、对话框、退出码 1；解包树不变 |
| F11 | 已验证（补充运行） | `S1`：可执行文件不在 `APPDIR` 下、`APPIMAGE` 指向一个带 `.home` 的真实 AppImage：旧规则（`<exe 目录>/data`）生效，daemon 在那里，诱饵 AppImage 的 `.home` 保持为空 |
| F12 | 部分 | Linux 非 portable：`e2e-full` 34/34、`e2e-negative` 2/2、release 冒烟 5/5。其他平台：`apppaths` 的 Windows/macOS 路径（旧规则）未改动，`GOOS=windows`、`GOOS=darwin` 的 `go build/vet` 通过，`cargo test -p uc-app-paths`（11 项）通过；**没有在 Windows 或 macOS 上重新运行 E2E** |
| F13 | 已验证 | `build-evidence.txt`（`daemon_source_dirty=false`，head `be709456e`）；清单的哈希链；`P1`/`P4` 的 `the daemon executes from the mount and is byte-identical to the build evidence` |
| F14 | 已声明 | 见下「不证明的边界」 |
| F15 | 已验证 | `probe-pixbuf-mime` 对照；F8/F9/F10 的对话框不再崩溃 |

`.home` 自动激活（无 `UC_PORTABLE`）由 `P1` 证明（首次启动 `environments.gui.UC_PORTABLE` 为空，数据根仍是 `.home/data`）；`UC_PORTABLE=1` 在已有 `.home` 时走同一路径由 `P3` 证明，缺少 `.home` 时失败由 F9 证明。

### 实施中被事实修正的几处（保留的失败运行）

- `iter1`：第一次运行用了错误的 uid（Ubuntu 镜像里 `ubuntu` 占用 1000），随后 root 读不了非 root 进程的 `/proc/<pid>/exe`（缺 `SYS_PTRACE`，仅本任务容器加了该 capability，不改全局 Docker）；CLI 找不到 daemon 后自己拉起了第二个 daemon 并写入真实用户目录（夹具问题）；控制通道没有 `autostart-state` 动词。
- `iter2`：FUSE 挂载只对挂载它的用户可见，root 不能哈希挂载里的 daemon，也不能 `stat` 挂载点；更新后原进程按设计退出，检查要改到全新启动的进程；**`run4` 中错误对话框让 GTK 崩溃（F15）**，保留 `iter2/e2e-portable/fail-*.log`。
- `iter3`：对话框修复后 61/64，剩下三项是「真实 HOME 完全不变」断言过严（GTK 对话框写 fontconfig 缓存）。第一次修订用了名称黑名单，被指出会放过未知写入，改为明确白名单（见「断言口径的更正」）。

## 未完成（open，不能记为完成）

- 真实桌面登录会话读取自启动条目并启动；注销登录；**更新之后** 的自启动有效性。
- `--appimage-extract-and-run`（`APPDIR` 为可写解包目录、`APPIMAGE` 已设置）没有端到端运行；规则对它同样适用（可执行文件在 `APPDIR` 下），但没有证据。
- 伪造的 `APPIMAGE`（`APPDIR` 同时伪造）指向另一个存在的文件时按其 `.home` 解析，无来源验证。
- Windows、macOS 的 `apppaths` 路径没有重新运行 E2E；Tauri 的 `get_install_kind` 在 portable AppImage 里会先判为 `WindowsPortable`（Tauri 外壳待删，不在本片处理）。
- 真实只读挂载（本片用非 root 与他人拥有的 `0555` 目录证明权限失败，未用只读文件系统挂载）。
- 错误对话框的文字内容没有被读取（只证明窗口出现、可关闭、退出码 1、消息在 stderr）；容器里没有窗口管理器。
- CLI 的 `UNICLIPBOARD_DAEMON_BASE_URL` 覆盖路径缺少 `/ws`（既有缺陷，未修）。
- runtime 固定已在 17c6 完成（arm64 的嵌入与运行已验证，amd64 只核对了资产；见 [gui-go-linux-appimage-runtime-pin.md](gui-go-linux-appimage-runtime-pin.md)）。下一步候选：跨发行版 dlopen 审计。
- 其余沿用 17c4：amd64、原生桌面、官方签名发布验证、dlopen 依赖审计、WebView HTTPS。

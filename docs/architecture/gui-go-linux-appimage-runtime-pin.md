# Go GUI Linux AppImage runtime 固定（17c6）

状态：已实现并在容器内验收（arm64）；amd64 只做了资产校验，没有嵌入与运行。

## 问题

AppImage 文件 = type2 runtime（一个静态 ELF）+ SquashFS。runtime 提供 `<AppImage>.home` 与 `$APPIMAGE` 语义，17c5 的 portable 模式依赖它们。`appimagetool 1.9.0` 的 SHA-256 早已固定，但 `package_linux.py` 没有传 `--runtime-file`，所以 appimagetool 在打包时从 GitHub 下载 **当前的** `continuous` runtime：

- 同一个提交在不同时间打出不同 runtime（不可复现）。
- 打包依赖网络：17c5 有一次因下载瞬断而失败（`linux-17c5/iter1/logs/package-v2.failed-network.log`）。
- 17c6 复现了缺陷：容器 `--network none`、不传 `--runtime-file` 时 appimagetool 报 `Failed to download runtime`，退出码 1（`linux-17c6/probe-tool/run-nort-offline.log`，打包流水线里的 `defect-repro-no-runtime-file` 步骤再次保存）。

## Wails 优先审计（固定版本 `v3.0.0-beta.28`，读模块源码）

| 能力 | 版本 / API | 采用情况 | 证据中的缺口 | 最小适配 | 验收 |
| --- | --- | --- | --- | --- | --- |
| AppImage runtime 固定 | `internal/commands/appimage.go`（`wails3 generate appimage`）：下载 `continuous` 的 linuxdeploy 与 AppRun，用 linuxdeploy 打包；整个模块中没有 `runtime-file`、`type2-runtime`、`appimagetool` 的引用 | 已有的 GTK 插件继续采用（17c4）；runtime 固定 Wails 不提供 | Wails 不固定、也不暴露 runtime | 使用 appimagetool 自带的 `--runtime-file` 选项，不自写 runtime，不加配置层 | 见下文失败方式与验收 |
| 自启动 | `app.Autostart` | 不变 | — | 不变 | — |

不重复造轮子：固定逻辑只是已有的 `fetch_verified`（SHA-256 校验）加一个 `--runtime-file` 参数。

## 选择哪个 runtime，以及为什么

| 候选 | 事实 | 结论 |
| --- | --- | --- |
| 带日期的 release `20251108`（commit `dd6cebe`） | 资产名稳定，但它和 `8f39b89` 之间 `runtime.c` 的唯一差异是 `mkdir_p` 的目录权限 `0755` → `0700` | 不选：会静默放弃该修复 |
| `continuous`（commit `8f39b89`，2026-09-28） | 17c5 实际测试的就是这个 runtime；URL 是可变的 | **选用**：来源 URL + 每架构 SHA-256，字节变化时校验失败并停止 |

0700 提交的官方说明（`runtime.c` 在 `8f39b89` 的提交信息）：设置 `APPIMAGE_EXTRACT_AND_RUN` 时 AppImage 解包到 `/tmp` 下的目录，0755 会让系统上所有用户读到解包内容，是不必要的信息泄露。源码证据（`source-evidence/runtime-8f39b89.c`）：`mkdir_p` 只被 `extract_appimage` 调用（`--appimage-extract`、`--appimage-extract-and-run` 的解包目录）。`.home`/`.config` 目录的创建（`--appimage-portable-home`）用的是 `mkdir(…, S_IRWXU)`，FUSE 挂载不经过它。所以对 UniClipboard：portable 模式（`.home`）不受这个差异影响；受影响的是 `--appimage-extract-and-run`（17c5 起一直列为未验证项），0700 让它更安全。没有自写 runtime，也没有在未验证的情况下恢复更宽的权限。

没有官方 immutable 链接：两个 release 的 `immutable` 字段都是 `false`（GitHub API，`source-evidence/release-*.json`）。可复现性的来源是：

1. 资产的 SHA-256（GitHub 资产 digest 与本地独立下载的哈希一致）。
2. 发布的 `.sig` 用 `signing-pubkey.asc` 做 gpg 验证通过（两个架构；密钥取自同一个 release，不是独立信任根，所以这只是一致性证据，不是信任锚）。
3. 来源 revision：`8f39b89e2ac31e1640b3d3f7e9a5108e6ce805fa`。

| 架构 | 资产 | SHA-256 | 字节数 | ELF `e_machine` |
| --- | --- | --- | --- | --- |
| arm64 | `runtime-aarch64` | `b4ff0030242d0c3bb12ce40541828303cf167493f4793456f0436edd6255c39d` | 936456 | 183（AArch64） |
| amd64 | `runtime-x86_64` | `156f4bdbde9c52d01814600013e0a273f0118dc2de98975f3c8c63427ec79074` | 944632 | 62（x86-64） |

pin 的位置：`apps/gui-go/e2e/package_linux.py` 中 `APPIMAGETOOL` 旁边的 `RUNTIME*`。不放进 `scripts/linux-appimage-tools.mjs`：那个文件只服务 Tauri 的 AppImage 打包，Tauri 不能传 `--runtime-file`，放进去只会是没人读取的 pin。

可变 URL 的边界：`continuous` 在下一次上游构建后会换成别的字节。此后：

- 工具目录里已缓存的、SHA 匹配的文件仍然可用，打包不需要网络。
- 缓存缺失或损坏时重新下载会因为 SHA 不匹配而失败，打包停止（不回退到别的来源）。
- 这时要有意识地更新 pin：改 `RUNTIME_REVISION`、两个 SHA 与（如果上游发布了带日期的 release 且包含该修复）URL，并重跑 17c6 流水线。
- 17c6 把当时下载的字节保存在 `linux-17c6/source-evidence/continuous-8f39b89-*`（含 `.sig`），所以即使 URL 失效也能核对当时的输入。

## 嵌入契约（实测）

appimagetool 把 runtime 文件原样写在 SquashFS 之前，只填充 runtime 自己的 `.digest_md5` 节（16 字节；arm64 偏移 923920）。打包后 `verify_embedded_runtime` 检查：

- 镜像前 `len(runtime)` 字节与 runtime 文件逐字节相同，差异只允许落在 `.digest_md5` 节（偏移与大小用 `readelf -S` 从 runtime 文件读取，没有硬编码；amd64 的偏移不同）。
- 紧接着是 SquashFS 魔数 `hsqs`。
- 在同架构宿主上运行 `<AppImage> --appimage-version`，必须报告 `8f39b89`。

manifest 的 `appimage.runtime` 记录 revision、来源 URL、文件 SHA-256 与大小、`e_machine`、SquashFS 偏移、`.digest_md5` 节范围、整段前缀 SHA-256、清零摘要后的前缀 SHA-256（等于 pin）、镜像报告的版本。

对 17c5 记录的更正：`c27d5a2ec0dcca7a99846673627b4fd37a8cb9ae0fa910aaffde696762ff1684` 是 **嵌入之后** 的前缀哈希（含那个 AppImage 自己的 MD5），不是 runtime 资产哈希。把 17c5 的 v1 AppImage 前 936456 字节的 `.digest_md5` 清零后与当前 `continuous` 的 `runtime-aarch64` 相同，哈希正是 `b4ff0030…`；因此 17c5 实际测试的就是现在钉住的字节（`source-evidence` 与本片报告）。偏移 936456 恰好等于 runtime 文件大小，并不是通用常量。

## 失败方式与验收

每个失败方式都用真实的打包命令（`run.sh package-appimage`，同一容器、daemon 证据与 GUI 二进制）制造，坏输入来自输入文件或一份只改一行的 `package_linux.py` 副本（用 bind mount 覆盖容器内的文件，工作树保持干净，打包器没有绕过 pin 的参数）。每个用例断言：退出码非 0、没有 `.AppImage`、日志含原因、被拒绝的字节保留为 `*.rejected-<sha8>`。

| 编号 | 失败方式 | 制造方法 | 结果（`runtime-pin-negative/summary.txt`） |
| --- | --- | --- | --- |
| n1 | 缓存文件不是 ELF | 工具目录放一行文字 | 拒绝：`ELF e_machine None, expected 183` |
| n2 | 缓存文件是别的架构的真 runtime | x86_64 资产放在 aarch64 文件名下 | 拒绝：`ELF e_machine 62, expected 183` |
| n3 | 缓存被截断 | 前 900000 字节 | 拒绝：SHA-256 与 pin 不同 |
| n4 | 缓存有一个字节被翻转 | 偏移 500000 | 拒绝：SHA-256 与 pin 不同 |
| n5 | pin 的 SHA 错误 | 副本里把最后一位改成 `e`，联网下载真 runtime | 拒绝，下载文件保留为 `.rejected-*` |
| n6 | pin 指向别的架构的资产 | 副本里 arm64 指向 `runtime-x86_64` | 拒绝：`ELF e_machine 62, expected 183` |
| n7 | 没有缓存也没有网络 | `--network none` | 拒绝：`cannot download the pinned input`，没有回退 |
| h1 | 缓存损坏但有网络 | 同 n4，联网 | 损坏文件移到 `.rejected-*`，重新下载并校验 SHA，打包成功；修复后的字节等于 pin（自愈不是接受坏字节） |

结果 7/7 拒绝，h1 通过。校验顺序是先 ELF 架构、后 SHA，所以 n1/n2 与 n3/n4 报不同的原因。

打包阶段不做隐式下载：`package-v2-offline` 在 `--network none` 的容器里跑真实的 `package-appimage`（工具目录里已有 SHA 校验过的 appimagetool、linuxdeploy、固定 runtime），成功；日志里 appimagetool 的命令行带 `--runtime-file /cache/tools/appimage-runtime-aarch64`。这个离线 v2 就是随后更新 E2E 使用的 v2。

## 运行证据（固定 runtime 之后整套重跑）

`apps/gui-go/e2e/linux/run_17c6.sh`，从干净提交 `3ed97b642` 运行（约 7 分钟）：

- 4 个包（v1、v2、negative control、release）的 manifest 都带同一个 runtime：清零 `.digest_md5` 后的前缀哈希都等于 pin，整段前缀哈希各不相同（只因各自的 MD5 摘要不同），`--appimage-version` 都报告 `8f39b89`（`runtime-identity` 步骤）。这是“runtime 部分身份相同”的证据，没有要求整个 SquashFS 字节相同。
- 真实 FUSE 探针 `probe-portable-home`（用固定 runtime 构造探针 AppImage）：`.home` 存在时 `$HOME` 被设置，`$APPIMAGE` 为解析后的绝对路径，含空格与非 ASCII 路径、符号链接启动、`--appimage-extract-and-run` 都按预期。
- portable E2E 64/64、`--supplement` 13/13、非 portable 回归 34/34、negative control 2/2、release 无控制面冒烟 5/5、MIME 探针有/无 `mime.cache` 为成功/失败。均使用真实 GUI、真实 release daemon 与挂载的真实 AppImage，没有 stub daemon。
- daemon：输入未变，沿用 17c5 从干净 `be709456e` 构建的 release daemon（SHA-256 `ea0f0bcb53e949ba94fc71e97748103e4f7136b4a354924991d686cc4d29f6c6`，构建证据 `linux-17c5/daemon-17c5/build-evidence.txt`，`package_linux.py` 对照 Cargo/crates/apps/daemon 与构建 HEAD 没有变化）。

## 没有证明的内容

- amd64：只核对了资产 SHA-256、ELF 为 x86-64、`runtime-x86_64` 的 gpg 签名。没有用 x86_64 runtime 嵌入过 AppImage，也没有运行；不能由 arm64 推断。仍需要原生 x86_64 主机或 CI。
- `continuous` URL 可变，没有官方 immutable 资产；可复现性靠 SHA-256 加保存的字节，URL 失效后需要有意更新 pin。
- gpg 密钥不是独立信任根。
- `--appimage-extract-and-run` 在 AppImage 内的 GUI 端到端运行仍未验证（只有探针里的 AppRun 行为）；0700 修复的影响面是上面的源码分析，不是运行时测量。
- 其余沿用：真实登录会话自启动、原生桌面、dlopen 依赖跨发行版审计（`libGLESv2`、WebView HTTPS、glib-networking）、官方签名发布。

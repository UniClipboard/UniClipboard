# 本地自更新测试 (local update loop)

这个工具让你在自己的电脑上完整跑一遍 **"检测到新版本 → 下载 → 安装 → 重启"** 的自更新流程，**不需要真的发布两个版本** 到线上更新渠道。

## 为什么需要它

桌面端的自更新链路（`src-tauri/crates/uc-tauri/src/commands/updater.rs`）只有在 updater 真正被触发时才会执行：下载新构件、校验签名、替换 app、重启。像 #1063 那个"更新重启后 daemon 实例锁竞态导致卡死"的修复，**只有走一遍真实的更新重启** 才能验证。

以前要验证它，必须先往真实更新渠道发两个版本（旧 + 新）。这个工具把成本降到：**本地构建两次 + 一个 localhost 静态服务**。

## 它怎么工作

两个只在 **debug 构建** 里存在的环境变量（release 构建会被 `#[cfg(debug_assertions)]` 整段编译掉，线上二进制没有这条路径，无法被劫持）：

- `UC_UPDATE_ENDPOINT`：把 updater 指向一个 localhost 的清单（manifest）地址，而不是线上发布服务器。
- `UC_UPDATE_PUBKEY`：用一把一次性的开发签名公钥来校验，而不是内置的线上公钥。

注意：**签名校验始终是开启的**，只是换成了一把开发用的临时密钥。所以这条路径和线上完全一致——真实地下载、校验、安装、重启。

对应的后端实现见 `do_check_for_update` 与 `apply_dev_updater_override`（`commands/updater.rs`）。

## 前置条件

- macOS（当前只支持 macOS；Windows/Linux 的构件格式与安装路径不同）。
- 仓库能正常构建桌面端：`bun install` 已跑过，Rust 工具链可用。
- 脚本会自动把 `uniclipd` daemon 作为 sidecar 暂存（已存在则跳过，`--rebuild-sidecar` 可强制重建）。

## 一次性准备：生成开发密钥

```bash
node scripts/dev-update-loop.mjs keygen
```

会在 `~/.uniclip-dev-updater/` 下生成一把一次性 minisign 密钥（私钥默认无密码）。这把钥匙只用于本地测试，**不要** 用线上密钥，也不会被提交进仓库。

## 完整流程

下面四步按顺序执行。`serve` 和 `run` 是常驻进程，请各开一个终端窗口。

```bash
# 1. 构建"旧"版 app（当前提交里的版本号），暂存到 target/dev-update/run/
node scripts/dev-update-loop.mjs build-base

# 2. 构建"新"版更新构件 + 清单（版本号自动 +1），暂存到 target/dev-update/serve/
node scripts/dev-update-loop.mjs build-update
```

> `build-update` 阶段 tauri 会打印一条警告：签名私钥与 `tauri.conf.json` 里内置的（线上）公钥不匹配。**这是预期的**——我们正是用开发密钥签名、再在运行时用 `UC_UPDATE_PUBKEY` 换成开发公钥来校验。忽略它即可。

```bash
# 3. 终端 A：把清单和构件挂到 http://localhost:8723/
node scripts/dev-update-loop.mjs serve

# 4. 终端 B：带 override 启动"旧"版 app
node scripts/dev-update-loop.mjs run
```

app 启动后，在界面里触发一次"检查更新"（设置页 / 关于页 / 托盘菜单均可）。然后观察它：发现新版本 → 下载 → 校验 → 安装 → 重启。重启后新 daemon 会接管，旧 daemon 被驱逐——这正是 #1063 要验证的行为。

`run` 用的数据目录通过 `UC_PROFILE=updtest` 隔离（数据落在 `app.uniclipboard.desktop-updtest`），不会动到你日常使用的安装实例。`app.restart()` 会带着同样的环境变量重新拉起进程，所以 override 在重启后依然有效。

## 验证 #1063（更新重启不卡死）

在 `run` 终端里关注这些日志信号：

- `pre-update: daemon stop complete` —— 安装前旧 daemon 已停止。
- `update installed, restarting app` —— 安装完成，准备重启。

`app.restart()` 之后，新进程会脱离当前终端，**终端不再有日志输出**。重启后的行为改看日志文件：

```text
~/Library/Application Support/app.uniclipboard.desktop-updtest/logs/
```

在那里确认：新 daemon 正常起来（出现实例锁驱逐相关日志），主窗口能正常进入，而不是卡在 loading。如果主窗口卡死、或必须手动 kill `uniclipd` 才能恢复，说明竞态仍在。

## 重复测试

不必每次都重新构建"旧"版。已有"旧"版后，只要再跑一次 `build-update`，版本号会在上次基础上继续 +1（状态记在 `target/dev-update/state.json`），然后重启 `serve` 即可再触发一轮。也可以用 `--update-version` 显式指定：

```bash
node scripts/dev-update-loop.mjs build-update --update-version 0.15.0-alpha.9
```

想从头开始（把"旧"版重置回当前提交版本），重新跑 `build-base`。

## 常用选项与排错

- `node scripts/dev-update-loop.mjs info` —— 打印解析出的路径、版本、override 环境变量。
- `--port <n>` —— 自定义端口；`build-update`（写进清单 URL）、`serve`、`run` 三者必须一致。
- 端口提醒：移动同步固定端口 `42720` 不随 profile 变化，测试时别同时开着日常实例的 daemon，否则会冲突。
- 构建产物在 `target/debug/bundle/macos/`；构件清理只需删 `target/dev-update/`（在 `target/` 下，已被 git 忽略）。
- 自定义路径/口令的环境变量：`UC_DEV_UPDATER_KEY_DIR`、`UC_DEV_UPDATER_KEY_PASSWORD`、`UC_DEV_UPDATER_PROFILE`、`UC_DEV_UPDATER_PORT`。
- `install from cached bytes failed error=Cross-device link (os error 18)`：macOS 更新器把新 app 解压到 `$TMPDIR` 后用 `rename` 覆盖旧 app，跨卷 `rename` 会报 `EXDEV`。当仓库（以及 run 的 app）在非启动卷（如外置 SSD）、而 `$TMPDIR` 默认在启动卷时就会触发。`run` 已自动把 `$TMPDIR` 指到 `target/dev-update/tmp`（与 run app 同卷）规避；正式环境里 app 在 `/Applications`、与 `$TMPDIR` 同在启动卷，不会有这个问题。
  - 顺带提醒：这也意味着 **真实用户若把 app 装在非启动卷上，自更新同样会失败**（tauri-plugin-updater 用 `rename` 而非带 copy 兜底的移动）。属上游已知限制，与本工具无关。

## 与线上发布的关系

本地清单的格式与线上完全一致，对齐 `scripts/assemble-update-manifest.js` 生成的结构（`version` / `notes` / `pub_date` / `platforms`）。`signature` 字段是 `.sig` 文件的原始内容（本身已是 base64）。这样本地验证的就是线上同一套格式，避免"本地能过、线上挂"。

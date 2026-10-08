# Go 宿主 Windows 安装包、便携包与验收合同

适用于 issue #1897。macOS 对应文档见 `apps/gui-go/README.md`「macOS 发布构建」；更新签名见 `docs/architecture/gui-go-updater-signatures.md`。

## 产物与流水线

`.github/workflows/build.yml` 中三个作业串联（`workflow_dispatch` 选择 `windows-latest`、`windows-x86_64` 或 `windows-arm64`）：

| 作业 | 运行位置 | 内容 |
| --- | --- | --- |
| `build-sidecar` | 现有矩阵 | 构建真实 Rust `uniclipd.exe`，并由 `scripts/ci/write-sidecar-provenance.mjs` 写入 `sidecar-provenance.json`（源码提交、SHA-256、rustc、`Cargo.lock` 哈希、run 链接） |
| `package-windows-gui` | 托管 `windows-latest`（两个架构都在 x64 上交叉编译 Go） | `e2e/package_windows.py` 生成 setup 与 portable；`e2e/windows_package_verify.py` 解包成品复核 |
| `smoke-windows-gui` | 托管一次性 runner：x64 用 `windows-latest`，arm64 用原生 `windows-11-arm` | `e2e/windows_package_acceptance.py` 真实安装、运行、更新、卸载 |

上传内容：只有 `UniClipboard_<版本>_<x64|arm64>-setup.exe` 与 `..._portable.zip`（artifact `windows-gui-<triple>`）。清单、哈希、验收输入（更高版本的 `ACCEPTANCE-` 安装包、`uniclip.exe`）和证据分别放在另外三个 artifact 中，发布流程的资产收集不会碰到它们。

## 失败模型

1. 打包进去的不是 CI 构建的 daemon（夹具、旧文件、其他提交）。
2. 安装包或便携包内的 daemon 与打包输入不是同一个文件。
3. 在真实用户机器上跑发布形态，污染真实数据根、凭据管理器与会话。
4. 更新时强制结束 daemon（`TerminateProcess`）后数据库不可用。
5. 降级覆盖、更新后出现两个实例、卸载残留登录项或快捷方式。
6. 便携包把数据写到目录之外，或在只读目录静默改写到系统目录。
7. 未签名安装包被当成可发布产物。

## 状态不变量

- 安装包与便携包中的 `uniclipd.exe` 的 SHA-256 等于 `sidecar-provenance.json` 中该 target 的记录，且记录的源码提交等于打包提交、树干净、target 匹配（`package_windows.py` 不满足即退出）。
- `windows_package_verify.py` 用 7-Zip 解开 setup、用 zipfile 解开 portable，逐一复核同一哈希；安装后 `windows_package_acceptance.py` 再复核安装目录里落盘的文件。
- `productionUsable` 恒为 false，直到有 Authenticode 决定；`workflow_call`（`require_signing == true`）调用时 `package-windows-gui` 直接失败。`release.yml` 的首个步骤不变，仍然失败关闭。
- 发布形态验收只在一次性托管 runner 上运行（脚本检查 `GITHUB_ACTIONS` 与 `RUNNER_ENVIRONMENT`）。

## 验收合同

`windows_package_acceptance.py` 的场景（结果写入 `acceptance.json`，每项为 PASS/FAIL 加细节）：

| 场景 | 覆盖 |
| --- | --- |
| A | 静默安装；卸载键、快捷方式、daemon 哈希；首次启动，daemon 来自安装目录且 `/health` 正常；space init 与剪贴板捕获；写入自启偏好后重启，Run 值指向安装的 exe 并带 `--autostart`；强制结束 GUI 与 daemon 后重启，历史可读 |
| B | `/P /R /UPDATE` 更新（更高版本，剪贴板写入进行中）：版本、GUI 替换、重启、单实例、旧进程消失、历史可读、Run 值保留 |
| C | 降级拒绝（`/S` 与 `/P`）：非零退出且不改动 |
| D | 保留数据卸载：文件、卸载键、快捷方式、Run 值移除，数据保留 |
| E | 在保留的数据上重装：旧历史仍可读 |
| F | `/DELETEAPPDATA` 卸载：两个数据根被删除 |
| G | 可写目录便携运行：数据在目录内；强制结束后重启历史可读；目录外无以应用命名的新文件、注册表项、凭据 |
| H | 只读目录便携运行：记录行为（不断言） |

安装器新增 `/DELETEAPPDATA` 命令行开关，等同卸载页的复选框，使静默卸载也能删除数据。

### 明确未覆盖

- 真实注销再登录：只检查 Run 值及其命令行；真实登录启动需要在专用测试主机的真实会话中人工完成。
- 交互式安装向导（自动化的是 `/P` 与 `/S`）。
- 真实 Tauri 写入的旧 Run 值：需要安装公开 Tauri 版本并手动开启自启；`windows_production_run.py` 只用种子值验证清理逻辑。
- 托管 runner 是 Windows Server，不是 Windows 10/11 客户端版本。
- 签名后的 `signtool verify` 与签名版本的更新流程。

前端遥测的平台差异（有意为之）：Windows 作业在 `build_mode=test` 时清空 `VITE_SENTRY_DSN`，而 macOS 作业不清空。原因是 Windows 验收在一次性 runner 上真实启动 GUI，不应让前端向生产 Sentry 上报；两者的差异不是遗漏，改动任一侧时需同步评估另一侧。

遥测：验收在首次启动前写入关闭遥测的偏好文件，避免 CI 向生产 Sentry/PostHog 上报。Sentry 调试符号上传沿用 `build-sidecar` 现有逻辑（`build_mode=test` 不上传）；前端 source map 上传与 macOS 相同，仅在非 test 且有密钥时执行。

## Authenticode 决定（待用户）

仓库与组织密钥中没有任何 Windows 签名证书、签名服务配置或 `signtool` 步骤，Tauri 时期也从未签名。需要产品决定：

1. **Azure Artifact Signing（原 Trusted Signing）**：云端托管、无需自管证书文件，CI 通过 OIDC 调用；需要 Azure 订阅与身份验证（组织验证周期）。成本低，适合 GitHub Actions，是 Microsoft 当前推荐的方案；新证书的 SmartScreen 信誉仍需积累。
2. **云 HSM 托管的 OV/EV 证书**（如 DigiCert KeyLocker、SSL.com eSigner）：EV 可更快建立 SmartScreen 信誉，费用最高，需要确定证书持有人。
3. **SignPath 等第三方签名服务**：对开源项目有免费方案，审批与流程由对方管理。
4. **暂不签名发布**：必须在发布说明中写明 SmartScreen 警告、杀毒软件误报风险，且更新通道无法依赖 Authenticode；本任务不会默认允许。

任一签名方案落地时需要对 setup、`UniClipboard.exe`、`uniclipd.exe` 都签名，在打包前签内部可执行文件，之后签安装包，再重算哈希与 `sidecar` 校验，并用 `signtool verify /pa` 验证、重跑本验收。

## 验收状态（已执行）

运行 37728413411（`build.yml`，`workflow_dispatch`，`build_mode=test`，源码提交 `ea1b0137b43264c7a417f51d76de5ed31e9f1cae`，Engine 固定为已合并的 `0e25f4189301efd68c21c8ffdd51a2f9fbfd4204`）：

- 两个架构的 `package-windows-gui` 与 `smoke-windows-gui` 全部成功，`acceptance.json` 中 amd64（Windows Server 2025，原生 x64）与 arm64（Windows 11 Enterprise，原生 ARM64，`Win32_Processor.Architecture=12`）各场景 A–G 的断言全部通过；H 为记录项。
- 只读目录的便携包：GUI 进程运行，但 daemon 不启动，也没有在目录之外写入以应用命名的数据（对应已有问题 #1259：daemon 启动失败时 GUI 不报错）。
- 该运行使用 `test` 构建方式（优化级别较低），不是发布构建；未签名；托管 runner 不是 Windows 10/11 客户端版本（arm64 例外，为 Windows 11）。
- 发现并修复的两个阻塞问题：`apps/gui-go/environment_windows.go` 与 `environment_portable.go` 重复定义 `validateIsolation`（#1875 之后 Windows 版 GUI 无法编译）；固定的 Engine `e86f94ce` 在 Windows 上无法编译（`uc-infra-storage` 使用未声明的 `windows-sys`，由 Engine PR #163 修复）。

## 无生产副作用的优化构建（release 构建方式）

`build.yml` 新增 `upload_symbols` 输入（默认 true，保持原行为）。设为 false 时，`build_mode=release` 仍使用完整优化构建，但不上传 Sentry 调试符号，也不上传前端 source map。用于在不产生生产副作用的前提下验收发布形态的构建；它不代表允许打 tag、发布或写更新源，`release.yml` 的失败关闭守卫没有改动。发布形态的 daemon 编译进了生产遥测密钥，因此验收在首次启动前写入关闭遥测的偏好。

## 本 PR 的 CI 基线

- `cargo audit`：RUSTSEC-2026-0330 与 RUSTSEC-2026-0331（`libcrux-kem 0.0.9`）。`main` 的 `Cargo.lock` 中版本相同，已有跟踪问题 #1918、#1919；本 PR 的 `Cargo.lock` 相对 `main` 只改动 Engine 固定版本与一条 `windows-sys` 依赖边，没有改动 `libcrux-kem`。升级加密库不在本任务范围，未增加忽略项。
- `bun audit (docs-site)`：9 项（Next.js、sharp、source-map-js、KaTeX），`main` 上同样失败；本 PR 未触及 `docs-site` 与任何 lockfile 的 JS 部分。未增加忽略项。

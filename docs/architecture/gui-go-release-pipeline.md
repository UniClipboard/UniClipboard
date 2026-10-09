# Go 宿主发布流水线：对外写入前的门禁与离线验收

本文记录 `release.yml` 在 Go/Wails 宿主上的整合：标准流程、门禁、失败模型、离线端到端验收，以及仍然阻塞的项目。它接续 [gui-go-tauri-retirement.md](gui-go-tauri-retirement.md)、[gui-go-updater-signatures.md](gui-go-updater-signatures.md)、[gui-go-windows-packaging.md](gui-go-windows-packaging.md) 与 [gui-go-distribution-channels.md](gui-go-distribution-channels.md)。

## 标准流程（未改动）

1. 维护者手动触发 `prepare-release.yml`（版本由维护者选择；该工作流 bump 版本、重新生成 Go `buildinfo`、刷新 `Cargo.lock`、生成更新日志，并创建 release PR）。
2. 合并 release PR。
3. `tag-on-merge.yml` 自动创建并推送注解 tag `v<version>`。
4. tag push 触发 `release.yml`。

本任务不选择、不设置、不递增版本，不改 `prepare-release.yml`、`tag-on-merge.yml`、`bump-version.js`。tag 是 `release.yml` 的输入，不属于“对外写入”；所有检查发生在 tag 触发之后的构建、签名与资产阶段，并且早于任何 tag（仅 `workflow_dispatch` 路径会在此创建 tag）、GitHub Release、R2、FlareRelease 与渠道写入。

没有生产证书时，tag 可以按现有流程存在，但 `release.yml` 的第一个门禁会失败，正式包不会发布。

## 变更

| 位置 | 变更 |
| --- | --- |
| `release.yml` `validate` | 原无条件 `exit 1` 改为 `scripts/ci/release_gate.py prerequisites`：缺少生产 Windows 签名后端（`WINDOWS_SIGN_BACKEND` 须为 `azure` 或 `pfx`）或更新签名私钥时失败。测试签名（self-test、SignPath test-signing）永远不满足。随后 `release_gate.py source` 固定 `source_sha`，并要求 tag 版本与 `package.json`、`apps/gui-go/app.json`、`Cargo.toml`、`buildinfo.go`、`Cargo.lock` 工作区成员一致，且 `Cargo.toml` 与 `Cargo.lock` 的 Engine 版本固定一致。 |
| `release.yml` `build` / `build-cli` / `create-release` | 一律检出并构建 `needs.validate.outputs.source_sha`（完整提交 SHA），而不是移动的分支或 tag 名。 |
| `release.yml` `create-release` | 收集、签名、清单、登记改为一次调用 `scripts/ci/assemble_release_assets.py`（见下），其后才是原有的 SHA256SUMS、tag（仅 dispatch）、Release、R2、FlareRelease。 |
| `release.yml` 渠道分发 | Snap：alpha 仍异步触发，但 `release=false`，只产出 artifact；COPR：alpha 仍触发，但 `dry_run=true`，只产出 SRPM。推送商店与提交 COPR 需要人显式 `workflow_dispatch`。npm 与 GitCode 镜像入口不变（见下）。 |
| `apps/gui-go/e2e/package_linux.py` | deb 的 `Control: Version` 把预发布分隔符换成 `~`（与 rpm 既有约定一致）；`Conflicts`/`Replaces`/`Provides` 同样使用该写法。文件名与更新语义版本不变。 |

### 共享的发布阶段

`scripts/ci/assemble_release_assets.py` 依次执行：收集具名发行物（`collect-release-assets.py`）→ `release_gate.py assets` → 更新签名（`updater-sign`，使用 `app.json` 里的生产公钥）→ 不带任何密钥的签名复验 → `assemble-update-manifest.js --require-all-platforms` → `build-flare-release-registration.js` → 写出 SHA-256 索引（`assembly-index.json`）。它不访问网络服务，也不写任何外部系统。

`--mode release` 只允许生产路径；`--mode fixture` 只多出“指定一次性签名器与公钥”的能力，阶段、顺序与检查相同。release 模式下传入这些参数会被拒绝。

### 门禁 `release_gate.py assets` 检查什么

- 必需文件集合（版本 `V`）：两个 macOS 更新归档、两个 dmg、两个 deb、两个 rpm、两个 AppImage 更新归档、两个 setup 与两个 portable，即六个平台；至少一个 CLI 归档。缺失、带有其他版本、出现未知文件都失败。
- 下载的 workflow artifact 中出现测试模式或测试签名构建（名称含 `-test`、`-signing-selftest`、`-signpath-test`）即失败，无论它是否贡献了文件。
- 每个发布文件的字节必须等于某个 workflow artifact 内的文件，索引里记录来源 artifact 名。
- 每个平台的打包证据（`package-manifest.json`、`provenance.json`）必须来自固定的 `source_sha`、非脏检出、版本等于 `V`；Windows 证据还必须是 `signing.provider == "signed"`（生产后端）。升级验收用的 `newer` 包刻意使用另一版本，被跳过。

## 失败模型与验证

离线验收：`python3 -I apps/gui-go/e2e/release_assembly_run.py --out <新的空目录>`。它在合成的 `download-artifact` 目录树上运行上述同一批脚本，再用真实 Go 消费者经本地 HTTP 下载并验证六个平台。进程环境移除所有令牌，代理指向无效端口。`scope.json` 明确声明：包是合成的、更新密钥是一次性的、生产 Windows 签名未运行。

| 失败方式 | 用例 |
| --- | --- |
| 缺平台 | 缺 Linux arm64 AppImage 归档；缺 Windows arm64 安装包 |
| 错版本 | 文件名为 9.9.9 的 deb；CLI 归档；平台证据版本不符 |
| 错源 | 证据来自另一提交；证据来自脏检出；源记录对应另一 SHA；预发布证据来自另一提交 |
| 重复 | 同名 deb 出现在第二个 artifact |
| 中间工件 | SignPath 阶段 2 输入与证据内的 CLI 副本不被收集（字节比对） |
| 测试签名 / 未签名 | `-signpath-test`、`-signing-selftest`、`-test` artifact；Windows 证据 provider 为 `signpath-test`、`selftest`、`unsigned`、`unspecified` |
| 版本载体漂移 | 陈旧的 `buildinfo.go`；`Cargo.lock` 的 Engine 版本漂移；单个载体版本不同；错误的版本或 SHA |
| 签名后篡改 | 篡改字节后复验与真实消费者均拒绝；互换两个 `.sig` 被拒绝 |
| 绕过生产公钥 | release 模式用一次性私钥签名，被 `app.json` 的生产公钥拒绝，且没有任何 `.sig` 写出 |
| 缺少生产前提 | 无前提、后端为 `signpath-test`/`selftest`、缺更新私钥，均失败；只有两者都存在才通过（仅验证存在性） |

预发布拼写 `1.3.0-alpha.1` 另在一份载体副本上整条链路重跑（`prerelease-*` 用例），因为仓库当前载体的版本由 prepare-release 决定。

deb 排序验证：`python3 -I apps/gui-go/e2e/linux/deb_version_order_run.py --out <新的空目录>`，在 Debian 容器里用真实 `dpkg` 与 `build_deb`：`1.3.0~alpha.1` → `alpha.2` → `1.3.0` 依次升级，稳定版之上装 alpha 被 `dpkg` 报告为降级。用修复前的 `package_linux.py`（`--baseline`）运行同一脚本会失败。

两个入口都挂在 `updater-e2e.yml`（PR 与手动触发，只读，不写任何外部系统）。

## 渠道触发边界

| 渠道 | 本次状态 |
| --- | --- |
| Snap | alpha 触发 `release=false`；`snap.yml` 默认同样不推送。推送需人显式 dispatch。 |
| COPR | alpha 触发 `dry_run=true`。Go rpm 从未真实提交过 COPR（见渠道文档），故不自动提交。 |
| npm | 不变：alpha 仍通过 `repository_dispatch` 触发发布 CLI 包（不可撤销）。该入口是否应在 Go 宿主发布时保持自动，需要维护者决定（见报告）。 |
| GitCode 镜像 | 不变：alpha 在 Release、R2 与 FlareRelease 登记成功之后 dispatch；`mirror` environment 仅允许 `main`。 |
| 稳定渠道 | 不变：Release 为 draft，FlareRelease 需显式 Promote；Tauri 用户的稳定渠道不受影响。 |

## 仍然阻塞或未验证

- 生产 Windows 代码签名后端（`WINDOWS_SIGN_BACKEND` 与对应凭据、SignPath 生产策略或 Azure Artifact Signing）不存在：生产签名验收为 blocked，未运行。
- 在 GitHub Actions 上的真实运行（tag 触发、构建、全平台包）未做：没有 push、没有 dispatch。所有证据来自本地。
- 生产更新私钥、`MINISIGN_RELEASE_PRIVATE_KEY`、R2 / FlareRelease 凭据的真实可用性未验证。
- `workflow_dispatch` 路径（`release.yml` 里的 `bump`）不是标准入口，本任务未改动，其二次 bump 与 `buildinfo` 不重新生成的问题见报告。

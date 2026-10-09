# Go 宿主的更新签名与渠道发布

## 信任来源与所有权

`apps/gui-go/app.json` 的 `updater.pubkey` 是唯一生产更新信任锚。Go 客户端、发布签名工具和隔离验收都通过 `internal/update.ParsePublicKey` 读取该值；生产构建不允许更换它。

`apps/gui-go/cmd/updater-sign` 复用现有固定依赖 `aead.dev/minisign v0.3.0` 的解密、签名 API；不实现密码学，也不恢复 Tauri GUI 或运行时。兼容输入为 minisign 私钥文本，或 Tauri 使用的「私钥文本再做一次 base64」格式。密钥材料和密码只读取环境变量 `TAURI_SIGNING_PRIVATE_KEY`、`TAURI_SIGNING_PRIVATE_KEY_PASSWORD`，CI 将同名仓库 secrets 仅交给签名步骤。错误不回显输入，不将密钥写入磁盘。

工具对最终归档字节签名，写出 `<artifact>.sig`（minisign 签名文本的 base64）。每个输出先由生产 `update.Client.Verify` 验证，包括可信注释。支持 `.app.tar.gz`、`.AppImage.tar.gz` 和 `-setup.exe`，它们与消费者安装路径和现有生成器一致。`SHA256SUMS.txt.minisig` 仍使用独立的发布校验和密钥，不能用于 updater。

## 发布入口

`release.yml` 保留现有全平台发布阻断。阻断解除后，顺序为：

1. `collect-release-assets.py` 只收集明确命名的安装器、更新归档、便携包和 CLI 归档；原始可执行文件、签名输入和 `package-manifest.json` 留在 CI。macOS 无架构的 tar 名从 CI artifact 目录的 target triple 派生；重名直接拒绝。
2. 签名工具对最终命名的归档签名并保存公共验签证据。
3. **唯一** `assemble-update-manifest.js --require-all-platforms` 要求六个平台：darwin/linux/windows × aarch64/x86_64。任何同优先级重复候选均拒绝。
4. **唯一** `build-flare-release-registration.js` 生成含 SHA256 的注册载荷。缺文件、缺平台、缺双语说明在创建 tag/Release 或上传 R2 前失败。
5. 保存验签与注册工件；之后沿用原有校验和签名、Release/R2 和 FlareRelease 注册步骤。

Ready 注册与渠道提升是两回事；不能因为注册成功就将该版本写入 fallback。

## 无发布的生产密钥验收

已有的 `build.yml` 手动入口新增 `platform=updater-signatures` 与 `updater_artifact_runs`。此模式跳过所有构建、Sentry、安装和发布，仅调用 `updater-signatures.yml`：

```bash
gh workflow run build.yml --ref <reviewed-branch> \
  -f platform=updater-signatures \
  -f updater_artifact_runs=<successful-build-run-id>,<successful-build-run-id>
```

输入必须是同一仓库的成功 Build Desktop run。只下载明确命名的 GUI artifact 及其包装证据。选择输入 run 是操作人的责任；run 的 `head_sha` **不等于** 每个重跑 job 的 checkout SHA，必须同时查看包装 `provenance.json` 和原始 job 日志。工具不执行归档里的代码。

验收保存 run ID/attempt、artifact ID/digest、输入与签名 SHA256、当前源码 SHA、工具版本与 app.json hash。签名后通过本地 HTTP feed 驱动生产 `Client.Check/Download/Verify`，同时拒绝篡改数据。只有签名与公共 JSON 证据上传，不上传私钥、密码或环境转储。

下载脚本按打包作业实际产出的 artifact 名选择输入：macOS `macos-gui-<target>[-test]`，Windows `windows-gui-<target>[-test][-signpath-test]`，Linux `linux-gui-<amd64|arm64>`（`package-linux-gui.yml` 也可单独触发，故其 run 同样可信），以及对应的 `*-gui-evidence-*`；证据保存在 `updater-evidence/`，不放进收集器扫描的 `updater-inputs/`，因为 Windows 证据里带有一份已签名的 CLI 归档（`cli-package/uniclipboard-cli-*.zip`），会被收集器当作第二份同名发布物。只有 macOS 包时，验收明确只证明两种 macOS 输入，不会生成伪 Linux/Windows 输入来冒充真实六平台。合成六平台 E2E 使用临时加密密钥，证据单独保存：

```bash
python3 -I apps/gui-go/e2e/updater_signatures_run.py --out <new-evidence-directory>
python3 -I apps/gui-go/e2e/update_pages_run.py --out <new-evidence-directory>
python3 -I apps/gui-go/e2e/updater_real_inputs_run.py --inputs <downloaded-artifact-directories> --out <new-evidence-directory>
```

`updater_real_inputs_run.py` 对真实 CI 包（六个更新归档加 dmg、deb、rpm、AppImage、便携包等同批命名分发物）依次执行收集、一次性密钥签名、无密钥重验、`--require-all-platforms` 生成 feed、注册载荷、本地 HTTP 下载六个平台并确认各平台取到各自归档、签名后篡改被拒。大体积包字节只留在临时目录，证据保存名称、大小、SHA256、签名、feed 与注册载荷。

## Pages fallback 的发布者

所有者为 Desktop 发布维护者，仓库入口为 `publish-update-pages.yml`。现有 Pages 源是 `gh-pages` 根目录；此流程保留其他站点文件，只改当前服务实际支持的 `stable.json`、`alpha.json`。

流程在 main 上手动或每 15 分钟执行，也响应正式发布事件。它从 FlareRelease 获取 **当前已提升渠道** 的完整 JSON，包含确认信息；发布前重新核对 primary 快照，避免用本地生成的 Ready 版本覆盖当前渠道。primary 返回 204 或明确的 `404 {"error":"Release not found"}` 时删除该渠道 fallback；路由缺失、其他 404 或其他错误均阻止发布。快照可重复校验并有 hash。

使用现有 `REPO_BOT_TOKEN` 提交到 `gh-pages`，让 legacy Pages 构建实际触发（`GITHUB_TOKEN` 写入不能被当作 Pages 发布证明）。发布后检查部署的两个响应与 primary 语义一致；超时或 primary 再变化时失败，重新取快照再运行。任务实现阶段不执行此生产流程。

定时复制存在至多一个调度周期加 Pages 构建时间的同步延迟；不提供瞬时撤回保证。primary 不可达时不生成新快照，也不宣称 fallback 新鲜。需要立即同步渠道提升/撤回时，维护者应立即手动运行该流程并检查结果。

```bash
python3 scripts/sync-update-pages.py snapshot --directory <new-snapshot>
python3 scripts/sync-update-pages.py check --directory <snapshot> \
  --fallback https://uniclipboard.github.io/UniClipboard
```

## 尚不能据此关闭 #1896

生产私钥解密及生产公钥验签、真实 Go 包、真实 FlareRelease 接受、真实 feed 下载、各 OS 安装与启动是独立证据。隔离本地 feed 不代表真实发布；交叉编译或显式 target 也不代表各 OS 安装。

当前服务源码未提供 staging 配置或只读注册验证入口。未经批准不得向生产注册试验版本，也不得为了验收修改服务 credentials/保护规则。待 Windows/Linux 生产打包接入及发布授权后，需验证真实六平台注册（含 linux-aarch64）、渠道提升、Pages 一致性和各 OS 真实下载安装。当前 `?from=` 仍不由客户端发送；这项实现没有改变其分析/路由语义。

客户端也定义 beta/rc，但当前 FlareRelease 不提供这两个 Desktop 路由，publisher 不伪造它们，也不修改相关文件。新增渠道需要先由服务明确提供对应路由及渠道权威来源。

## 已记录的隔离验收（2026-10-08）

[生产密钥验收 run 37713328581](https://github.com/UniClipboard/UniClipboard/actions/runs/37713328581) 在源码 `049c6b1aa62aaf5f0cfa44abd415ba65e2bcdad9` 上完成，工具为 Go 1.27.1（linux/amd64）。现有加密 updater secret 被成功解密；两个真实 Go macOS 归档的签名由不变的 app.json 公钥、`Client.Verify` 和本地 HTTP `Client.Check/Download` 正向验证，篡改字节被拒绝。其他构建 job 全部跳过。

| 输入架构 | 真实归档 SHA256 | `.sig` 文件 SHA256 |
| --- | --- | --- |
| aarch64 | `c80397a0d249d91037642dccf556b1809efbf2f327251448772462ec4cebd7b7` | `6120d0a56f898d7ec5a1b733cfc24c987c9d3e5121b0d4c3d4f3097ce2ea379c` |
| x86_64 | `dc56eb0ec998c079500b260ca7981f4f970db519548ce2f3f6e875b78fe602f9` | `4bce2dd035a67243e0a676bc482c97c35991456bef844bff21831a58af04fcf8` |

输入分别来自 [run 37638284857](https://github.com/UniClipboard/UniClipboard/actions/runs/37638284857) 和 [run 37638290509](https://github.com/UniClipboard/UniClipboard/actions/runs/37638290509) 的 test-mode 包。包装 provenance 的源码为 `32b9346810a83705c24a68435a91daef52148ef2`；workflow/sidecar 来源为 `368a27b3e2b34e9b8a0f717c82a5395b8de0b39d`。它们不是本次重新完成的优化 release 构建，也没有在本次验收中被安装或启动。

最初 run `37713132288` 是启动失败，未执行任何 job；GitHub annotation 指出 reusable workflow 调用方只允许 `actions: none`。修复仅为隔离调用 job 赋予 `contents: read`、`actions: read`；随后新 run 才构成有效证据。

FlareRelease 的不可变源码 `c5d4581dcb239862643cade8931e60799b7ede36` 在任务专用本地 Worker/D1/R2 环境中接受了六个平台的合成注册载荷，状态为 Ready，随后读回六条 artifact（含 linux-aarch64）。此结果仅证明本地服务合同；没有执行远端 staging/生产注册。

## 真实六平台包的隔离验收（2026-10-08，一次性密钥）

输入由修正后的 `download-updater-inputs.py` 对下列四个 run 实际下载得到（`updater_real_inputs_run.py --inputs updater-inputs`）：Linux 来自 [package-linux-gui run 37768856110](https://github.com/UniClipboard/UniClipboard/actions/runs/37768856110)（源码 `d960d3480`，amd64 与 arm64 原生 runner），Windows 来自 [Build Desktop run 37875320922](https://github.com/UniClipboard/UniClipboard/actions/runs/37875320922)（SignPath 测试 Authenticode 签名模式），macOS 来自上文两个 test-mode run。

`updater_real_inputs_run.py` 全部通过：收集到 16 个命名分发物，其中 6 个更新归档获得 `.sig` 并由产品的 `update.Client.Verify` 实现使用一次性测试公钥验证（不是生产公钥）；`--require-all-platforms` 生成含 darwin/linux/windows × aarch64/x86_64 的 feed；注册载荷含六个平台（含 linux-aarch64）；本地 HTTP 下客户端按显式 target 选择逐个下载六个平台各自归档并通过验签，篡改字节被拒（只有客户端 target 选择与下载验证，没有 native 安装）。Linux 的裸 `.AppImage` 与 `.deb`、`.rpm`、便携包、`.dmg` 不产生 `.sig`，不进入 feed。

| 平台键 | 归档 | SHA256 |
| --- | --- | --- |
| darwin-aarch64 | `UniClipboard_aarch64-apple-darwin.app.tar.gz` | `c80397a0d249d91037642dccf556b1809efbf2f327251448772462ec4cebd7b7` |
| darwin-x86_64 | `UniClipboard_x86_64-apple-darwin.app.tar.gz` | `dc56eb0ec998c079500b260ca7981f4f970db519548ce2f3f6e875b78fe602f9` |
| linux-aarch64 | `UniClipboard_1.1.1_aarch64.AppImage.tar.gz` | `d6b5a7869837305e50bca42e2de77394d4a95897ff661f76ac79a78c101d0cba` |
| linux-x86_64 | `UniClipboard_1.1.1_amd64.AppImage.tar.gz` | `a3dcbb27d3188ce6dcdc271cc626913c23a1d858d65a631aa6c7f787d410d43b` |
| windows-aarch64 | `UniClipboard_1.1.1_arm64-setup.exe` | `d49381ecda178f16a0b01575729c9a1c883a95e699bec28174aba85e4a8d86b4` |
| windows-x86_64 | `UniClipboard_1.1.1_x64-setup.exe` | `5a030def68c7e1865484412c08b1bcfa4f67816142ea5df24cc63312d566efbc` |

边界：签名密钥是一次性的，**不是**生产密钥；Windows 安装包上的 SignPath 测试 Authenticode 签名与 updater 的 minisign 签名是两件事，互不替代（minisign 对最终 Authenticode 签名后的字节签名）；没有安装、没有向 FlareRelease 写入、没有原生 OS 覆盖。此前下载脚本只匹配 `linux-gui-<arch>-unknown-linux-gnu`（真实 artifact 名是 `linux-gui-amd64|arm64`）且拒绝 Windows 的 `-signpath-test` 后缀，生产密钥验收因此无法取到真实 Linux 包，也会漏掉 Windows 包，本次已修正选择规则。

发布资产收集边界：`release.yml` 用 `download-artifact` 把所有 artifact 下载到 `artifacts/<名称>/` 再交给 `collect-release-assets.py`。既定来源（`gui-go-windows-packaging.md`）是：发布用 CLI 归档来自 `build.yml` 的 `build-cli` 作业（artifact `cli-<target>`，`require_signing` 时 `package-cli.sh` 拒绝未签名输入），安装包来自 `windows-gui-<target>` 等 GUI 包 artifact。`*-gui-evidence-*`（含 GUI 作业为验收重打的已签名 CLI 副本）、`*-gui-acceptance-inputs-*` 和 `signpath-stage<N>-*`（含最终签名前的 `UniClipboard_<版本>_x64-setup.exe`）只是证据或中间物。收集器现在按 artifact 顶层目录名跳过它们，不改变重名即拒绝的规则，也不改变签名策略。

复现（真实 artifact 布局，`scripts/ci/release_asset_collection_run.py`）：修复前的收集器因 `duplicate release asset: uniclipboard-cli-…zip` 失败；修复后成功，CLI 取自 `cli-<target>`、安装包取自最终 `windows-gui-<target>`，没有 `ACCEPTANCE-` 包。`cli-<target>` 在该输入 run 中不存在（`package_cli` 未开），E2E 用明确标注的合成字节代替，只证明来源选择，不证明 CLI 本身已签名。

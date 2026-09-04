# 版本化升级 userdata 样本

本目录保存会随仓库一起版本管理的旧版本开发资料，用于快速、稳定地重复执行升级测试。样本只由官方旧版本在 `UNICLIPBOARD_ENV=development` 下生成，不得放入任何真实用户资料或系统凭据。

## 目录结构

```text
fixtures/upgrades/
├── v0.19.1/
├── v0.20.0-alpha.2/
├── v0.20.0-alpha.6/
└── v1.0.0-alpha.4/
    └── macos-aarch64/
        └── single-node-empty/
            ├── manifest.json
            └── userdata.tar.gz
```

每个样本目录包含：

- `manifest.json`：来源版本、官方发布包摘要、平台、场景、压缩包摘要，以及包内每个文件的大小和摘要。
- `userdata.tar.gz`：经过白名单筛选的开发资料和缓存。测试不会直接修改它，而是先恢复到全新的随机开发档案。

## 样本边界

允许保存：

- SQLite 主数据库和仍承载已提交内容的 WAL。
- 开发文件密钥、keyslot、设置状态和合成设备身份。
- 旧版带档案名后缀的 Iroh 身份与内容库；打包时改为中性目录，恢复时再绑定到本次随机档案名。
- 场景需要的合成文字、图片和文件内容。

禁止保存：

- `daemon.conn`、`.daemon-token`、`.daemon-pid`、`.uniclipd.lock`。
- `daemon-run.json`、`daemon-last-exit.json`、日志和崩溃现场。
- SQLite `-shm`、临时文件和其他可重建运行状态。
- 真实用户内容、真实设备身份、系统钥匙串内容、本机绝对路径和私人服务地址。

## 两条验证路径

仓库样本用于每次改动的快速回归，但不能取代官方旧版本现场生成：

1. 日常回归从本目录恢复 userdata，先验证摘要，再在全新的 `dev-upgrade-*` 档案中启动当前版本。
2. 发布前继续运行 `upgrade_from_v0_19_1` 与历史成员兼容测试，由已校验的官方旧版本程序现场创建全部单机和多机状态，再原地升级。

## 当前版本矩阵

| 来源版本 | 选择理由 | 连接发现 |
| --- | --- | --- |
| `v0.19.1` | 最后一个稳定旧版基线 | 固定档案端口 |
| `v0.20.0-alpha.2` | 独立 Engine 切换前的旧内置核心和旧网络身份目录 | 固定档案端口 |
| `v0.20.0-alpha.6` | 持久 legacy bootstrap 与自动安全升级，网络身份已进入开发文件密钥 | 固定档案端口 |
| `v1.0.0-alpha.4` | workspace convergence、设备信任选择、旧版交接和 `daemon.conn` | `daemon.conn` |

`v1.0.0-alpha.5` 与 alpha.4 之间没有新的持久格式，不重复保存单机空样本；后续增加“已移除设备等待重新批准”多机场景时再使用。`v1.0.0-alpha.6/.7` 没有正式桌面发布资产，不作为官方旧版输入。

## 统一恢复规则

上述四个来源版本升级到当前版本后，一律采用相同规则：

1. 保留每台设备自己的历史、设置和设备身份。
2. 不继承旧设备组关系，每台设备先成为只有本机成员的独立设备组。
3. 明确提示用户需要重新配对，不允许等待其他旧设备升级后自动恢复。
4. 用户在一台保留设备上确认原空间口令并生成邀请，其他升级设备通过该邀请重新加入。
5. 离线设备稍后升级时仍须重新加入；已移除设备不会自动恢复，只有再次收到用户邀请才能加入。

完整现场测试会先用官方旧版程序建立两台设备的关系，让其中一台离线，再分别升级。测试随后确认两台设备都只保留本机、都显示重新配对要求，并在用户确认口令和重新邀请后恢复双向同步。`v0.19.1` 的三设备测试继续负责验证已移除设备不会回来。

验证样本与防篡改规则：

```bash
CARGO_INCREMENTAL=0 CARGO_BUILD_JOBS=2 RUSTC_WRAPPER= \
  cargo test --locked --manifest-path tests/e2e/Cargo.toml \
  --test upgrade_userdata_fixtures
```

运行仓库样本的真实升级：

```bash
CARGO_INCREMENTAL=0 CARGO_BUILD_JOBS=2 RUSTC_WRAPPER= \
  cargo test --locked --manifest-path tests/e2e/Cargo.toml \
  --test upgrade_userdata_fixtures \
  tracked_v0191_fixture_upgrades_to_current_in_a_fresh_dev_profile \
  -- --exact --ignored --nocapture
```

运行 0.20 / 1.0 alpha 破坏边界矩阵：

```bash
CARGO_INCREMENTAL=0 CARGO_BUILD_JOBS=2 RUSTC_WRAPPER= \
  cargo test --locked --manifest-path tests/e2e/Cargo.toml \
  --test upgrade_userdata_fixtures \
  selected_breaking_release_fixtures_upgrade_to_current_dev_runtime \
  -- --exact --ignored --nocapture
```

只重跑一个来源版本时增加 `UC_E2E_UPGRADE_VERSION=<版本号>`。

运行某个来源版本的完整两设备恢复流程：

```bash
UC_E2E_UPGRADE_VERSION=0.20.0-alpha.6 \
UC_E2E_UPGRADE_RELEASE_DIR=<已验证发布包目录> \
  cargo test --locked --manifest-path tests/e2e/Cargo.toml \
  --features historical-membership \
  --test membership_compatibility \
  h4_upgraded_legacy_devices_recover_only_after_explicit_re_pairing \
  -- --exact --ignored --nocapture
```

桌面窗口测试会在 macOS ARM 上自动恢复同一份仓库样本，不需要手工复制资料：

```bash
E2E_SPEC=e2e/specs/upgrade-re-pair-notice.e2e.js bun run e2e:wdio
```

该命令会依次验证四个版本的可见升级提示、错误口令和邀请生成。完整双窗口恢复与即时成功反馈使用：

```bash
E2E_SPEC=e2e/specs/upgrade-re-pair-success.dual.e2e.js bun run e2e:wdio:dual
```

## 重新生成 v0.19.1 样本

先使用仓库已有下载工具取得并校验官方发布包，再运行：

```bash
UC_E2E_V0_19_1_RELEASE_DIR=<已校验发布包目录> \
  CARGO_INCREMENTAL=0 CARGO_BUILD_JOBS=2 RUSTC_WRAPPER= \
  cargo run --locked --manifest-path tests/e2e/Cargo.toml \
  --bin build-v0-19-1-userdata-fixture
```

生成器只接受 macOS ARM，并始终使用全新的开发档案。它会重新校验官方摘要和发布压缩包、正常停止旧后台、筛选允许文件、关闭样本中的使用统计、标准化旧 Iroh 目录，再生成压缩包和清单。生成后的身份和摘要变化是正常的，但必须同时审查清单、包内路径和真实升级结果。

## 重新生成 0.20 / 1.0 alpha 样本

先下载并验证选定旧版程序：

```bash
cargo run --locked --manifest-path tests/e2e/Cargo.toml \
  --bin prepare-upgrade-release -- \
  --version 0.20.0-alpha.6
```

再从输出的发布目录生成样本：

```bash
cargo run --locked --manifest-path tests/e2e/Cargo.toml \
  --bin build-upgrade-userdata-fixture -- \
  --version 0.20.0-alpha.6 \
  --release-dir <已验证发布目录>
```

两个命令只接受版本目录表中的来源。旧后台不得直接用 `--version` 探测；历史版本不保证支持该参数，必须由生成器在随机 `dev-upgrade-*` 档案和开发文件密钥下启动。

## 增加后续版本

只为实际的升级边界增加目录，例如数据库、密钥格式、设备关系或文件布局发生变化的稳定版本，不需要为每个补丁版本复制一份。

每个新版本必须同时具备：

1. 使用官方发布程序和开发文件密钥的生成器。
2. 明确的文件白名单和平台目录。
3. 压缩包与逐文件摘要。
4. 篡改拒绝测试。
5. 从仓库样本恢复到随机开发档案后的真实升级测试。
6. 发布前由官方旧程序现场生成的完整场景测试。

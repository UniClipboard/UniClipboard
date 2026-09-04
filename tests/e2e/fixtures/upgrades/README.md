# 版本化升级 userdata 样本

本目录保存会随仓库一起版本管理的旧版本开发资料，用于快速、稳定地重复执行升级测试。样本只由官方旧版本在 `UNICLIPBOARD_ENV=development` 下生成，不得放入任何真实用户资料或系统凭据。

## 目录结构

```text
fixtures/upgrades/
└── v0.19.1/
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
2. 发布前继续运行 `upgrade_from_v0_19_1`，由已校验的官方旧版本程序现场创建全部单机和多机状态，再原地升级。

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

桌面窗口测试会在 macOS ARM 上自动恢复同一份仓库样本，不需要手工复制资料：

```bash
E2E_SPEC=e2e/specs/upgrade-re-pair-notice.e2e.js bun run e2e:wdio
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

## 增加后续版本

只为实际的升级边界增加目录，例如数据库、密钥格式、设备关系或文件布局发生变化的稳定版本，不需要为每个补丁版本复制一份。

每个新版本必须同时具备：

1. 使用官方发布程序和开发文件密钥的生成器。
2. 明确的文件白名单和平台目录。
3. 压缩包与逐文件摘要。
4. 篡改拒绝测试。
5. 从仓库样本恢复到随机开发档案后的真实升级测试。
6. 发布前由官方旧程序现场生成的完整场景测试。

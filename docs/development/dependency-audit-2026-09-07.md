# 依赖审计与 CLI 轻量化

日期：2026-09-07。范围：Desktop Rust 工作区、根前端包；额外扫描诊断与端到端测试包。

## 结论

优先删除错误的依赖边，其次删除无用声明，最后处理兼容版本重复。多个组件声明同一个版本不会自动产生多份运行库；把声明搬到 `workspace.dependencies` 主要改善维护，不等于缩小安装包。

本轮完成：18 条 Rust 无用依赖声明、4 条前端无用依赖声明的清理；将 CLI 的 `uc-observability` 改为仅在 `dev-tools` 下启用；合并一处 `tailwind-merge` 重复版本。

## 统计口径

- Rust 统计来自当前 macOS 主机的 `cargo tree -e normal,build`，包含构建期依赖，排除测试依赖，按包版本去重，包含根包。
- 普通 CLI：**207 → 180 个包，减少 27 个，约 13.0%**。不表示安装包体积或启动时间减少相同比例。
- 清理前整个工作区在当前主机上有 802 个不同包版本。此集合混合 GUI、daemon、诊断工具和构建工具，不能当作 CLI 的交付依赖。
- 本机已经使用本地 Engine 覆盖，解析到 `1.1.0-rc.7`；报告反映当前工作目录，不是仅依赖仓库固定 Engine 提交的干净检出。未修改 Engine 或覆盖配置。
- 前端锁文件同时包含开发工具和其他平台安装项；其重复数量不能直接当作浏览器包的重复数量。`docs-site` 有独立依赖与锁文件，本轮没有修改它。

## 已完成的清理

| 位置 | 清理内容 | 依据 |
| --- | --- | --- |
| `apps/cli/Cargo.toml` | `uc-observability` 移入 `dev-tools` | CLI 唯一使用点在 `commands/app_session.rs::build_app_session`，该函数本来就受 `dev-tools` 控制 |
| `crates/uc-platform/Cargo.toml` | 删除 `base64`、`bytes`、`futures`、`libc`、`log`、`mockall`、`once_cell`、`serde_json`、`sha2`、`subtle`、`tokio-util` | 扫描后逐项核对源代码，没有实际使用 |
| `crates/uc-bootstrap/Cargo.toml` | 删除 `chrono` | 无使用点 |
| `crates/uc-desktop/Cargo.toml` | 删除生产和测试两处 `chrono`，以及测试依赖 `diesel`、`mockall`、`tempfile` | 无使用点；其中 `diesel` 会让纯客户端的独立测试无谓带入数据库 |
| `crates/uc-observability/Cargo.toml` | 删除 `sha2` | 当前代码没有直接使用；Engine 契约包自己拥有所需的哈希依赖 |
| 根 `package.json` | 删除 `next-themes`、`react-masonry-css`、`@tailwindcss/postcss`、`autoprefixer` | 当前应用没有使用前两个包；构建走 `@tailwindcss/vite`，没有根 PostCSS 配置 |
| 根 `bun.lock` | 删除 `shadcn` 下的 `tailwind-merge@3.5.0`，复用根 `3.6.0` | `shadcn` 要求 `^3.0.1`，已有 `3.6.0` 满足范围；未加覆盖规则，未升级其他包 |

CLI 默认依赖图已不再包含 `uc-observability`、`uc-observability-contract`、`tracing-subscriber`、`tracing-appender` 和 OpenTelemetry。原有数据库、同步引擎、GUI 框架隔离仍然成立。

验证时还修正了 `apps/cli/src/commands/member_trust.rs` 的旧测试数据：补齐当前设备分组契约新增的字段。仅修改测试初始化，未改变命令行为。

## 不能直接删除的扫描结果

| 项目 | 保留原因 |
| --- | --- |
| `uc-platform` 的 `libdbus-sys` | 在 Linux musl 上开启 `vendored`，用于构建方式选择；没有直接导入不代表无用 |
| 打包壳的 `tauri-plugin-*` | `tauri-build` 通过直接依赖发现插件权限，运行代码在 `uc-tauri` 并不意味着壳内声明可以删除 |
| 打包壳的 `tauri-build`、`serde_json` | 构建脚本和 `generate_context!` 展开需要 |
| 根前端的 `scheduler` | `use-context-selector` 明确将它声明为必要的配套依赖 |
| WebdriverIO runner、framework、reporter，`react-grab`、`@react-grab/mcp` | 通过配置或开发入口使用，不只通过普通源代码导入 |
| `objc2-quartz-core` | macOS 代码对返回的 layer 调用方法，需要核实功能开关传播后再删除直接声明 |

`cargo machete` 仍报告上述部分项目。本轮没有用忽略名单掩盖结果，也不能宣称整个扫描已经清零。

## 后续优化顺序

### 1. 分离接口文档生成依赖

`uc-daemon-contract` 无条件依赖 `utoipa`，使 CLI 为只需传输的数据类型也编译接口文档相关代码。可以将文档生成设为服务端显式启用的功能，同时保留唯一一套数据类型定义。涉及大量派生声明及 `api/openapi_meta.rs`，应单独处理并检查服务端文档生成、前端生成结果和 CLI 独立构建。

### 2. 收窄 CLI 的网络与异步功能

`apps/cli` 和 `uc-daemon-client` 同时启用 `tokio/full`，后者的 `tokio-util/codec` 与实际使用的 `sync::CancellationToken` 不对应。要结合每个包单独构建验证最小功能，不能依赖整个工作区合并后的功能集掩盖缺失。

普通 CLI 主要访问本机 daemon，但 `reqwest` 仍启用完整 HTTPS 能力，`main.rs` 也无条件初始化 Rustls。进一步清理之前，必须确认公开客户端的 HTTPS 支持范围以及开发诊断命令的 TLS 初始化需求，不能仅根据当前使用本机地址就删除能力。

### 3. 处理诊断、测试与 GUI 的剩余候选

- `uc-cli-macros` 仍是工作区成员，但没有其他包声明依赖它。删除整个旧宏包能减少工作区维护和构建工作；它目前并不进入普通 CLI 的依赖图。
- `p2p-bench` 的 `bytes`、`tracing`，`tests/e2e` 的 `assert_cmd`、`predicates` 没有找到使用点；需分别覆盖诊断工具的可选功能与独立测试工作区后清理。
- `uc-tauri` 的 `base64`、`core-foundation`、`mockall`、测试用 `sentry`，打包壳的测试用 `tempfile`、`tokio` 也是候选；应与完整 GUI 打包验证一起处理。

### 4. 对重复版本按来源处理

| 重复项 | 当前原因与处理方式 |
| --- | --- |
| `reqwest 0.12 / 0.13` | Desktop 客户端使用 0.12；iroh、Engine 日志运行组件、Tauri 更新插件使用 0.13。普通 CLI 只有 0.12；合并需要分别验证上游和 TLS 行为 |
| `thiserror 1 / 2` | CLI 的 `dialoguer`、`tungstenite` 带入 1，内部客户端使用 2；仅修改 Desktop 自己的声明不能消除所有旧版本 |
| `syn 1 / 2 / 3` | 不同代码生成工具需要，主要影响构建，不应按三个完整运行库估算体积 |
| `socket2 0.5 / 0.6` | 来自 `hyper-util` 与 `tokio`，需要上游兼容的依赖调整 |
| 多代密码学、随机数和系统接口包 | 多数来自 Engine、网络栈和平台包，不能通过手改锁文件强制成同一版本 |

不建议为减少版本数字而批量升级依赖，也不建议跨到 Engine 仓库进行未单独验证的清理。

## 验证

- `cargo test -p uc-cli --locked --offline`：122 项通过。单独构建，未依赖其他工作区包合并功能。
- `cargo run -p uc-cli --locked --offline -- --help`：正常运行。
- `cargo check -p uc-cli --features dev-tools --all-targets --locked --offline`：通过，开发诊断功能仍可构建。
- `cargo test -p uc-daemon-client -p uc-platform -p uc-bootstrap -p uc-desktop -p uc-observability --locked --offline`：通过；平台条件测试和原有忽略的文档测试不计为已执行。
- `bun run build`：成功，macOS 12.5 兼容性检查通过。
- 主题同步、设置主题、外观设置三个测试文件：16 项通过。
- `bun install --frozen-lockfile --ignore-scripts`：通过；应用和 `shadcn` 实际都解析到 `tailwind-merge 3.6.0`；`shadcn --help` 正常。
- 未进行 Windows/Linux 实机构建、GUI 安装包验证、物理双设备同步验证或前后 release 文件体积测量。

复查命令：

```bash
cargo tree -p uc-cli --locked --offline -e normal,build
cargo tree -p uc-cli --locked --offline -e normal,build -d
cargo tree --workspace --locked --offline -i reqwest@0.13.4 -e normal,build
cargo machete --with-metadata
```

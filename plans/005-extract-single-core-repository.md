# Plan 005：迁入单一核心仓库并保留可选 LAN 通道

> **执行要求**：准备工作可以提前进行，但只有计划 004 的统一发布干跑和完成标准全部通过后，才能创建新的事实来源、发布候选版本或切换消费者。若实体设备矩阵继续按用户决定跳过，必须先在计划 004 和本计划中记录明确的风险接受；不得把“跳过”写成“通过”。
>
> **事实来源规则**：迁移只允许有一个可写事实来源。历史过滤后到消费者切换完成前，desktop 仓中的旧核心目录进入短期只读冻结；禁止在两个仓库同时修改同一源码。
>
> **漂移检查**：`git diff --stat 1c229e9e1..HEAD -- Cargo.toml Cargo.lock crates apps src-tauri .github/workflows docs plans`

## 状态

- **优先级**：P1
- **工作量**：XL
- **风险**：HIGH
- **依赖**：`plans/004-ship-mobile-bindings-and-conformance.md`
- **类别**：migration
- **计划基线**：`1c229e9e1`，2026-07-19
- **当前状态**：IN PROGRESS
- **执行进度**：Phase 0 至 Phase 5 已完成；候选版本已发布，desktop 已切换到独立核心，下一阶段是 Android 和 iOS 消费端迁移

**实体设备风险接受（2026-07-24）**：用户已明确决定跳过 Plan 004 剩余六种设备对的实体设备矩阵，并要求继续创建独立核心仓库。该矩阵继续记录为未通过；本决定只允许统一发布干跑和建仓准备继续，不构成互通验收证据，也不允许把 Plan 004 标记为全部完成。

## 结论

建立 `UniClipboard/core`，使用“一个仓库、多个内部 crate、一个稳定入口、一次统一发布”的结构。

- 外部 Rust 调用方只依赖 `uc-engine`。
- iOS、Android、HarmonyOS 只消费同一提交生成的绑定产物。
- `uc-core`、`uc-application`、`uc-infra` 等属于仓库内部实现，不单独承诺稳定。
- 各产品仓继续拥有系统生命周期、安全存储、剪贴板、文件选择和界面接线。
- LAN HTTP 兼容能力可以放在同一仓库的 `compatibility/` 下，但使用独立版本、独立产物和独立发布线；默认 P2P 核心不启用它。

不能直接把当前目录搬走。当前 desktop 中仍有多个 crate 直接使用 `uc-core`、`uc-application`、`uc-infra` 和 `uc-mobile-proto`，`uc-infra` 也仍依赖完整的 desktop 观测实现。必须先完成依赖收口，再做物理迁移。

## 仓库所有权

### 迁入 `UniClipboardCore`

| 范围 | 内容 | 说明 |
| --- | --- | --- |
| 稳定入口 | `uc-engine` | 唯一稳定 Rust 接口和行为约定 |
| 内部实现 | `uc-core`、`uc-application`、`uc-infra` | 全部设为 `publish = false`，不对消费者承诺稳定 |
| 可移植基础 | `uc-content-hash`、`uc-observability-contract` | 核心和绑定共同需要的叶子能力 |
| iOS/Android 绑定 | `uc-engine-uniffi` | 生成 XCFramework、AAR、Swift/Kotlin 绑定 |
| HarmonyOS 绑定 | `uc-ohos-napi` | 生成动态库、HAR 组装输入和 ArkTS 声明 |
| 持久化 | `uc-infra/migrations/` | 数据格式与核心版本一起演进 |
| 一致性检查 | 最小宿主、golden vectors、明文探针、升级探针 | 只通过公开入口验证行为 |
| 发布工具 | 四平台构建脚本、校验、许可证清单、调试符号 | 所有产物来自同一提交 |
| LAN 兼容 | `uc-mobile-proto`、`uc-mobile` 和后续兼容专用代码 | 放在 `compatibility/`，保持 `uc-mobile-v*` 独立发布 |

### 留在 desktop 仓库

- `uc-platform`、`uc-bootstrap`、`uc-observability`
- `uc-webserver`、`uc-daemon-*`、`uc-desktop`
- daemon、CLI、Tauri 和全部桌面打包代码
- 桌面系统剪贴板、安全存储、自动启动、日志和进程管理
- HTTP/WS 传输 DTO、桌面界面和产品发布流程

### 留在各移动产品仓

- iOS Keychain、Pasteboard、系统文件出口和生命周期接线
- Android Keystore、ClipboardManager、前台服务和生命周期接线
- HarmonyOS Asset Store/HUKS、系统剪贴板、文件选择和生命周期接线
- Expo、Swift、Kotlin、ArkTS 的产品模块、界面和商店打包

系统宿主源码不进入核心仓。核心仓只定义它们必须满足的宿主能力，并发布绑定。

## 目标目录

```text
core/
├── Cargo.toml
├── Cargo.lock
├── rust-toolchain.toml
├── LICENSE
├── AGENTS.md
├── README.md
├── docs/
│   ├── architecture/
│   ├── security/
│   ├── compatibility/
│   └── release/
├── crates/
│   ├── uc-engine/
│   ├── uc-core/
│   ├── uc-application/
│   ├── uc-infra/
│   ├── uc-content-hash/
│   └── uc-observability-contract/
├── bindings/
│   ├── uc-engine-uniffi/
│   └── uc-ohos-napi/
├── compatibility/
│   ├── uc-mobile-proto/
│   └── uc-mobile/
├── tests/
│   ├── conformance/
│   ├── migration/
│   └── persistence-scan/
└── .github/workflows/
    ├── pr-check.yml
    ├── release-core.yml
    └── release-lan-compat.yml
```

目录只是所有权表达，不改变对外接口。核心内部仍可保持多个 crate，消费者不需要理解这些内部层次。

## 版本与发布

### 统一核心版本

- 新仓使用独立的 `core-vMAJOR.MINOR.PATCH[-rc.N]` 版本线。
- 首个公开候选版本为 `core-v0.20.0-rc.1`。
- `uc-engine`、内部 crate 和三种绑定使用同一个 workspace 版本。
- 内部 crate 不发布到 crates.io；desktop 通过精确 Git 提交消费 `uc-engine`。
- desktop 的 `Cargo.lock` 必须记录对应提交，不得依赖分支、浮动 tag 或主干。

desktop 依赖形态：

```toml
uc-engine = {
  git = "https://github.com/UniClipboard/core.git",
  rev = "<immutable-commit-sha>"
}
```

`core-v*` 标签必须指向同一提交，并由仓库保护规则禁止移动或覆盖。

### 发布产物

一次 `core-v*` 发布必须从同一提交生成：

- Rust 源码标签、`Cargo.lock` 和依赖许可证清单
- iOS `UniClipboardEngine.xcframework.zip`、Swift 绑定和 SwiftPM 校验值
- Android `UniClipboardEngine.aar`、Kotlin 绑定、POM 和运行依赖清单
- HarmonyOS 动态库、HAR 组装输入、ArkTS 声明和校验值
- 四平台调试符号
- `release-manifest.json`

`release-manifest.json` 至少记录：

- 核心版本、完整提交、Rust 工具链和锁文件 SHA-256
- 每个产物的文件名、目标平台、架构、SHA-256 和大小
- UniFFI、N-API、Kotlin、Swift、ArkTS 生成工具版本
- P2P 协议范围、数据库版本、最新迁移和最低支持系统
- 已完成与明确跳过的设备矩阵，不得把跳过项记为通过

Release 资产不可覆盖。发现坏版本时发布新版本并标记旧版本不可用。

### LAN 兼容版本

- LAN 继续使用 `uc-mobile-vMAJOR.MINOR.PATCH`。
- LAN 产物不得出现在 `core-v*` 的默认移动包中。
- LAN 代码不得读取 P2P 失败信号来触发自动切换。
- LAN 与 P2P 可以位于同一源码仓库，但必须有独立工作流、发布清单和消费者固定版本。

## 跨仓协作规则

1. 核心行为、协议、存储或绑定变化先进入核心仓。
2. 核心仓发布 RC，产品仓只消费 RC，不复制补丁。
3. 产品仓发现问题时回到核心仓修复并发布下一 RC。
4. 系统能力和界面变化留在对应产品仓。
5. 紧急修复可以固定未发布的精确核心提交，但不得创建产品仓内的核心分叉。
6. 本地联调使用未跟踪的 Cargo override 或临时构建产物；CI 必须拒绝本地路径依赖。

## 执行阶段

### Phase 0：固定迁移基线

**目的**：避免在脏工作区和变化中的核心上做历史切割。

1. 完成并提交当前 `uc-engine` 结构迁移。
2. 完成 Plan 004 的统一四平台发布干跑。
3. 处理实体设备矩阵：要么真实通过，要么由用户明确接受跳过风险并写入两份计划。
4. 记录 cutover commit、依赖图、发布产物校验和数据库版本。
5. 将所有核心目录设为短期变更冻结，指定唯一迁移负责人。

**进入条件**：工作区干净；核心、绑定和迁移文件全部已提交；没有未追踪的发布资产。

**验收**：

- `git status --short` 为空。
- Plan 004 的统一发布干跑来自同一提交。
- cutover commit 已推送且不可变。

**回退**：未创建新仓库前直接取消冻结，不改变任何消费者。

**完成进度（2026-07-24）**：desktop 迁移基线固定为 `12104cbab7a3b167f33f95c5a9d6d7d90fbbfa75` 并已推送。独立仓提交 `47018f40800f8d3671b960de2ed5911b6f3c76b2` 完成 `core-v0.19.1` 统一发布干跑，iOS、Android、HarmonyOS 文件均记录同一版本和来源提交，22 项资产校验全部通过。实体设备矩阵继续明确记录为跳过而非通过。

### Phase 1：建立依赖防火墙

**目的**：让物理拆仓只是改变来源，不再同时重写接口。

1. `uc-webserver` 的正式业务调用全部改为 `Engine::execute` 和 `Engine` 结果类型。
2. `uc-bootstrap` 只准备宿主能力，不再依赖 `uc-application` 或 `uc-infra`。
3. `uc-platform` 改用 desktop 自有的系统快照类型，并在宿主组装处转换为 `HostCapabilities`。
4. daemon contract/client、CLI、Tauri 删除对 `uc-core` 的直接依赖；传输 DTO 由 daemon contract 自己拥有。
5. `uc-infra` 删除对完整 `uc-observability` 的依赖，后台任务交给核心任务管理或可移植约定。
6. `uc-application` 和 `uc-infra` 删除对 `uc-app-paths` 的依赖：核心文件布局常量移入核心，portable 状态由宿主配置传入。
7. engine 检查使用核心仓本地记录器，不依赖 desktop 的观测实现。
8. 将 `uc-application`、`uc-infra` 和 `uc-engine` 中的 LAN 专用代码放到显式兼容 feature，默认依赖图不再包含 `uc-mobile-proto`。
9. P2P 移动产物默认不启用 LAN 兼容 feature；兼容发布才显式启用。

**提前完成进度（2026-07-23）**：第 1-9 项已完成。`uc-webserver` 正式业务和 LAN 兼容运行时均只通过 `Engine` 调用核心；`uc-bootstrap` 只准备桌面宿主能力，`uc-platform` 自行拥有平台快照、目录和安全存储类型；daemon、CLI、Tauri 和轻量传输 crate 的正式依赖不再直接引用核心内部 crate。默认核心不再编译 LAN 协议依赖与真实实现，desktop 网页服务显式启用兼容能力，三种移动消费端保持默认关闭。为避免在拆仓准备阶段制造破坏性变更，现有兼容数据格式暂时保留；默认核心收到兼容操作时返回不可用。物理迁移、新仓创建、版本发布和消费者切换均尚未开始。

**验收**：

- desktop 正式依赖中，只有 `uc-engine` 可以指向待迁移核心。
- `uc-engine` 的普通依赖闭包不包含 `uc-platform`、`uc-bootstrap`、`uc-webserver`、daemon、Tauri 或完整 `uc-observability`。
- 绑定 crate 的普通依赖只有 `uc-engine` 和各自绑定运行库。
- 默认 P2P 发布构建不包含 LAN HTTP 客户端或服务器符号。

**停止条件**：任何 desktop 功能仍必须直接调用 `uc-application` facade、`uc-core` port 或 `uc-infra` 实现。

### Phase 2：建立跨仓检查

在搬迁前先把规则变成自动检查：

- core dependency firewall：拒绝任何 desktop 仓路径和平台外壳依赖
- public surface check：只允许 `uc-engine` 和绑定成为外部入口
- consumer firewall：desktop 拒绝迁出 crate 的本地路径依赖和内部类型引用
- binding provenance：二进制、生成代码、版本文件和来源提交必须一致
- persistence gate：业务负载保持密文，文件内容例外规则不变
- compatibility gate：P2P 失败不得触发 LAN 请求

这些检查先在 desktop 仓运行，迁移后原样归新仓或消费者仓各自拥有。

**验收**：故意加入一个反向路径依赖、错误绑定版本和自动 LAN 回退时，检查均能准确失败。

**提前完成进度（2026-07-24）**：六类规则已收敛为统一静态入口并接入 desktop PR 检查；反向依赖、绑定版本不一致和 P2P 失败自动切换 LAN 三个隔离错误样例均被准确拒绝。正常工作区的统一检查、格式检查、完整工作区全部目标编译和差异检查通过，按用户要求未运行测试用例。检查归属和迁移方式记录在 `docs/architecture/core-repository-checks.md`。

### Phase 3：保留历史创建核心仓库

1. 在临时目录克隆 cutover commit。
2. 安装并记录固定版本的 `git-filter-repo`。
3. 只保留本计划“迁入”清单中的源码、迁移、文档和发布脚本，并按目标目录重排。
4. 保留提交作者、时间、许可证和可追溯提交说明。
5. 加入新根 `Cargo.toml`、`Cargo.lock`、工具链、规则、README、AGENTS 和核心安全底线。
6. 迁移 root `[patch.crates-io]` 中固定的 `iroh-blobs` 提交，并保留审计说明。
7. 推送到新仓的受保护迁移分支；此时不发布、不切消费者。

不得用复制目录、subtree、submodule 或定时镜像代替历史过滤。

**验收**：从空目录全新检出后，不访问 desktop/mobile/HarmonyOS 本地路径即可完成：

```bash
cargo metadata --locked --format-version 1
cargo check --workspace --all-targets --locked
cargo test --workspace --locked
```

并完成 iOS、Android、HarmonyOS 发布干跑及持久化明文扫描。

**回退**：删除尚未成为事实来源的新仓迁移分支，desktop 解除冻结。消费者尚未变化。

**完成进度（2026-07-24）**：已创建公开仓库 `https://github.com/UniClipboard/core`。迁移使用 `git-filter-repo 2.47.0`，保留 348 个相关提交，来源基线与过滤后提交映射记录在新仓 `docs/migration/source-history-map.md`。新仓只保留核心、绑定、LAN 兼容线、验收宿主、文档、检查和发布工具，默认分支为 `main`。从独立检出完成依赖解析、全项目全部目标编译、格式、仓库边界、差异和仓库完整性检查；按用户要求未重复运行测试用例。三端发布文件已从当前远端提交重新生成并统一归集。远端检查已通过，`main` 要求最新检查、一人批准和讨论解决，管理员同样遵守，并禁止强制推送、删除和非线性历史。未创建 `core-v*` 标签或 Release，未切换任何消费者。

### Phase 4：发布 `core-v0.20.0-rc.1`

1. 从受保护提交触发 `release-core.yml`。
2. 先做 dry-run，确认所有平台产物和发布清单完整。
3. 从同一提交重建第二次，比较可复现字段和产物校验。
4. 创建不可移动的 `core-v0.20.0-rc.1` 标签和预发布 Release。
5. 从 Release 资产而不是工作区文件建立四个最小宿主。

**验收**：

- 标签、manifest、版本文件和所有产物指向同一提交。
- iOS/Android 绑定与本地库不能被错误版本混用。
- HarmonyOS 包能从 Release 资产独立组装。
- 最小宿主不需要核心仓源码即可启动、恢复身份并执行基础操作。

**回退**：不覆盖 RC；标记为不可用并发布 `rc.2`。

**完成进度（2026-07-24）**：`core-v0.20.0-rc.1` 已从受保护的 `main` 提交 `dcdccb234f020be49884bf92d886f25a0f192188` 发布为预发布版本。两次有效演练 `30054432143`、`30057385020` 均成功，稳定清单一致；正式发布运行 `30058537116` 全部通过。Release 共 23 个文件，其中清单声明并逐项校验 22 个文件，额外 1 个为清单本身；版本、来源提交、锁文件校验、文件名称、大小和 GitHub 校验值全部一致。iOS、Android、HarmonyOS 实体设备矩阵仍为 `skipped`，不计为通过。新仓已配置仅供本仓使用的 HarmonyOS 自托管构建机，并以登录后自动启动的后台服务保持在线。本阶段未切换任何消费者。

### Phase 5：切换 desktop

desktop 是第一个消费者，因为它能最早暴露 Rust 依赖泄漏。

1. 将 `uc-engine` 改为精确 Git commit 依赖。
2. 更新 `Cargo.lock`，记录对应 `core-v*`。
3. 删除本地已迁出的 crate、迁移、绑定脚本和重复检查。
4. 保留 desktop 自己的系统宿主、daemon、Web、CLI 和 Tauri。
5. LAN 需要时显式启用兼容 feature，不得成为默认或回退。
6. 在同一个迁移 PR 中完成依赖切换和旧源码删除。

**验收**：

- 全新检出 desktop 能只通过远程固定核心提交构建。
- workspace 中不存在已迁出 crate 的本地副本。
- daemon、CLI、Tauri 和 HTTP/WS 真实流程通过。
- 依赖扫描只发现一个核心提交。
- P2P 失败不会产生 LAN 请求。

**回退**：在没有不兼容持久化写入时，把 `uc-engine` 固定回上一已知提交；不恢复已删除的本地源码。

**完成进度（2026-07-24）**：desktop 的全部核心引用已集中固定到 `UniClipboard/core` 提交 `dcdccb234f020be49884bf92d886f25a0f192188`，锁文件只解析该单一来源。11 个迁出包、数据库迁移、移动绑定、验收宿主和旧移动发布工作流已从 desktop 删除；daemon、CLI、Web、Tauri、平台和宿主代码继续由 desktop 拥有。消费者检查会拒绝本地旧副本、浮动版本、内部运行依赖和自动 LAN 回退，`uc-webserver` 与 CLI 开发入口仍显式启用 LAN 兼容。完整工作区全目标编译、CLI 开发功能编译、daemon/CLI/Tauri 构建、格式和静态检查通过；隔离 portable profile 已实际完成空间初始化、daemon 启动、状态读取和停止。按用户要求没有重复运行测试用例。

### Phase 6：切换 Android 和 iOS

Android 先切，iOS 后切；两端必须消费同一个 RC Release。

1. 更新移动仓的产物下载脚本，按版本下载并校验 Release manifest。
2. Android 同时采纳 AAR、Kotlin 绑定和运行依赖。
3. iOS 同时采纳 XCFramework、Swift 绑定和 SwiftPM checksum。
4. 删除仓库内旧二进制和手工生成绑定，只保留版本与校验记录。
5. 保留各自 Keystore/Keychain、剪贴板、文件和生命周期实现。
6. 分别验证升级、暂停恢复和身份不变。

**验收**：正式 APK/IPA 只包含一个核心版本；产物来源提交一致；错误混用绑定时构建必须失败。

**回退**：重新固定上一 Release 的完整产物集合，不允许只回退绑定或只回退本地库。

### Phase 7：切换 HarmonyOS

1. 社区仓固定同一 RC Release 和校验值。
2. 删除 `rust/space-core` 等复制的核心源码。
3. 只保留系统适配、ArkTS 产品代码和 Release 产物组装。
4. 将可复用的 N-API 修改提交回核心仓，不在社区仓保留补丁副本。

**验收**：社区仓没有核心源码副本；完整 HAP 可从固定 Release 独立构建、安装、恢复身份并完成内容流程。

**回退**：固定上一完整 HarmonyOS 资产；不恢复复制源码。

### Phase 8：稳定发布与清理

1. 汇总 desktop、Android、iOS、HarmonyOS 对 RC 的验证记录。
2. 没有阻断问题时发布稳定 `core-v0.1.0`；有修复则继续 RC 序列。
3. 删除 desktop 中遗留的核心 CI、文档、脚本和路径例外。
4. 更新所有仓库的所有权说明、贡献规则和故障定位入口。
5. 解除迁移冻结，核心改动只在新仓进行。
6. 记录旧目录最后提交、新仓首个提交和消费者首次固定版本的映射。

**验收**：仓库搜索、Cargo metadata、移动包扫描和发布清单共同证明只有一个事实来源和一个生效核心版本。

## 回退原则

| 阶段 | 可用回退 | 禁止做法 |
| --- | --- | --- |
| 新仓创建前 | 取消冻结 | 提前复制源码到消费者 |
| RC 发布前 | 删除候选迁移分支 | 在两个仓同时修代码 |
| RC 发布后、消费者切换前 | 发布下一 RC | 覆盖已有 Release |
| 单个消费者切换后 | 固定回上一完整版本 | 运行时双核心、只回退一半绑定 |
| 不可逆数据迁移后 | 向前修复并发布新版本 | 降级到不能读取新数据的旧版本 |

任何回退都必须保持持久化密文规则，不得用导出明文或删除用户数据换取降级成功。

## 提交拆分

建议按以下意图独立提交或独立 PR：

1. desktop 依赖防火墙
2. 可移植观测与路径依赖收口
3. LAN 兼容 feature 和发布隔离
4. 自动化跨仓检查
5. 历史过滤后的核心仓初始提交
6. 核心仓 CI 和 RC 发布
7. desktop 固定版本并删除本地源码
8. Android/iOS 固定发布产物
9. HarmonyOS 固定发布产物并删除复制源码
10. 稳定发布与文档清理

不要把依赖收口、物理迁移、协议变化、数据迁移和消费者功能改动混进同一个提交。

## 完成标准

- [ ] `UniClipboardCore` 是核心源码、数据库迁移、绑定和发布的唯一事实来源。
- [ ] 新仓可在全新环境独立检出、检查、测试和生成四平台产物。
- [ ] desktop 正式代码只通过 `uc-engine` 使用核心，不依赖内部 crate。
- [ ] desktop、Android、iOS、HarmonyOS 均固定不可变核心版本和校验值。
- [ ] 各消费者源码树不存在可编辑的核心源码、生成绑定或二进制副本；缓存产物只由固定版本和校验值重新取得。
- [ ] 一个 `core-v*` Release 的全部产物来自同一提交并有完整 manifest。
- [ ] 数据库升级、身份恢复、密文持久化和四平台一致性门禁仍有效。
- [ ] P2P 与 LAN 可由用户明确选择，彼此独立且不存在自动回退。
- [ ] LAN 兼容路径使用独立 `uc-mobile-v*` 版本、工作流和发布清单。
- [ ] 旧仓与新仓的提交映射、冻结窗口和消费者切换记录可审计。

## 停止条件

- 核心仓仍需要 desktop/mobile/HarmonyOS 的本地路径、补丁或脚本。
- desktop 仍必须直接使用 `uc-core`、`uc-application`、`uc-infra` 或 `uc-mobile-proto`。
- 任一消费者只能依赖分支、浮动主干或可覆盖资产。
- 新仓需要长期保留 desktop 中的核心副本才能开发或发布。
- 迁移同时要求不兼容协议、身份或持久化格式升级。
- 同一应用包中出现两个核心版本或来源提交。
- LAN 需要读取 P2P 失败信号、接入未公开内部类型或复制 P2P 业务流程。
- 任何平台无法从同一提交生成可校验的绑定与本地库。

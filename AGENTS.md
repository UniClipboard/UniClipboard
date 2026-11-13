# Agent Instructions

unic：跨设备剪贴板同步。Go 核心（网络层 tailcat，存储 SQLite），桌面界面统一采用 mygo + 自定义 UI Kit，界面原型待定；后续纳入 Android、iOS。项目概览见 [README.md](README.md)。

## 仓库结构

monorepo：平台无关的 Go 核心在根（`engine/` 是公开 SDK，`internal/` 是实现），各端应用在 `app/<平台>/`，每个应用有自己的 `README.md` 与 `AGENTS.md`。依赖单向：应用依赖核心，核心不依赖应用。布局与依赖边界见[仓库总览](docs/architecture/overview.md)。

## 命令

| 任务 | 命令 |
|---|---|
| 测试（核心 + 桌面） | `go test -race ./...` |
| 构建桌面端 | `go build -o unic-desktop ./app/desktop/cmd/unic-desktop` |
| 构建无头版 | `make build-headless` |
| 第二个本地节点 | `UNIC_HOME=/tmp/unicA ./unicd` |
| 文档检查（链接、归档引用、架构页校验与过期） | `go run ./tools/docscheck`（`make docs-check`） |
| CLI 服务生命周期（临时 unicd 自动启停，真实多进程） | `make service-lifecycle-e2e`；回执见 [0021 计划](docs/exec/0021-cli-on-demand-service-lifecycle.md) |
| CLI 端到端（真实 unic/unicd 多进程） | `UNIC_E2E=1 go test ./e2e/cli -v`；用例清单见 [e2e/cli/CASES.md](e2e/cli/CASES.md) |

## 文档目录

文档分三类（生命周期与台账见 [docs/decisions/README.md](docs/decisions/README.md)）：

- **架构（活文档）** [`docs/architecture/`](docs/architecture/README.md)：系统现状、不变量、代码地图。动手前先读与任务相关的页面；它是唯一的叙述性事实源。
- 已落地的页面：[仓库布局与依赖边界](docs/architecture/overview.md)、[流水线与本机历史](docs/architecture/pipeline.md)、[设备身份、信任与配对](docs/architecture/identity-and-trust.md)。
- **契约** `docs/contracts/`：接口与协议的精确规格（见下）。
- **决策** `docs/decisions/`：只回答“为什么”。`archive/` 里的已落地决策只读，**不得引用**；`proposed/` 里是进行中的决策，尚未落地的主题暂时需要读它。

文档规则：

- 改了行为，就在同一次改动里更新对应的架构页或契约，并运行 `go run ./tools/docscheck -bump <页>` 更新 `verified`。
- 决策实现完成后必须“落地”再归档：结论合并进架构页/契约/本文件“约定”，再移入 `archive/`。流程用 `/maintain-docs`。
- 代码注释、README、各端 AGENTS.md 只链接架构页与契约，不链接 `archive/`。新决策写进 `docs/decisions/proposed/`，按编号命名并登记到台账。

### 进行中的决策（`docs/decisions/proposed/`，落地后移出本表）

| 编号 | 主题 | 摘要 |
|---|---|---|
| [0002](docs/decisions/proposed/0002-config-file.md) | 配置文件：TOML + 保存即生效 | `$UNIC_HOME/config.toml`，监听目录+防抖热加载；坏配置保留上一份有效值；`unic config` 查看；`inbox` 需重启 |
| [0003](docs/decisions/proposed/0003-engine-sdk.md) | 独立 Engine：统一接口与契约 | 公开 Go SDK，宿主负责剪贴板与系统集成；定义生命周期、内容所有权、事件与错误；先落地契约并迁移桌面，移动端验证后续进行 |
| [0004](docs/decisions/proposed/0004-clipboard-pipeline-lua.md) | 剪贴板处理流水线 + Go / Lua 插件 | 已采纳并实施（GitHub 安装、真机基线未完成）；Capture→Processing→Commit→Sync→Use→End；captured/received 可干预（仅标注，不改正文；改写内容走用户触发的 Action），其余事件只读；同步支持 builtin/外部 Go 插件，默认 builtin.tailcat 位于 `internal/plugins/tailcat/`，随宿主编译；设备身份/信任/关联按 0008 统一管理，无旧 tailcat 兼容；Go 常驻进程/RPC（预编译优先，--build 兜底）+ 增强 gopher-lua 沙箱（`internal/luavm`，预算/白名单已实现）；统一接口与模板见插件契约；本地历史库，标签/过期不同步；历史归 Engine 管理，历史与流水线业务实现放 `engine/` |
| [0007](docs/decisions/proposed/0007-multiple-sync-backends.md) | 多同步后端与分层策略 | 已采纳并实现；全局开关、实例启用、每后端类型/自动文件阈值；多后端结果/资源隔离与有界跨后端去重；默认只交付 tailcat，WebDAV 对象协议另定 |
| [0009](docs/decisions/proposed/0009-external-channels.md) | 外部渠道：无 unic 身份证明的第三方对端 | 草案；不是设备、不写 `devices` 表、无指纹；目标为组；与设备混排展示；默认自动写剪贴板并标 `external:` 来源；移除仅本地生效，后端声明 `remote_revocation`；接入形态待定（首个对象微信输入法）；数据模型与接收路径已实现（`engine/channels.go`），桌面 `/v1/channels` |
| [0010](docs/decisions/proposed/0010-plugin-settings-schema.md) | 插件设置 Schema 与表单生成 | JSON Schema 子集 + UI 提示；Go 配置定义生成静态 schema，各 shell 生成原生表单；独立工具和 macOS 会话预览已实现，配置 API/清单/RPC/安装接入待完成 |
| [0011](docs/decisions/proposed/0011-v1-history-migration.md) | v1 本机历史迁移 | `unic migrate` 子命令（无独立程序），随 `unic` 覆盖三平台；只读解锁旧数据，离线导入文本/图片/文件/标签；支持新目录与追加合并、幂等续跑和重开校验；验收见执行计划 |
| [0013](docs/decisions/proposed/0013-mygo-uikit.md) | 桌面 UI Kit：移植到 mygo | 统一 Kit API，Adwaita / macOS glass / Fluent 三皮肤；token 从 default.json 生成；59 个公共组件与 GUI 已接入，交互和视觉分别验证；各平台原生验收边界见执行记录 |
| [0014](docs/decisions/proposed/0014-client-observability-otlp.md) | 客户端可观测性：OTLP 与动态日志等级 | 已采纳；本次 logs + metrics 已接入工作区，traces 后续实施；验收与限制见执行计划；本机/远程等级独立，有界非阻塞、秘密白名单；current upload defaults are defined in the [observability architecture](docs/architecture/observability.md) |
| [0015](docs/decisions/proposed/0015-headless-cli-and-desktop-integration.md) | 纯 CLI、无头服务与独立桌面集成 | Runtime implemented; UI-free CLI/service, exclusive desktop adapter and separate distributions; native acceptance pending. See [execution receipt](docs/exec/0015-headless-cli-and-desktop-integration.md) |
| [0016](docs/decisions/proposed/0016-sync-and-file-settings.md) | Sync and file settings | Sync and quota CLI/TOML implemented; 100 MB default file limit; custom received files remain user-managed; separate file cleanup superseded by 0017 |
| [0017](docs/decisions/proposed/0017-history-auto-cleanup.md) | Automatic history cleanup | Unified age/count cleanup, tag exclusions, and managed-file/history deletion implemented; per-device cache quota retained; separate file-cleanup settings removed; 16 CLI E2E cases passed; native visual acceptance pending |
| [0018](docs/decisions/proposed/0018-independent-product-build-and-release.md) | Independent product builds and releases | Android, iOS, CLI and Desktop own independent versions and separate build/release workflows; immutable artifacts and per-target channels. CLI and Desktop construction/delivery controllers are implemented; native/sandbox evidence is recorded. Production activation and mobile delivery remain pending. See [release architecture](docs/architecture/releases.md) and [execution plan](docs/exec/0018-independent-product-build-and-release.md). |
| [0020](docs/decisions/proposed/0020-tailcat-sync-observability.md) | Tailcat sync observability | Connection/session/recovery metrics, payload measurements and remote-content value are implemented; incoming real-stack direct/DERP accepted. Public Client status is blocked upstream; remaining acceptance is recorded. Actual paste tracking is excluded; see [execution plan](docs/exec/0020-tailcat-sync-observability.md). |
| [0023](docs/decisions/proposed/0023-cli-lifecycle-acceptance-followup.md) | CLI lifecycle acceptance follow-up | Draft: other-platform process runs (C36), file release barrier after send deadline, C03 race investigation |
| [0024](docs/decisions/proposed/0024-directory-copy-and-send.md) | Directory copy and explicit send | Implemented; local process acceptance passed; native/real-device acceptance pending. One-shot directory transfer; automatic sync defaults off; explicit send prepares snapshots; mixed selections follow directory policy. |
| [0025](docs/decisions/proposed/0025-desktop-package-identity-and-replacement.md) | Desktop package identity and replacement | Accepted; same-channel v2 replaces v1, alpha/stable share installation identity, development stays isolated, bundled CLI files are private, package managers own updates; runtime and construction implemented, production activation and real v1 replacement acceptance pending. |

### 接口契约（`docs/contracts/`）

- [Directory content contract](docs/contracts/directory-contract.md): deferred directory references, explicit snapshots, portable manifests, file-policy grouping, resource bounds, receive ownership and platform acceptance limits.

- [设备与后端关联契约](docs/contracts/device-contract.md)：统一设备、身份验证、候选/配对、实例关联审批、设备列表与全路径撤销；已实现并有测试；对象存储路径与 macOS 界面未做，密码学协议见 [identity-protocol](docs/contracts/identity-protocol.md)。

- [Engine 公共契约](docs/contracts/engine-contract.md)：Engine 接口、DTO、生命周期、信任、内容租约、发送与应用语义、事件恢复及错误码。修改核心、桌面适配或未来平台绑定前必须阅读，并同步维护契约与测试。

- [History search contract](docs/contracts/history-search-contract.md): Engine-owned text/file search, typed filters, explicit previews, catalogue paging, exact counts and resource bounds.

- [本机历史契约](docs/contracts/history-contract.md)：稳定历史 ID、查询分页、内容租约、更新/删除、过期与配额、接收列表映射及历史应用；已实现（Engine 历史 API、桌面 `/v1/history`、`unic history`）。

- [流水线提交契约](docs/contracts/pipeline-contract.md)：提交结果与网络结果分离、请求幂等、旧 SubmitContent 门面、三类入口状态转换、提交屏障、Received/去重发布与资源账本；已实现并有测试。

- [Action 契约](docs/contracts/clipboard-action-contract.md)：插件声明、用户主动触发的内容变换（ListActions/RunAction），结果为 origin=action 的新历史条目并经 BeginApplyHistory 应用；取代已取消的自动写回；Engine 侧、`builtin.text` 与桌面 `/v1/actions` 已实现；外部插件的 Action 随插件加载；GUI Action 菜单随待定界面原型规划。

- [插件契约](docs/contracts/plugin-contract.md)：Go/Lua 标识、注册、生命周期、配置、顺序、上下文与最小模板、GitHub 仓库安装；builtin.tailcat 后端边界。Go/Lua 注册、外部进程、builtin.tailcat、桌面配置与本地 Lua 目录（`unic plugin list/enable/disable`）已实现；§8 GitHub 安装未实现。

- [插件设置 Schema 契约](docs/contracts/plugin-settings-contract.md)：字段约束、控件映射、Go 生成、校验/默认值与秘密字段；独立工具和固定 schema 的 macOS 表单已实现，运行时集成待完成。

- [插件 RPC 核心协议](docs/contracts/plugin-rpc-contract.md)：二进制帧、握手、核心方法、取消、patch、流控和机器可读 Schema/互操作向量；peer_sessions 后端方法已实现并经真实子进程端到端验证；后端业务 DTO 见下项。

- [同步后端业务契约](docs/contracts/sync-backend-contract.md)：状态快照/撤销屏障、连接审批、短期传输授权、接收准入与完成 DTO；peer_sessions 已实现并有端到端测试；对象存储 DTO 未做。

- [后端迁移契约](docs/contracts/backend-migration-contract.md)：全新设备/传输身份初始化、私有目录/锁、恢复边界、Network 移除（不兼容旧身份/地址）、显式程序装配与测试入口；已实现；真机双设备实测未做。

- [Lua 运行时契约](docs/contracts/lua-runtime-contract.md)：固定上游版本、取消实测、增强 VM 方案、白名单、资源预算与失败状态；四项上游探针通过，增强 VM、沙箱与 Engine 接入已实现（`internal/luavm`、`internal/luaplugin`），差距见契约状态行。

- [SDK 扩展类型契约](docs/contracts/sdk-extension-contract.md)：历史/提交/Action DTO、插件及 Lua 装配、可选值/零值、严格配置和错误映射；已落在 `engine/` 与 `plugin/`。

- [可观测性契约](docs/contracts/observability-contract.md)：本次 logs + metrics 的 Engine 诊断接口、阶段统计、指标目录、日志白名单、配置、等级覆盖、socket API、停用屏障与资源上限；已接入工作区，验收范围见 [0014 计划](docs/exec/0014-client-observability-otlp.md)。Traces 留待后续实施。

### 原型

- [`prototype/gui.html`](prototype/gui.html)：GUI 交互原型（浏览器直接打开）。

## 约定

- Desktop uses mygo and the existing UI Kit demo composition. The normal entry opens history directly; setup and passphrase-unlock flows are removed. See [live desktop composition](docs/architecture/overview.md#live-desktop-composition).
- 核心不得依赖 `app/*`；宿主专属代码留在对应应用，公开 Engine 的传递依赖边界由 `engine/boundary_test.go` 检查。
- 设备地址与配对票据都是秘密：不要写进日志、事件或错误信息。
- 通过配对码进来的设备永远需要用户批准；不要为了方便绕过。
- 项目尚未发布任何版本：线路协议、数据库结构、配置和 API 都不需要考虑向后兼容。需要改就直接改，不写迁移、兼容分支或旧版本回退；协议版本号保持为 1。

- Release programs: `unic` and `unicd` must have no transitive UI dependency. `unic-desktop` owns graphical integration. See [headless architecture](docs/architecture/headless.md).

- Client telemetry defaults enabled. The config.toml master and per-signal switches can disable it. Ship only the ingestion-only project token; analytics uses an independent random profile identifier. See the [observability contract](docs/contracts/observability-contract.md).

- Mobile presentation lives in `app/expo` as one shared Expo application. The sibling `app/ios` and `app/android` directories contain platform guidance only. Engine integration is outside the UI-shell migration. See the [Expo shell](app/expo/README.md).

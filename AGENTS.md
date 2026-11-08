# Agent Instructions

unic：跨设备剪贴板同步。Go 核心（网络层 tailcat，存储 SQLite），桌面界面 mygo，后续纳入 Android、iOS。项目概览见 [README.md](README.md)。

## 仓库结构

monorepo：平台无关的 Go 核心在根（`engine/` 是公开 SDK，`internal/` 是实现），各端应用在 `app/<平台>/`，每个应用有自己的 `README.md` 与 `AGENTS.md`。依赖单向：应用依赖核心，核心不依赖应用。布局与理由见 [0005](docs/design/decisions/0005-monorepo-layout.md)。

## 命令

| 任务 | 命令 |
|---|---|
| 测试（核心 + 桌面） | `go test -race ./...` |
| 构建桌面端 | `go build -o unic ./app/desktop/cmd/unic && go build -o unicd ./app/desktop/cmd/unicd` |
| 第二个本地节点 | `UNIC_HOME=/tmp/unicA ./unicd` |

## 文档目录

动手前先读与任务相关的设计决策；新的决策写进 `docs/design/decisions/`，按编号命名，并在下面登记。

### 设计决策（`docs/design/decisions/`）

| 编号 | 主题 | 摘要 |
|---|---|---|
| [0001](docs/design/decisions/0001-pairing-code.md) | 添加设备：6 位配对码 + 完整地址 | 复用 rendezvous `/v1/web-pairings`，5 分钟一次性；票据 kind `unic-pair`；配对码只找地址不授信，需批准并核对指纹；仅桌面端 |
| [0002](docs/design/decisions/0002-config-file.md) | 配置文件：TOML + 保存即生效 | `$UNIC_HOME/config.toml`，监听目录+防抖热加载；坏配置保留上一份有效值；`unic config` 查看；`inbox` 需重启 |
| [0003](docs/design/decisions/0003-engine-sdk.md) | 独立 Engine：统一接口与契约 | 公开 Go SDK，宿主负责剪贴板与系统集成；定义生命周期、内容所有权、事件与错误；先落地契约并迁移桌面，移动端验证后续进行 |
| [0004](docs/design/decisions/0004-clipboard-pipeline-lua.md) | 剪贴板处理流水线 + Go / Lua 插件 | 草案；Capture→Processing→Commit→Sync→Use→End；captured/received 可干预（仅标注，不改正文；改写内容走用户触发的 Action），其余事件只读；同步支持 builtin/外部 Go 插件，默认 builtin.tailcat 位于 `internal/plugins/builtintailcat/`，随宿主编译；设备身份/信任/关联按 0008 统一管理，无旧 tailcat 兼容；Go 常驻进程/RPC（预编译优先，--build 兜底）+ 增强 gopher-lua 沙箱（`internal/luavm`，预算/白名单已实现）；统一接口与模板见插件契约；本地历史库，标签/过期不同步；历史归 Engine 管理，历史与流水线业务实现放 `engine/` |
| [0005](docs/design/decisions/0005-monorepo-layout.md) | 仓库布局：monorepo | 核心在根（`engine/`、`internal/`），应用在 `app/<平台>/`；依赖单向，桌面专属包放 `app/desktop/internal`；单一根模块 `github.com/uniclipboard/unic`，gomobile 引入时再拆 `bindings/` |
| [0006](docs/design/decisions/0006-macos-swiftui-client.md) | macOS 端：SwiftUI 客户端 | 草案；SwiftUI 作为 `unicd` 的客户端（unix socket，HTTP+JSON/SSE），不内嵌 Engine；`unicd` 增加无托盘模式；新增本机历史 API（事件不带正文）；历史与流水线由 `engine/` 管理；Developer ID 分发，不上 App Store |
| [0007](docs/design/decisions/0007-multiple-sync-backends.md) | 多同步后端与分层策略 | 已采纳，待实现；全局开关、实例启用、每后端类型/自动文件阈值；多后端结果/资源隔离与有界跨后端去重；默认只交付 tailcat，WebDAV 对象协议另定 |
| [0008](docs/design/decisions/0008-device-identity-and-backend-bindings.md) | 统一设备身份与后端关联 | 已采纳，待实现；Engine 统一身份/信任/审批；设备按后端实例独立授权和状态；无旧 tailcat 兼容；身份协议与 SDK/RPC 定义待补齐 |
| [0009](docs/design/decisions/0009-external-channels.md) | 外部渠道：无 unic 身份证明的第三方对端 | 草案；不是设备、不写 `devices` 表、无指纹；目标为组；与设备混排展示；默认自动写剪贴板并标 `external:` 来源；移除仅本地生效，后端声明 `remote_revocation`；接入形态待定（首个对象微信输入法）；数据模型与接收路径已实现（`engine/channels.go`），桌面 `/v1/channels` |
| [0010](docs/design/decisions/0010-plugin-settings-schema.md) | 插件设置 Schema 与表单生成 | JSON Schema 子集 + UI 提示；Go 配置定义生成静态 schema，各 shell 生成原生表单；独立工具和 macOS 会话预览已实现，配置 API/清单/RPC/安装接入待完成 |
| [0011](docs/design/decisions/0011-v1-history-migration.md) | v1 本机历史迁移 | 独立 Go CLI，三平台 arm64/amd64；只读解锁旧数据，离线导入文本/图片/文件/标签；支持新目录与追加合并、幂等续跑和重开校验；验收见执行计划 |

### 接口契约

- [设备与后端关联契约](docs/design/device-contract.md)：统一设备、身份验证、候选/配对、实例关联审批、设备列表与全路径撤销；行为已确定，身份协议及具体 DTO 待定义。

- [Engine 公共契约](docs/design/engine-contract.md)：Engine 接口、DTO、生命周期、信任、内容租约、发送与应用语义、事件恢复及错误码。修改核心、桌面适配或未来平台绑定前必须阅读，并同步维护契约与测试。

- [本机历史契约](docs/design/history-contract.md)：稳定历史 ID、查询分页、内容租约、更新/删除、过期与配额、接收列表映射及历史应用；契约已确定，待实现。

- [流水线提交契约](docs/design/pipeline-contract.md)：提交结果与网络结果分离、请求幂等、旧 SubmitContent 门面、三类入口状态转换、提交屏障、Received/去重发布与资源账本；契约已确定，实现待完成。

- [Action 契约](docs/design/clipboard-action-contract.md)：插件声明、用户主动触发的内容变换（ListActions/RunAction），结果为 origin=action 的新历史条目并经 BeginApplyHistory 应用；取代已取消的自动写回；契约已确定，待实现。

- [插件契约](docs/design/plugin-contract.md)：Go/Lua 标识、注册、生命周期、配置、顺序、上下文与最小模板、GitHub 仓库安装；builtin.tailcat 后端边界（目标设计，待实现）。

- [插件设置 Schema 契约](docs/design/plugin-settings-contract.md)：字段约束、控件映射、Go 生成、校验/默认值与秘密字段；独立工具和固定 schema 的 macOS 表单已实现，运行时集成待完成。

- [插件 RPC 核心协议](docs/design/plugin-rpc-contract.md)：二进制帧、握手、核心方法、取消、patch、流控和机器可读 Schema/互操作向量；后端业务 DTO 见下项，实现与互操作仍待完成。

- [同步后端业务契约](docs/design/sync-backend-contract.md)：状态快照/撤销屏障、连接审批、短期传输授权、接收准入与完成 DTO；业务协议已定义，实现待完成。

- [后端迁移契约](docs/design/backend-migration-contract.md)：全新设备/传输身份初始化、私有目录/锁、恢复边界、Network 移除（不兼容旧身份/地址）、显式程序装配与测试入口；方案已确定，实现待完成。

- [Lua 运行时契约](docs/design/lua-runtime-contract.md)：固定上游版本、取消实测、增强 VM 方案、白名单、资源预算与失败状态；四项上游探针通过，增强 VM、沙箱与 Engine 接入已实现（`internal/luavm`、`internal/luaplugin`），差距见契约状态行。

- [SDK 扩展类型契约](docs/design/sdk-extension-contract.md)：历史/提交/Action DTO、插件及 Lua 装配、可选值/零值、严格配置和错误映射；目标定义完成，尚未加入公开 Go 包。

### 原型

- [`prototype/gui.html`](prototype/gui.html)：GUI 交互原型（浏览器直接打开）。

## 约定

- 设备地址与配对票据都是秘密：不要写进日志、事件或错误信息。
- 通过配对码进来的设备永远需要用户批准；不要为了方便绕过。
- 项目尚未发布任何版本：线路协议、数据库结构、配置和 API 都不需要考虑向后兼容。需要改就直接改，不写迁移、兼容分支或旧版本回退；协议版本号保持为 1。

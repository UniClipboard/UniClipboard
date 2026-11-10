# 设计决策与文档生命周期

文档分三类，职责不重叠：

| 类别 | 位置 | 回答 | 性质 |
|---|---|---|---|
| 架构（活文档） | [`docs/architecture/`](../architecture/README.md) | 系统现在是什么样、有哪些不变量、代码在哪 | 唯一事实源，随代码维护 |
| 契约 | [`docs/contracts/`](../contracts/) | 接口、DTO、协议的精确规格 | 规范性，随代码维护 |
| 决策 | `docs/decisions/` | 当时为什么这么定 | `proposed/` 进行中；`archive/` 只读历史 |

**代码、`AGENTS.md`、README、契约、各端文档只引用架构与契约，不引用 `archive/` 里的决策**（`docs/decisions/` 内的决策之间可以互相引用）（`go run ./tools/docscheck` 会拦截）。后来者读完架构页就能动手，决策只在考古时才读。

## 生命周期

1. **提案**：新决策写在 `proposed/`，编号递增，状态“草案”或“已采纳，待实现”，并登记到下面的台账。
2. **实施**：进度记在 `docs/exec/`，不写进决策。
3. **落地（关口）**：实现完成并有验证后，把结论合并进活文档——现状与不变量写入 `docs/architecture/<主题>.md`，接口细节写入 `docs/contracts/`，全局硬规则写入 `AGENTS.md` 的“约定”。对照代码核实，不照抄决策。
4. **归档**：决策移到 `archive/`，状态改为“已归档”，补一行 `- 落地：` 链到上一步的活文档；被推翻或取消的部分在开头标明。归档后只读。
5. **推翻**：行为变了就直接改活文档；只有取舍被推翻时才新开决策，新决策落地时把旧决策标为 superseded。

没有落地到活文档的决策不得归档。由 agent 操作这套流程的方法见 `.claude/skills/maintain-docs/`（`/maintain-docs`）。

## 台账

“目标页”是落地后承接它的活文档（`—` 表示尚未规划）。状态：`proposed` 未落地；`archive` 已落地归档。

| 编号 | 决策 | 状态 | 目标页 |
|---|---|---|---|
| 0001 | [配对码](archive/0001-pairing-code.md) | archive | [identity-and-trust](../architecture/identity-and-trust.md) |
| 0002 | [配置文件](proposed/0002-config-file.md) | proposed | desktop-runtime |
| 0003 | [Engine SDK](proposed/0003-engine-sdk.md) | proposed | engine |
| 0004 | [剪贴板流水线 + 插件](proposed/0004-clipboard-pipeline-lua.md) | proposed（流水线与历史已写入 [pipeline](../architecture/pipeline.md)；插件页与 GitHub 安装未完成，完成后归档） | pipeline、plugins |
| 0005 | [monorepo 布局](archive/0005-monorepo-layout.md) | archive | [overview](../architecture/overview.md) |
| 0006 | [macOS SwiftUI 客户端](archive/0006-macos-swiftui-client.md) | archive（已取消；界面选型由 0013 承接，原型待定） | [overview](../architecture/overview.md) |
| 0007 | [多同步后端](proposed/0007-multiple-sync-backends.md) | proposed | sync-backends |
| 0008 | [设备身份与后端关联](archive/0008-device-identity-and-backend-bindings.md) | archive | [identity-and-trust](../architecture/identity-and-trust.md) |
| 0009 | [外部渠道](proposed/0009-external-channels.md) | proposed（草案） | sync-backends |
| 0010 | [插件设置 Schema](proposed/0010-plugin-settings-schema.md) | proposed | plugins |
| 0011 | [v1 历史迁移](proposed/0011-v1-history-migration.md) | proposed | — |
| 0012 | [设备介绍](archive/0012-device-introductions.md) | archive | [identity-and-trust](../architecture/identity-and-trust.md) |
| 0013 | [桌面 UI Kit](proposed/0013-mygo-uikit.md) | proposed | desktop-ui |
| 0014 | [OTLP 可观测性](proposed/0014-client-observability-otlp.md) | proposed（logs + metrics 已接入工作区，验收见[执行计划](../exec/0014-client-observability-otlp.md)；traces 后续） | observability |
| 0015 | [纯 CLI 与桌面集成](proposed/0015-headless-cli-and-desktop-integration.md) | proposed (runtime implemented; native acceptance pending) | [headless](../architecture/headless.md) |
| 0016 | [Sync and file settings](proposed/0016-sync-and-file-settings.md) | proposed (sync/quota implemented; separate file cleanup superseded by 0017) | [desktop-runtime](../architecture/desktop-runtime.md), pipeline |
| 0017 | [Automatic history cleanup](proposed/0017-history-auto-cleanup.md) | proposed (implemented; CLI accepted; native visual acceptance pending) | [pipeline](../architecture/pipeline.md), [history contract](../contracts/history-contract.md), [desktop-runtime](../architecture/desktop-runtime.md) |
| 0018 | [Independent product builds and releases](proposed/0018-independent-product-build-and-release.md) | proposed (accepted design; implementation pending) | releases, release-contract |
| 0019 | [PostHog Cloud integration](proposed/0019-posthog-integration.md) | proposed (local implementation; default-on uploads and config.toml controls; hosted project/region acceptance pending) | observability, observability-contract |

# UniClipboard 贡献指南

[English](./CONTRIBUTING.md) | 简体中文

感谢你愿意为 UniClipboard 做贡献！本文档说明如何搭建开发环境、我们遵循的工作流，以及对贡献内容的约定。

## 目录

- [行为准则](#行为准则)
- [贡献方式](#贡献方式)
- [报告 Bug](#报告-bug)
- [建议新功能](#建议新功能)
- [报告安全问题](#报告安全问题)
- [开发环境搭建](#开发环境搭建)
- [项目结构](#项目结构)
- [开发流程](#开发流程)
- [分支策略](#分支策略)
- [Commit 规范](#commit-规范)
- [代码风格与质量](#代码风格与质量)
- [测试](#测试)
- [文档](#文档)
- [Pull Request](#pull-request)
- [发布流程](#发布流程)
- [许可证](#许可证)

## 行为准则

请保持尊重、建设性与耐心。我们希望所有在 issue、PR、讨论中互动的人都遵守开源社区的基本礼仪：默认对方善意，聚焦在技术问题上，让新人感到被欢迎。

## 贡献方式

无论经验深浅，都有很多参与方式：

- **报告 Bug**：附带清晰的复现步骤。
- **提出功能建议**：与项目"隐私优先、跨设备"的方向契合即可。
- **修复 Bug**：可以从带有 `good first issue` 或 `help wanted` 标签的 issue 入手。
- **改进文档**：错别字、表述不清、缺失的搭建步骤都欢迎修正。
- **补充测试**：Rust 与 TypeScript 两侧的测试覆盖率提升都很受欢迎。
- **翻译界面**：项目使用 `i18next`，欢迎补充新语种。
- **审阅 PR**：一个用心的第二双眼睛非常有价值。

## 报告 Bug

提 issue 之前请先：

1. 在 [现有 issue](https://github.com/UniClipboard/UniClipboard/issues) 中搜索，避免重复。
2. 确认该 Bug 在最新发布版本上仍能复现。

提 Bug 时请附带：

- **环境信息**：操作系统及版本、UniClipboard 版本、安装方式（DMG、AppImage、MSI、Homebrew、源码构建）。
- **复现步骤**：简短、确定性、按编号列出。
- **预期行为 vs 实际行为**。
- **日志**：从下列日志目录摘取相关片段：
  - macOS：`~/Library/Application Support/app.uniclipboard.desktop[-<profile>]/logs/`
  - Linux：`~/.local/share/app.uniclipboard.desktop[-<profile>]/logs/`
  - Windows：`%LOCALAPPDATA%\app.uniclipboard.desktop[-<profile>]\logs\`
- **截图或录屏**：UI 问题尤其需要。

发布前请清除日志和剪贴板内容中的个人数据。

## 建议新功能

提 issue 时请说明：

- 你想解决的 **用户层面** 问题（不是实现细节）。
- 现有功能为什么不够。
- 该功能是否与项目原则一致：隐私优先、端到端加密、不强制依赖云账户。

破坏安全模型的提案（例如允许服务器获取明文）不会被接受。

## 报告安全问题

**请勿** 通过公开 GitHub issue 报告安全漏洞。

请按照 [`SECURITY.md`](./SECURITY.md) 中的披露流程联系我们。我们会认真对待与加密、隐私相关的报告，并与你一起协调修复与发布的时间线。

## 开发环境搭建

### 前置依赖

- **Rust**：稳定版工具链（推荐使用 `rustup`）。如果仓库中存在 `rust-toolchain.toml`，会以其为准。
- **Bun**：JavaScript 包管理器与运行时。可从 [bun.sh](https://bun.sh) 下载。
- **Tauri 构建依赖**：参考 Tauri 官方 [前置依赖文档](https://tauri.app/start/prerequisites/)，按系统准备好相应组件（Windows 的 WebView2、Linux 的 `webkit2gtk` 等、macOS 的 Xcode CLT）。

可选但有用：

- `cargo-llvm-cov`：用于生成 Rust 覆盖率报告。
- `cargo sweep`：开发期间清理 `target/` 目录。

### 克隆与安装

```bash
# `--recurse-submodules` 会同步拉取 `src-tauri/vendor/iroh-blobs/`
# 下的 iroh-blobs fork，缺这个 `cargo build` 会失败。
git clone --recurse-submodules https://github.com/UniClipboard/UniClipboard.git
cd UniClipboard
bun install
```

如果克隆时漏了 `--recurse-submodules`：

```bash
git submodule update --init --recursive
```

`bun install` 会通过 `prepare` 脚本自动安装 Husky 钩子，`git commit` 时会自动跑 lint-staged 检查。

### 以开发模式运行桌面端

```bash
# 单实例，dev profile（数据目录在 app.uniclipboard.desktop-dev 下）
bun tauri:dev
```

如果要在本机调试 P2P 同步，可以同时运行两个相互隔离的实例：

```bash
# 并行运行两个 peer，peerA 是完整剪贴板模式，peerB 是被动模式
bun tauri:dev:dual

# 或者分别启动，便于挂调试器
bun tauri:dev:peerA
bun tauri:dev:peerB
```

两个 peer 使用不同的 `UC_PROFILE`，所以它们的数据、密钥库、日志互不冲突。

### 构建发行包

```bash
bun tauri build
```

产物会出现在 `src-tauri/target/release/bundle/`。

### 发布期 Telemetry Secrets

Release 构建可以通过 `option_env!` 在编译期把 telemetry 凭证烤进 binary。
**没有任何 secret 是必需的**——缺失时对应通道走 noop sink，应用照常启动。

| Secret                | 通道                                       | 编译期读取                                          | CI workflow 注入位置                            |
| --------------------- | ------------------------------------------ | --------------------------------------------------- | ----------------------------------------------- |
| `SENTRY_DSN`          | 后端 Sentry（错误 / breadcrumb）           | `uc-bootstrap/src/tracing.rs` — `option_env!`        | `.github/workflows/{build,alpha-build}.yml`     |
| `VITE_SENTRY_DSN`     | 前端 Sentry（必须是独立 Sentry 项目）      | `import.meta.env.VITE_SENTRY_DSN`（Vite 构建期）     | 同上                                            |
| `POSTHOG_PROJECT_KEY` | 产品 analytics（PostHog Cloud，US region） | `uc-bootstrap/src/analytics.rs` — `option_env!`      | 同上（issue #549 落地时同位添加）               |

本地 dev 构建：在 shell 里 export 对应变量即可 opt-in；否则 dev profile
analytics 走 stdout sink，Sentry 通道继续用 `cfg(dev)` 编译期 DSN。
PostHog 注入契约与缺 key 时的降级语义详见
[`docs/architecture/telemetry-events.md`](./docs/architecture/telemetry-events.md) §10.1。

空字符串等价于"未设置"——CI secret 未配置时 `${{ secrets.X }}` 渲染为空，
对应 sink 静默退化到 noop。**任何情况下不要把这几个值提交到仓库或写进
issue / PR 正文。**

## 项目结构

```text
.
├── src/                # React + TypeScript 前端（Tauri webview）
├── src-tauri/          # Rust 工作区（daemon、app、core、infra、platform 等 crate）
├── workers/            # Cloudflare Worker，加密中继
├── docs/               # 架构、agent 规则、发布流程、UAT 等
├── scripts/            # 开发/发布脚本（如 bump-version.js）
├── public/             # Vite 提供的静态资源
├── assets/             # 营销/图标素材
├── AGENTS.md           # 仓库说明的根导航索引
└── README.md           # 面向用户的项目介绍
```

`AGENTS.md` 是仓库约定的入口文档。在某个具体方向工作时，按它的指引读对应的专题文档（前端、Rust/Tauri、架构、工作流、项目记忆），不要把所有文档一次性全读一遍。

## 开发流程

非简单改动需要遵循结构化的处理方式。完整规则见 [`docs/agent/workflow-rules.md`](./docs/agent/workflow-rules.md)，要点是：

- **修复根因，而不是症状**。不允许"先这样处理一下"式的临时补丁掩盖结构问题。
- **保持单一事实来源**。同一业务规则不在多个模块重复实现。
- **不允许新旧逻辑长期并存**，必须有明确的迁移/删除计划。
- **遵守架构边界**。Rust 工作区采用六边形分层：`uc-app → uc-core ← uc-infra / uc-platform`。`uc-core` 不得依赖 infra 或 platform crate。详见 [`docs/agent/architecture-rules.md`](./docs/agent/architecture-rules.md) 与 [`docs/architecture/ports.md`](./docs/architecture/ports.md)。

如果你不确定某个变更是局部修复还是需要做结构调整，建议先开 issue 或 draft PR 与维护者讨论，再投入大块时间重构。

## 分支策略

项目使用以 `main` 为锚的 **trunk-based 工作流**：

- **`main`**：主干（trunk）。所有改动通过 PR 进入这里。`main` 必须始终保持可构建、可发布。
- **`release/vX.Y.Z[-channel.N]`**：发版（alpha / beta / rc / stable）时由 `prepare-release` workflow 从 `main` 切出。release PR 合回 `main` 后会自动打 tag 并构建产物，release 分支随后自动删除。
- **功能分支**：从 `main` 切出，命名要有描述性（如 `feat/quick-panel-search`、`fix/devices-online-state`）。

提 PR 时默认目标分支是 `main`，除非维护者明确要求你目标到某个 release 分支。

未完成的工作不要合入。功能没准备好就留在 feature 分支上（或加运行时 gate），不要把半成品合进 `main`。

## Commit 规范

每个 commit 必须只表达 **一个** 工程意图。完整规则见 [`docs/agent/architecture-rules.md`](./docs/agent/architecture-rules.md#atomic-commit-rule)。

### 允许的 commit 类型

| 类型        | 适用场景                                        |
| ----------- | ---------------------------------------------- |
| `feat:`     | 新增的用户可见能力                              |
| `impl:`     | 已规划功能的具体实现步骤                        |
| `fix:`      | Bug 修复                                       |
| `hotfix:`   | 紧急的生产环境修复                              |
| `refactor:` | 不改变行为的结构调整                            |
| `arch:`     | 架构或边界变更（如新增一个 port）              |
| `chore:`    | 工具、构建、依赖、脚本                          |
| `infra:`    | 部署或环境配置                                  |
| `test:`     | 新增或调整测试                                  |
| `perf:`     | 性能优化（需附带 benchmark）                    |
| `docs:`     | 仅文档变更                                      |

### 格式

```text
<type>(<可选 scope>): <单一意图概述>

[可选正文：解释"为什么"，而不是"做了什么"]
```

来自项目历史的真实示例：

```text
fix(storage): isolate cache_dir from data root on Windows
fix(devices): show real online state and cut offline detection latency
chore(observability): silence swarm_discovery::socket EHOSTUNREACH spam
```

> commit 信息、PR 标题与描述使用英文，确保工具链与外部协作者通用。项目文档使用中文，代码注释使用英文（详细范围与豁免目录见下方"代码风格与质量"小节）。

### 应避免的写法

- 把功能改动和格式化清理混在同一个 commit 里。
- commit 信息需要用 `and`、`also`、`plus`、`misc` 才能概括 —— 这种就要拆分。
- 同一个 commit 中既改了 port 定义又改了 adapter 实现 —— 拆开，让 port 先落地。
- 仅靠后续 commit 才能"补全"的 commit。每个 commit 应单独可构建、可运行。

如果你本地积累了较多变更，可以借助 `atomic-commits` 之类的工作流把它们重新整理成单一意图的干净 commit 后再推送。

## 代码风格与质量

### Lint 与格式化

JavaScript / TypeScript：

```bash
bun run lint        # eslint
bun run lint:fix    # eslint --fix
bun run format      # prettier --write .
```

Rust（在 `src-tauri/` 目录下执行）：

```bash
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
```

通过 Husky 与 lint-staged 配置的 pre-commit 钩子，会在暂存文件上自动跑 `eslint`、`prettier`、`cargo fmt`。**不要随意使用 `--no-verify` 跳过钩子**，除非有清晰的理由并写进 PR 说明。

### 风格约定

- **项目文档使用中文**（`docs/`、README、crate 级 `AGENTS.md`、`CONTRIBUTING*.md`，依据 `AGENTS.md`）。
- **代码注释使用英文**（`//`、`///`、`/* */`、doc comments）：新增或修改代码时按英文撰写，存量中文注释顺手改即可、不要求批量翻译。代码标识符、commit message、PR 标题与描述同样保持英文。开发自留路径 —— `.planning/`、`.claude/`、`publish = false` 的诊断 crate —— 在 `.coderabbit.yaml` 中排除 CodeRabbit 审查。
- 仓库内文件 **不得包含机器特定的绝对路径**，统一使用相对仓库根的路径。
- Markdown 代码围栏 **必须带语言标识**（`bash`、`rust`、`ts`、`text` 等）。
- **前端代码** 遵循 [`docs/agent/frontend-ui-rules.md`](./docs/agent/frontend-ui-rules.md)。
- **Rust/Tauri 代码** 遵循 [`docs/agent/rust-tauri-rules.md`](./docs/agent/rust-tauri-rules.md)。

## 测试

### 前端

```bash
bun test           # vitest，watch 模式
bun test --run     # 单次运行，CI 中适用
```

测试基于 Vitest 与 `@testing-library/react`。请把测试与被测代码放在同一目录（如 `Component.test.tsx`）。

### Rust

```bash
cd src-tauri
cargo test --workspace
```

覆盖率报告：

```bash
bun run test:coverage   # 在 src-tauri/target/llvm-cov 下生成 HTML 报告
```

### 手动 / UAT 验证

部分变更需要手动跑 UI 或多设备同步场景。项目把 UAT 记录放在 `docs/uat/`。涉及 UI 的 PR，请在描述中说明你手动验证了哪些路径（黄金路径 + 至少一个边界场景）。

如果某个修复难以用自动化测试覆盖，请在 `docs/fixes/` 下添加一份回归说明，描述失效形式以及现在如何防止它再次发生。

## 文档

- 改动影响用户行为时，`README.md` 与 `README_ZH.md` 应同步更新。
- 内部架构决策放在 `docs/architecture/` 下。
- 面向 agent / 贡献者的说明放在 `docs/agent/` 下，并通过 `AGENTS.md` 索引。
- 发布相关说明在 [`docs/release-workflow.md`](./docs/release-workflow.md) 与 [`docs/CHANGELOG_TEMPLATE.md`](./docs/CHANGELOG_TEMPLATE.md)。

新增顶层文档时，请在 `AGENTS.md` 中加一条指引，便于后续贡献者发现。

## Pull Request

### 提 PR 之前

- 先 rebase 到最新的 `main`。
- 本地确认 `bun run lint`、`bun run format`、`bun test`、`cargo test`（涉及时）都能通过。
- 保持 diff 聚焦。互不相关的改动请拆成独立 PR。

### PR 描述

应包含：

- 改动 **做了什么** 以及 **为什么**（如对应了 issue 请关联）。
- **如何验证**：自动化测试、手动步骤、双 peer 场景等。
- UI 改动请附 **截图或短视频**。
- 涉及存储、加密、网络、daemon 生命周期的改动，请说明 **风险评估**。

### 评审流程

- 维护者会进行 review，可能要求修改。
- 自动化 review bot 提出的建议视为 **输入而非命令**：每条都需要结合代码现状判断后再决定接受或拒绝，并附简短的技术理由。详见 [`docs/agent/workflow-rules.md`](./docs/agent/workflow-rules.md) 中的 "AI Review Intake" 章节。
- CI 必须全绿才能合并。默认使用 squash merge，squash 后的 commit 信息也要遵守上述 commit 规范。

## 发布流程

发布由维护者通过 GitHub Actions 的 `Release` 工作流触发。版本升级规则、发布渠道（`stable` / `alpha` / `beta` / `rc`）以及打包细节详见 [`docs/release-workflow.md`](./docs/release-workflow.md)。一般情况下贡献者无需手动 bump 版本号，由发布工作流统一处理。

如果你的改动应当出现在用户可见的 changelog 中，请在 PR 描述中提一下，便于发布说明引用。格式参考 [`docs/CHANGELOG_TEMPLATE.md`](./docs/CHANGELOG_TEMPLATE.md)：只写用户可感知的变化，每条一行，避免内部术语。

## 许可证

提交贡献即表示你同意你的贡献以 [AGPL-3.0](./LICENSE) 协议授权，与项目其余部分一致。如果你引入了第三方代码，请确保许可证兼容并在 PR 中注明来源。

---

再次感谢你帮 UniClipboard 变得更好。如果本指南有任何不清楚或过时的地方，欢迎开 issue 或 PR —— 改进贡献者体验本身也是一种宝贵的贡献。

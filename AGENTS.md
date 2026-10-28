# AGENTS.md

This file is the root navigation index for repository instructions.

## Core Rule

Do not treat this file as a full memory dump. Read only the documents needed for the current task.

## Always Apply

- **用户内容持久化必须密文（不可打破的基石）**：用户内容——来自剪贴板或经同步收到的内容及其派生数据（剪贴板正文、标题、预览、搜索渲染字段、搜索索引、标签名、剪贴板文件的文件名和文件路径，以及经同步收到的其他设备设备名等用户输入）——写入持久化存储（SQLite 数据库列、磁盘缓存、搜索索引、配置文件）前必须先经 MasterKey AEAD 加密，**严禁明文落库**。例外包括内容类型分类枚举（text/image/file/link 等）、文件内容本体，以及入站文件在受管文件缓存中的经安全清理的原始文件名；该文件名只能作为实际缓存文件的 basename，原始目录路径、数据库/搜索字段、日志及关联元数据仍须加密或脱敏。
  - **凭据**：凭据和密钥材料（MasterKey、passphrase、KEK、relay 访问令牌、鉴权令牌等）只能保存在系统安全存储中，不得写入配置文件、日志或其他普通文件。已有兼容例外仅限 ADR-007 §2.6 的文件式 KEK、ADR-011 的 `daemon.conn` 本机 daemon token，以及 Engine「运行期密钥材料」的 `vault/profile-secrets-v1`；另有仅限调试构建、`development` 环境与独立 `UC_PROFILE` 的开发文件式密钥存储例外，以及用户授权的 pacman 发布签名专用 Environment Secrets 与临时内存目录例外。按原授权边界保留，不得扩大；详见 `VISION.md`。
  - **配置**：用户或程序的偏好与参数（本机设备名、保存目录、relay 地址、局域网地址、快捷键、主题、标签颜色与顺序、启动与更新设置等）可以明文持久化，无需逐项审批；配置不得包含用户内容（例如剪贴板文件的文件名或路径）或凭据。日志与遥测规则不变。
  - **其他持久化数据**：除配置外，新增持久化列/文件默认加密，主张明文需在 PR 中论证并获批。
  - 详见 `VISION.md`「安全与隐私底线」与「绝对禁区」。
- Fix root causes, not symptoms.
- Use the standard, established terminology of software engineering and computer science. If a metaphor or coined expression is unavoidable, also give the corresponding standard term; never invent new words for the sake of novelty.
- Preserve single source of truth and clear ownership.
- Do not keep parallel old/new logic without a removal plan.
- Use repo-relative paths in tracked docs.
- Use language identifiers on fenced code blocks.
- When conversation is in Chinese, respond in natural Chinese.
  - Do not mix Chinese and English within a Chinese reply, except for established proper nouns / technical terms (e.g. crate names, API identifiers, file paths) that have no natural Chinese equivalent.
- **项目文档**（`docs/`、README、crate 级 `AGENTS.md`、`CONTRIBUTING*.md`）使用中文。此约定覆盖全局 `CLAUDE.md` 中"写文档用英文"的默认规则。
  - 引用外部规范（RFC、标准库 API 等）时，专有名词保留英文原文。
- **代码注释**（`//` / `///` / `/* */` / doc comments）使用英文。存量中文注释不强制迁移，新增或修改注释时按英文撰写；踏到旧中文注释可顺手改成英文。
  - 代码标识符（函数、类型、变量名）、Git commit message、PR 标题与描述同样保持英文。
  - **不强制语言审查的开发自留路径**：`.planning/`（调研/spike 笔记）、`.agents/`（项目技能）、`.codex/`（Codex 配置与工具）、`publish = false` 的诊断 crate（例如 `crates/p2p-bench`）。这些目录按写作者方便即可，CodeRabbit 也已在 `.coderabbit.yaml` 中跳过。
- `CLAUDE.md` is only a compatibility entrypoint. This file is the root instruction source.

## Read-on-Demand Map

### 0. Product direction / architecture decisions

Read: `VISION.md`

Use when:
- making product decisions (add/remove features, change UX paradigm)
- evaluating whether a proposal violates project principles
- judging issue priority or scope
- changing architecture direction or locked decisions
- reviewing whether a PR aligns with project goals

**This is the first document to consult before any non-trivial decision.**

### 1. General code change / bug fix / review
Read: `docs/agent/workflow-rules.md`

Use when:
- fixing bugs
- evaluating whether a change is a patch or a refactor
- processing AI review comments
- updating docs or scripts with repository hygiene constraints

### 2. Architecture / boundaries / commit planning
Read: `docs/agent/architecture-rules.md`

Use when:
- changing crate boundaries
- adding ports/adapters
- touching cross-crate DTO conversions
- planning commit splits
- reviewing whether a diff mixes multiple intents

### 2a. Port definition / evolution / refactoring
Read: `docs/architecture/ports.md`

Port 的可写事实来源现位于 `UniClipboard/Engine`。本仓中的这份文档只作为迁移期参考；不得在 desktop 下重新创建或修改 Engine 内部包。

Use when:
- 规划必须在 `UniClipboard/Engine` 实现的 Port 变更
- adding methods to existing port traits
- deciding port granularity or naming
- refactoring large port interfaces into smaller ones
- reviewing whether a use case depends on more than it needs

### 3. Rust / daemon / tracing work
Read: `docs/agent/rust-rules.md`

Use when:
- editing Rust code
- adding or changing daemon APIs or desktop-core use cases
- handling async loops, network drivers, or daemon APIs
- working on tracing/logging
- emitting frontend events from Rust
- running cargo commands

### 4. React / TypeScript / Tailwind / UI work
Read: `docs/agent/frontend-ui-rules.md`

Use when:
- editing React or TypeScript UI code
- adjusting layouts or styling
- touching theme behavior
- working on frontend DTO handling or frontend tests

### 5. Project memory / historical lessons / deeper references
Read: `docs/agent/project-memory.md`

Then selectively read:
- `docs/README.md` and linked docs for current-state guidance
- `.planning/` for roadmap, milestones, and spike research notes
- `apps/gui-go/frontend/src/AGENTS.md` for frontend-local navigation
- `crates/AGENTS.md` for Rust-workspace navigation (crates/ + apps/)
- `apps/gui-go/AGENTS.md` for the Go/Wails desktop host (the only desktop host) and its packaging
- `tools/AGENTS.md` for development-only crates, `tools/uc-dev-cli/AGENTS.md` for the Rust development CLI
- `apps/cli-go/AGENTS.md` for the Go `uniclip` implementation (migration in progress)

Log file locations (platform-conventional, separate from the data root; single
source of truth is `uc_app_paths::app_log_dir()`):
- macOS: `~/Library/Logs/app.uniclipboard.desktop[-<profile>]/`
- Linux: `~/.local/state/app.uniclipboard.desktop[-<profile>]/logs/`
- Windows: `%LOCALAPPDATA%\\app.uniclipboard.desktop[-<profile>]\\logs\\`

Per-role files (`uniclipboard-{gui,daemon,cli,quick-panel}.json.<date>`), daily rotation,
7-day retention (older files pruned on start). Portable builds keep logs under
`<exe>/data/logs/`.

Do not assume the older `uniclipboard` root is current, and note logs are no
longer under the data root's `logs/` subdir on macOS/Linux. The app dir name is
`app.uniclipboard.desktop`, with an optional `UC_PROFILE` suffix such as `-dev`.

Use when:
- entering an unfamiliar subsystem
- trying to understand why a pattern exists
- doing structural work that depends on past decisions

## Practical Loading Order

### Frontend task
1. `AGENTS.md`
2. `docs/agent/frontend-ui-rules.md`
3. `apps/gui-go/frontend/src/AGENTS.md`
4. relevant code/docs only

### Rust/daemon task
1. `AGENTS.md`
2. `docs/agent/rust-rules.md`
3. `docs/agent/architecture-rules.md` if boundaries are involved
4. `crates/AGENTS.md` (plus `apps/gui-go/AGENTS.md` for desktop host packaging work)
5. relevant code/docs only

### Complex bug in unfamiliar area
1. `AGENTS.md`
2. `docs/agent/workflow-rules.md`
3. `docs/agent/project-memory.md`
4. selective reads from `.planning/`, local `AGENTS.md`, and targeted docs

## Files Managed by This Index

- `VISION.md` — 产品方向、架构原则、锁定决策、绝对禁区
- `docs/agent/workflow-rules.md`
- `docs/agent/architecture-rules.md`
- `docs/architecture/ports.md`
- `docs/agent/rust-rules.md`
- `docs/agent/frontend-ui-rules.md`
- `docs/agent/project-memory.md`

If new global guidance is added, prefer placing it in one of those focused documents and only add a pointer here.

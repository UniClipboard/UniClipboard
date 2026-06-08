# AGENTS.md

This file is the root navigation index for repository instructions.

## Core Rule

Do not treat this file as a full memory dump. Read only the documents needed for the current task.

## Always Apply

- Fix root causes, not symptoms.
- Preserve single source of truth and clear ownership.
- Do not keep parallel old/new logic without a removal plan.
- Use repo-relative paths in tracked docs.
- Use language identifiers on fenced code blocks.
- When conversation is in Chinese, respond in natural Chinese.
- **项目文档**（`docs/`、README、crate 级 `AGENTS.md`、`CONTRIBUTING*.md`）使用中文。此约定覆盖全局 `CLAUDE.md` 中"写文档用英文"的默认规则。
  - 引用外部规范（RFC、标准库 API 等）时，专有名词保留英文原文。
- **代码注释**（`//` / `///` / `/* */` / doc comments）使用英文。存量中文注释不强制迁移，新增或修改注释时按英文撰写；踏到旧中文注释可顺手改成英文。
  - 代码标识符（函数、类型、变量名）、Git commit message、PR 标题与描述同样保持英文。
  - **不强制语言审查的开发自留路径**：`.planning/`（调研/spike 笔记）、`.claude/`（本地 agent skill 与工具）、`publish = false` 的诊断 crate（例如 `src-tauri/crates/p2p-bench`）。这些目录按写作者方便即可，CodeRabbit 也已在 `.coderabbit.yaml` 中跳过。
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

Use when:
- defining new ports in `uc-core`
- adding methods to existing port traits
- deciding port granularity or naming
- refactoring large port interfaces into smaller ones
- reviewing whether a use case depends on more than it needs

### 3. Rust / Tauri / daemon / tracing work
Read: `docs/agent/rust-tauri-rules.md`

Use when:
- editing Rust code
- adding or changing Tauri commands
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
- `src/AGENTS.md` for frontend-local navigation
- `src-tauri/AGENTS.md` for Rust/Tauri-local navigation

Log file locations for the current desktop app naming:
- macOS: `~/Library/Application Support/app.uniclipboard.desktop[-<profile>]/logs/`
- Linux: `~/.local/share/app.uniclipboard.desktop[-<profile>]/logs/`
- Windows: `%LOCALAPPDATA%\\app.uniclipboard.desktop[-<profile>]\\logs\\`

Do not assume the older `uniclipboard` root is current. Current code resolves data/logs under `app.uniclipboard.desktop`, with an optional `UC_PROFILE` suffix such as `-dev`.

Use when:
- entering an unfamiliar subsystem
- trying to understand why a pattern exists
- doing structural work that depends on past decisions

## Practical Loading Order

### Frontend task
1. `AGENTS.md`
2. `docs/agent/frontend-ui-rules.md`
3. `src/AGENTS.md`
4. relevant code/docs only

### Rust/Tauri task
1. `AGENTS.md`
2. `docs/agent/rust-tauri-rules.md`
3. `docs/agent/architecture-rules.md` if boundaries are involved
4. `src-tauri/AGENTS.md`
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
- `docs/agent/rust-tauri-rules.md`
- `docs/agent/frontend-ui-rules.md`
- `docs/agent/project-memory.md`

If new global guidance is added, prefer placing it in one of those focused documents and only add a pointer here.

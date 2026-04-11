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
- When writing project docs, use English.
- `CLAUDE.md` is only a compatibility entrypoint. This file is the root instruction source.

## Read-on-Demand Map

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
- `.gsd/KNOWLEDGE.md` for lessons and recurring pitfalls
- `.gsd/DECISIONS.md` for architectural decisions
- `docs/README.md` and linked docs for current-state guidance
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
4. selective reads from `.gsd/KNOWLEDGE.md`, `.gsd/DECISIONS.md`, local `AGENTS.md`, and targeted docs

## Files Managed by This Index

- `docs/agent/workflow-rules.md`
- `docs/agent/architecture-rules.md`
- `docs/agent/rust-tauri-rules.md`
- `docs/agent/frontend-ui-rules.md`
- `docs/agent/project-memory.md`

If new global guidance is added, prefer placing it in one of those focused documents and only add a pointer here.

# Contributing to UniClipboard

Thanks for your interest in contributing to UniClipboard! This document explains how to set up the project, the workflow we follow, and the conventions we expect contributions to respect.

> A Chinese version is available — see [`CONTRIBUTING_ZH.md`](./CONTRIBUTING_ZH.md).

## Table of Contents

- [Code of Conduct](#code-of-conduct)
- [Ways to Contribute](#ways-to-contribute)
- [Reporting Bugs](#reporting-bugs)
- [Suggesting Features](#suggesting-features)
- [Reporting Security Issues](#reporting-security-issues)
- [Development Setup](#development-setup)
- [Project Structure](#project-structure)
- [Development Workflow](#development-workflow)
- [Branching Strategy](#branching-strategy)
- [Commit Conventions](#commit-conventions)
- [Code Style and Quality](#code-style-and-quality)
- [Testing](#testing)
- [Documentation](#documentation)
- [Pull Requests](#pull-requests)
- [Release Process](#release-process)
- [License](#license)

## Code of Conduct

Be respectful, constructive, and patient. We expect everyone interacting in issues, pull requests, and discussions to follow basic open-source etiquette: assume good intent, focus on the technical question, and keep conversations welcoming to newcomers.

## Ways to Contribute

There are many ways to help, no matter your experience level:

- **Report bugs** with clear reproduction steps.
- **Suggest features** that fit the project's privacy-first, cross-device focus.
- **Fix bugs** — start with issues labeled `good first issue` or `help wanted`.
- **Improve documentation** — typos, unclear sections, missing setup steps.
- **Add tests** — both Rust and TypeScript test coverage are always welcome.
- **Translate UI strings** — the app uses `i18next`; new locales are appreciated.
- **Review pull requests** — a thoughtful second pair of eyes is valuable.

## Reporting Bugs

Before opening a new issue, please:

1. Search [existing issues](https://github.com/UniClipboard/UniClipboard/issues) to avoid duplicates.
2. Confirm you can reproduce the bug on the latest release.

When filing a bug, include:

- **Environment** — OS and version, UniClipboard version, install method (DMG, AppImage, MSI, Homebrew, source build).
- **Steps to reproduce** — short, deterministic, numbered.
- **Expected vs actual behavior**.
- **Logs** — relevant excerpts from the log directory:
  - macOS: `~/Library/Application Support/app.uniclipboard.desktop[-<profile>]/logs/`
  - Linux: `~/.local/share/app.uniclipboard.desktop[-<profile>]/logs/`
  - Windows: `%LOCALAPPDATA%\app.uniclipboard.desktop[-<profile>]\logs\`
- **Screenshots or recordings** for UI issues.

Strip personal data from logs and clipboard payloads before posting.

## Suggesting Features

Open a GitHub issue describing:

- The user-facing problem you're trying to solve (not the implementation).
- Why existing functionality is insufficient.
- Whether the feature aligns with the project's principles: privacy-first, end-to-end encrypted, no mandatory cloud account.

Proposals that compromise the security model (for example, server-side plaintext access) will not be accepted.

## Reporting Security Issues

**Do not** report security vulnerabilities through public GitHub issues.

See [`SECURITY.md`](./SECURITY.md) for the disclosure process. We take cryptographic and privacy-related reports seriously and will coordinate a fix and release timeline with you.

## Development Setup

### Prerequisites

- **Rust** — stable toolchain (`rustup` recommended). The version is pinned via `rust-toolchain.toml` if present.
- **Bun** — JavaScript package manager and runtime. Install from [bun.sh](https://bun.sh).
- **Tauri prerequisites** — see the official [Tauri prerequisites guide](https://tauri.app/start/prerequisites/) for OS-specific build dependencies (WebView2 on Windows, `webkit2gtk` and friends on Linux, Xcode CLT on macOS).

Optional but useful:

- `cargo-llvm-cov` for Rust coverage reports.
- `cargo sweep` to keep `target/` directories small during development.

### Clone and Install

```bash
git clone https://github.com/UniClipboard/UniClipboard.git
cd UniClipboard
bun install
```

`bun install` triggers Husky hook installation via the `prepare` script. Pre-commit lint-staged checks will run automatically on `git commit`.

### Run the Desktop App in Development Mode

```bash
# Single instance, dev profile (data lives under app.uniclipboard.desktop-dev)
bun tauri:dev
```

To debug peer-to-peer sync locally, two isolated instances can run side by side on the same machine:

```bash
# Run two peers concurrently — peerA in full clipboard mode, peerB passive
bun tauri:dev:dual

# Or start them individually if you need to attach a debugger
bun tauri:dev:peerA
bun tauri:dev:peerB
```

Each peer uses a different `UC_PROFILE` so their data, vault, and logs do not collide.

### Build a Release Bundle

```bash
bun tauri build
```

Bundles land in `src-tauri/target/release/bundle/`.

### Release-time Secrets (Telemetry)

Release builds optionally bake telemetry credentials into the binary at
compile time via `option_env!`. None of these are required to build or
run the app — when missing, the matching channel falls back to a noop
sink and the app boots normally.

| Secret                  | Channel                                     | Compile-time read                                  | CI workflow source                              |
| ----------------------- | ------------------------------------------- | -------------------------------------------------- | ----------------------------------------------- |
| `SENTRY_DSN`            | Backend Sentry (errors / breadcrumbs)       | `uc-bootstrap/src/tracing.rs` — `option_env!`      | `.github/workflows/{build,alpha-build}.yml`     |
| `VITE_SENTRY_DSN`       | Frontend Sentry (must be a separate project) | `import.meta.env.VITE_SENTRY_DSN` (Vite at build)  | same workflows                                  |
| `POSTHOG_PROJECT_KEY`   | Product analytics (PostHog Cloud, US)       | `uc-bootstrap/src/analytics.rs` — `option_env!`    | same workflows (added as part of issue #549)    |

Local dev builds: set the variable in your shell to opt in; otherwise
the dev profile uses a stdout sink for analytics and the `cfg(dev)`
Sentry DSN baked at compile time stays effective. See
[`docs/architecture/telemetry-events.md`](./docs/architecture/telemetry-events.md)
§10.1 for the PostHog injection contract and degraded-startup
semantics.

Empty values count as "missing" — when a secret is not configured,
`${{ secrets.X }}` renders to an empty string and the matching sink
silently drops to noop. Never commit any of these values to the repo
or to issue / PR text.

## Project Structure

```text
.
├── src/                # React + TypeScript frontend (Tauri webview)
├── src-tauri/          # Rust workspace (daemon, app shell, core, infra, platform crates)
├── workers/            # Cloudflare Worker for the encrypted relay
├── docs/               # Architecture, agent rules, release workflow, UAT, etc.
├── scripts/            # Dev/release scripts (e.g. bump-version.js)
├── public/             # Static assets served by Vite
├── assets/             # Marketing/icon assets
├── AGENTS.md           # Root navigation index for repository instructions
└── README.md           # User-facing project introduction
```

`AGENTS.md` is the canonical entry point for repository conventions. When working on a specific area, read the focused document it links to (frontend, Rust/Tauri, architecture, workflow, or project memory) instead of skimming everything.

## Development Workflow

The project enforces a structured approach for non-trivial changes. Read [`docs/agent/workflow-rules.md`](./docs/agent/workflow-rules.md) for the full rules; the highlights are:

- **Fix root causes, not symptoms.** No "temporary compromise" patches that hide structural issues.
- **Preserve a single source of truth.** Do not duplicate the same business rule across modules.
- **No parallel old/new logic** without an explicit removal plan.
- **Architecture matters.** The Rust workspace follows a hexagonal layering rule: `uc-app → uc-core ← uc-infra / uc-platform`. `uc-core` must not depend on infra or platform crates. See [`docs/agent/architecture-rules.md`](./docs/agent/architecture-rules.md) and [`docs/architecture/ports.md`](./docs/architecture/ports.md).

If you are unsure whether a change is a localized bug fix or a structural one, default to opening an issue or draft PR for discussion before investing time in a refactor.

## Branching Strategy

The project uses a **trunk-based workflow** anchored on `main`:

- **`main`** — the trunk. Every change lands here through a pull request. `main` must stay buildable and shippable at all times.
- **`release/vX.Y.Z[-channel.N]`** — cut from `main` by the `prepare-release` workflow when preparing a release (alpha / beta / rc / stable). Merging the release PR back into `main` triggers tagging and artifact builds; the release branch is auto-deleted afterwards.
- **Feature branches** — branch from `main`, name them descriptively (e.g. `feat/quick-panel-search`, `fix/devices-online-state`).

When opening a PR, target `main` unless the maintainers explicitly ask you to target a release branch.

Unfinished work should not be merged. If a feature is not ready to ship, keep it on its feature branch (or behind a runtime gate) rather than merging a half-finished change into `main`.

## Commit Conventions

Every commit must represent **exactly one engineering intent**. See [`docs/agent/architecture-rules.md`](./docs/agent/architecture-rules.md#atomic-commit-rule) for the full rule set.

### Allowed Commit Types

| Type        | Use For                                                              |
| ----------- | -------------------------------------------------------------------- |
| `feat:`     | New user-facing capability                                           |
| `impl:`     | Concrete implementation step of a previously planned feature         |
| `fix:`      | Bug fix                                                              |
| `hotfix:`   | Urgent production fix                                                |
| `refactor:` | Structural change without behavior change                            |
| `arch:`     | Architecture or boundary change (e.g. introducing a new port)        |
| `chore:`    | Tooling, build, dependency, scripts                                  |
| `infra:`    | Deployment or environment config                                     |
| `test:`     | Add or adjust tests                                                  |
| `perf:`     | Performance optimization (benchmarks expected)                       |
| `docs:`     | Documentation only                                                   |

### Format

```text
<type>(<optional scope>): <single intent summary>

[optional body explaining why, not what]
```

Examples drawn from the project's history:

```text
fix(storage): isolate cache_dir from data root on Windows
fix(devices): show real online state and cut offline detection latency
chore(observability): silence swarm_discovery::socket EHOSTUNREACH spam
```

### What to Avoid

- Mixing a feature change with formatting cleanup in the same commit.
- Commits whose message needs `and`, `also`, `plus`, or `misc` to summarize them — split them.
- Commits that modify both a port definition and its adapter — split them so the port lands first.
- Commits that build successfully only because a follow-up commit "completes" them.

If you accumulate many local changes, the [`atomic-commits`](https://docs.anthropic.com/) workflow encourages re-organizing them into clean, single-intent commits before pushing.

## Code Style and Quality

### Linters and Formatters

JavaScript/TypeScript:

```bash
bun run lint        # eslint
bun run lint:fix    # eslint --fix
bun run format      # prettier --write .
```

Rust (run inside `src-tauri/`):

```bash
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
```

Pre-commit hooks (via Husky and lint-staged) automatically run `eslint`, `prettier`, and `cargo fmt` on staged files. Do not bypass hooks (`--no-verify`) without a documented reason.

### Style Guidelines

- **Comments and project documentation are written in Chinese**, per `AGENTS.md`. Identifiers, commit messages, and PR titles/descriptions remain English so tooling and external collaborators stay aligned.
- **No machine-specific absolute paths** in tracked files. Use repo-relative paths in docs and configuration.
- **Markdown fenced code blocks must include a language identifier** (`bash`, `rust`, `ts`, `text`, etc.).
- **Frontend code** follows the rules in [`docs/agent/frontend-ui-rules.md`](./docs/agent/frontend-ui-rules.md).
- **Rust/Tauri code** follows the rules in [`docs/agent/rust-tauri-rules.md`](./docs/agent/rust-tauri-rules.md).

## Testing

### Frontend

```bash
bun test           # vitest, watch mode
bun test --run     # single run, useful in CI
```

Tests use Vitest with `@testing-library/react`. Place colocated tests next to the code they cover (e.g. `Component.test.tsx`).

### Rust

```bash
cd src-tauri
cargo test --workspace
```

For coverage reports:

```bash
bun run test:coverage   # produces an HTML report under src-tauri/target/llvm-cov
```

### Manual / UAT Verification

Some changes require UI verification or multi-device sync testing. The project keeps UAT notes under `docs/uat/`. For UI work, please describe in the PR what you exercised manually (golden path + at least one edge case).

When a fix is hard to cover with automated tests, add a regression note under `docs/fixes/` describing the failure mode and how it is now prevented.

## Documentation

- User-facing changes that alter behavior should update `README.md` and `README_ZH.md` together.
- Internal architectural decisions belong under `docs/architecture/`.
- Agent/contributor instructions belong under `docs/agent/` and are linked from `AGENTS.md`.
- Release-related guidance lives in [`docs/release-workflow.md`](./docs/release-workflow.md) and [`docs/CHANGELOG_TEMPLATE.md`](./docs/CHANGELOG_TEMPLATE.md).

If you add a new top-level doc, add a pointer to it from `AGENTS.md` so future contributors can discover it.

## Pull Requests

### Before You Open a PR

- Rebase onto the latest `main`.
- Make sure `bun run lint`, `bun run format`, `bun test`, and `cargo test` (where relevant) all pass locally.
- Keep the diff focused. Open separate PRs for unrelated changes.

### PR Description

Include:

- **What** changed and **why** (link to the issue if applicable).
- **How** you verified the change — automated tests, manual steps, dual-peer scenarios, etc.
- **Screenshots or short clips** for UI changes.
- **Risk assessment** for changes that touch storage, encryption, networking, or daemon lifecycle.

### Review Process

- A maintainer will review and may request changes.
- Automated review bots may post suggestions. Treat them as **inputs, not commands** — verify each item against the codebase, then accept or reject with a short technical justification. See the "AI Review Intake" section in [`docs/agent/workflow-rules.md`](./docs/agent/workflow-rules.md).
- CI must be green before merging. PRs use squash merge by default; the squashed message should follow the commit conventions above.

## Release Process

Releases are driven by maintainers via the GitHub Actions `Release` workflow. End-to-end version bump rules, channel definitions (`stable` / `alpha` / `beta` / `rc`), and packaging details are documented in [`docs/release-workflow.md`](./docs/release-workflow.md). Contributors generally do not need to bump versions manually — the workflow handles it.

If your change should appear in the user-facing changelog, mention it in your PR description so the release notes can pick it up. The format follows [`docs/CHANGELOG_TEMPLATE.md`](./docs/CHANGELOG_TEMPLATE.md): only user-perceivable changes, one line each, no internal jargon.

## License

By contributing to UniClipboard, you agree that your contributions will be licensed under the [AGPL-3.0](./LICENSE) license, the same license that covers the rest of the project. If you incorporate third-party code, ensure its license is compatible and document the source in the PR.

---

Thanks again for helping make UniClipboard better. If anything in this guide is unclear or out of date, open an issue or PR — improving the contributor experience is itself a valuable contribution.

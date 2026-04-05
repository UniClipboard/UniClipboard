# UniClipboard Documentation

UniClipboard Desktop is a privacy-first, cross-device clipboard synchronization tool built with Tauri 2, React, and a modular Rust workspace.

This documentation set is a mix of:

- **Current-state guides** that should match the codebase today
- **Architecture intent** documents that describe target boundaries and design rules

When documentation conflicts with code, treat the code as the source of truth and update the docs.

## Quick Navigation

**For New Developers:**

- [Project Overview](overview.md) - What is UniClipboard and how it works
- [Architecture Principles](architecture/principles.md) - Understanding Hexagonal Architecture
- [Module Boundaries](architecture/module-boundaries.md) - What each crate/layer may depend on

**For Implementation:**

- [Bootstrap System](architecture/bootstrap.md) - How dependency injection works
- [Snapshot Cache Pipeline ADR](architecture/snapshot-cache/adr-001-snapshot-cache-pipeline.md) - Cache/spool/worker design decisions
- [Error Handling](guides/error-handling.md) - Error handling strategy
- [GitHub Releases Updater](guides/github-releases-updater.md) - Auto-update pipeline with latest.json

**For Code Review:**

- [Coding Standards](guides/coding-standards.md) - Code style and conventions
- [Module Boundaries](architecture/module-boundaries.md) - Architecture compliance checklist

**For Reference:**

- [DeepWiki Documentation](https://deepwiki.com/UniClipboard/UniClipboard) - Interactive diagrams and flows

## Architecture at a Glance

```
┌─────────────────────────────────────────────────────────────┐
│                    React + Tauri GUI                      │
│  ┌──────────────────────────────────────────────────────┐ │
│  │                    uc-tauri                          │ │
│  │  - command wiring                                    │ │
│  │  - tray / quick panel                                │ │
│  │  - GUI ↔ daemon integration                           │ │
│  └──────────────────────────────────────────────────────┘ │
└─────────────────────────────────────────────────────────────┘
                            ↓ bootstrap / invoke / events
┌─────────────────────────────────────────────────────────────┐
│                      uc-app                                  │
│  (Use Cases, orchestration, port-only dependencies)          │
└─────────────────────────────────────────────────────────────┘
                            ↓ domain + ports
┌─────────────────────────────────────────────────────────────┐
│                      uc-core                                 │
│           (Domain models, IDs, protocols, port definitions)  │
└─────────────────────────────────────────────────────────────┘
                            ↑ implemented by
        ┌───────────────────┴───────────────────┐
        │                                       │
┌──────────────────┐      ┌──────────────────┐      ┌──────────────────┐
│   uc-infra       │      │  uc-platform     │      │    uc-daemon      │
│ (DB, FS, crypto, │      │ (clipboard, OS,  │      │ background sync,  │
│ settings, blobs) │      │ network runtime) │      │ WS/API bridge)    │
└──────────────────┘      └──────────────────┘      └──────────────────┘
```

**Key Principle**: `uc-app` depends on `uc-core` abstractions. Implementations are wired by bootstrap code and consumed by both the GUI process and the daemon process.

## Crate Structure

```
src-tauri/crates/
├── uc-core/         # Pure domain layer (Port definitions)
├── uc-infra/        # Infrastructure implementations (DB, FS, crypto)
├── uc-platform/     # Platform adapters (Clipboard, Network, OS)
├── uc-app/          # Application layer (Use cases, business logic)
└── uc-tauri/        # Tauri integration (Commands, Bootstrap)
```

## Current State

The codebase is already organized around a modular Rust workspace and hexagonal boundaries, but the migration is still ongoing in practice.

- Core domain, application use cases, infrastructure adapters, and platform adapters all exist as first-class crates
- The Tauri entrypoint still carries important integration logic for bootstrap, daemon supervision, resource resolution, and window management
- Historical migration notes may still refer to removed legacy paths or earlier architecture phases
- Avoid relying on old completion percentages; prefer current code and current docs

## Getting Started

1. **Read** [Project Overview](overview.md) to understand the system
2. **Study** [Architecture Principles](architecture/principles.md) to grasp the design
3. **Review** [Module Boundaries](architecture/module-boundaries.md) before making changes
4. **Follow** [Coding Standards](guides/coding-standards.md) when implementing

## Development Workflow

```bash
# Install dependencies (uses Bun)
bun install

# Start frontend-only dev server
bun run dev

# Start full Tauri app in development
bun run tauri:dev

# Run frontend tests
bun run test

# Run Rust workspace tests
(cd src-tauri && cargo test --workspace)

# Build for production
bun run tauri build
```

## Documentation Guide

### How to Use These Documents

**When implementing a feature:**

1. Check [Module Boundaries](architecture/module-boundaries.md) to understand which crates are involved
2. Review [Bootstrap System](architecture/bootstrap.md) to see how to inject dependencies
3. Follow [Error Handling](guides/error-handling.md) for proper error propagation

**When reviewing code:**

1. Verify architecture compliance using [Module Boundaries](architecture/module-boundaries.md) checklists
2. Check [Coding Standards](guides/coding-standards.md) for style and conventions
3. Ensure error handling follows [Error Handling](guides/error-handling.md) strategy

**When making architectural decisions:**

1. Reference [Architecture Principles](architecture/principles.md) for core principles
2. Review [Bootstrap System](architecture/bootstrap.md) for dependency injection patterns
3. Prefer current architecture docs and code over historical planning material

### Document Conventions

- **✅ Allowed**: What you SHOULD do
- **❌ Prohibited**: What you MUST NOT do
- **⚠️ Warning**: Common pitfalls to avoid
- **Iron Rule**: Critical architectural constraint that cannot be violated

## Contributing to Documentation

When updating documentation:

1. Keep it focused on **principles**, not implementation details
2. Use **examples** from actual code when possible
3. Update **cross-references** if moving or renaming sections
4. **Avoid duplication** - link to existing sections instead of repeating

## Additional Resources

- **Project DeepWiki**: https://deepwiki.com/UniClipboard/UniClipboard (interactive diagrams)
- **GitHub Repository**: https://github.com/UniClipboard/UniClipboard
- **CLAUDE.md**: Project-specific instructions for Claude Code (in repository root)

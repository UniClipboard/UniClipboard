# Progress

## 2026-08-18

- Confirmed the active goal is full ADR-012 implementation plus real daemon E2E.
- Restored the implementation plan after the documentation-only turn.
- Existing Engine pin, lockfile cleanup, join response compatibility, and ADR remain in the worktree.
- Added four daemon-client contract tests and observed the expected missing-method compilation failures.
- Implemented cancel join, device trust decision, member sync GET, and member sync PATCH client calls.
- Added CLI command modules and focused behavior tests for join waiting, trust decisions, and member sync settings.
- Connected all ADR-012 subcommands to the CLI entrypoint and preserved failure exit codes for rejected join JSON output.
- Made join failures and interruptions emit one structured JSON object on stdout and added stable JSON shape tests for all three workflows.
- Added real-daemon E2E scenarios for join status and pairing, member sync read-update-read, and three-device trust decisions.
- Tried three real-process strategies to hold an Engine join in `pending`; all depended on an unacceptably narrow scheduling race because Engine immediately advances the session transition.
- Removed the timing-dependent pending E2E and its process-ID helper. Kept the three stable real-daemon scenarios for join, member sync, and device trust.
- Passed `cargo fmt --all -- --check`, 116 CLI tests, 14 daemon-client tests, and `cargo check --workspace --locked`.
- Passed all three real-daemon CLI workflow tests: join, member sync, and device trust.
- Smoke-tested the root, join, join status, member trust, and member sync help pages.
- Tightened degraded-daemon setup attachment so it rejects any same-version non-degraded failure, added the regression case, and reran the real join scenario successfully.
- Completed diff hygiene and scoped Clippy review. Full strict Clippy remains blocked only by pre-existing warnings outside the changed paths.
- Made ADR background wording timeless by describing CLI gaps as the state before implementation; retained `提议` for user approval.
- Reopened the completion audit against every ADR acceptance item instead of treating three happy-path E2E scenarios as full completion.
- Found that successful cancel and rejected status queries inherited the start-command failure result. Added failing tests first, then introduced intent-specific outcomes; 13 focused join tests now pass.
- Updated the Engine pin to current `origin/main` at `1fe43f83973bedd89bd6dba99014f47784eaf3d3` and reran focused and real-daemon coverage.
- Traced the missing pending status to Engine's unavailable snapshot hiding `current_join` while the active state is locked. Recorded the Engine prerequisite in ADR-012 instead of adding CLI-owned state.
- Replaced the invalid real-daemon rejection assumption with stable JSON and human passphrase-failure coverage; verified that the subsequent public status is `none`.
- Fixed Engine projection fallback so a durable pending join remains visible during transition, pinned Desktop to `94b21aac9db2fa0fb89bc9027f0e05e545ecc1f5`, and pushed the Engine branch.
- Added deterministic real-daemon pending coverage for Ctrl-C detachment, stable join identity, daemon restart visibility, and explicit cancellation.
- Added human-output E2E coverage for pending, member sync show/set, device trust status, and stale decisions.
- Added controlled coverage proving a successful sync update is reported as failure when the authoritative reread fails, with exactly one update and one reread.
- Passed all five real-daemon CLI workflow tests, 120 CLI tests, and 14 daemon-client tests.
- Added real-daemon request-count assertions: join, cancel, sync update, and trust decisions submit exactly once; stale and unconfirmed decisions submit zero times.

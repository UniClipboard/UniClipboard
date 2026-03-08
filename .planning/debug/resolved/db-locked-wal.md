---
status: resolved
trigger: "r2d2 connection pool initialization causes 'database is locked' errors when multiple connections simultaneously try to set journal_mode=WAL on startup. 100% reproducible."
created: 2026-03-08T09:00:00Z
updated: 2026-03-08T09:15:00Z
---

## Current Focus

hypothesis: CONFIRMED - WAL mode set per-connection in on_acquire causes lock contention during concurrent pool init
test: Applied fix, compiled successfully, all 206 uc-infra tests pass
expecting: No more "database is locked" warnings at startup
next_action: Awaiting human verification - user needs to run `bun tauri dev` and confirm no WAL lock errors in logs

## Symptoms

expected: Database pool should initialize cleanly without lock errors
actual: Multiple WARN/ERROR messages "Failed to set journal_mode=WAL error=database is locked" from r2d2 pool on every startup (~10 occurrences within 1-2ms window)
errors: "2026-03-08 08:51:51.974 WARN uc_infra::db::pool: Failed to set journal_mode=WAL error=database is locked" and "r2d2: database is locked"
reproduction: Start the app with `bun tauri dev`. 100% reproducible every startup.
started: Ongoing issue. App continues to work after errors (migrations complete ~400ms later).

## Eliminated

## Evidence

- timestamp: 2026-03-08T09:00:00Z
  checked: pool.rs SqlitePragmaCustomizer::on_acquire implementation
  found: PRAGMA execution order is (1) journal_mode=WAL, (2) busy_timeout=5000, (3) foreign_keys=ON. The busy_timeout is set AFTER journal_mode=WAL.
  implication: When multiple connections are initialized concurrently by r2d2, the journal_mode=WAL pragma runs without any busy_timeout, so SQLite returns SQLITE_BUSY immediately instead of waiting.

- timestamp: 2026-03-08T09:01:00Z
  checked: r2d2 Pool::builder() default pool size and behavior
  found: r2d2 default pool size is 10. Pool::build() initializes min_idle connections (defaults to pool size) concurrently. Each calls on_acquire -> SqlitePragmaCustomizer.
  implication: ~10 connections all try to set journal_mode=WAL simultaneously. Only one can hold the write lock; the rest fail immediately.

- timestamp: 2026-03-08T09:02:00Z
  checked: PRAGMA journal_mode=WAL semantics in SQLite
  found: Setting journal_mode=WAL is a write operation that requires an exclusive lock on the database file. Once set, it persists (WAL mode is sticky across connections). Subsequent connections opening the same DB file in WAL mode don't need to re-set it.
  implication: Only the FIRST connection ever needs to set WAL. All pool connections trying to set it is redundant AND causes lock contention.

## Resolution

root_cause: The SqlitePragmaCustomizer sets PRAGMA journal_mode=WAL on every connection acquired from the pool. During pool initialization, r2d2 creates multiple connections concurrently. Setting journal_mode requires an exclusive lock. Since busy_timeout hasn't been set yet (it's the second pragma), all connections except the first fail immediately with "database is locked". Additionally, WAL mode is persistent/sticky - once set on the database file, all new connections automatically use it, so setting it per-connection is unnecessary.
fix: Extracted WAL mode setup into a dedicated `enable_wal_mode()` function that runs on a single connection BEFORE pool creation. Removed `PRAGMA journal_mode=WAL` from `SqlitePragmaCustomizer::on_acquire`. The customizer now only sets per-connection pragmas (busy_timeout, foreign_keys). WAL mode is a database-file-level persistent setting, so it only needs to be set once.
verification: cargo check passes (0 errors). All 206 uc-infra tests pass (3 ignored). Human verified: no more "database is locked" warnings on startup, WAL journal mode enabled log appears correctly, app works normally.
files_changed: [src-tauri/crates/uc-infra/src/db/pool.rs]

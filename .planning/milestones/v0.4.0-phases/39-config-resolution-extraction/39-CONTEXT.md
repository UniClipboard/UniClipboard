# Phase 39: Config Resolution Extraction - Context

**Gathered:** 2026-03-18
**Status:** Ready for planning
**Mode:** Auto-resolved, revised after scope review

<domain>
## Phase Boundary

Extract inline config/path resolution logic from `main.rs` into a reusable, testable module within `uc-tauri/src/bootstrap/`. Delete duplicate functions in main.rs that already exist in assembly.rs. Make run_app() delegate to existing path infrastructure instead of manual inline resolution.

**Minimum success criteria** (from ROADMAP.md):

1. A dedicated config resolution module (not main.rs) owns all path/profile/keyslot resolution functions and is accessible to non-Tauri entry points
2. main.rs delegates to the module rather than containing inline resolution logic; main.rs shrinks accordingly
3. The resolution functions are unit-testable without a running Tauri app
4. GUI app launches and resolves config paths correctly after the extraction

**In scope:**

- Extract `resolve_config_path()` from main.rs into `uc-tauri/src/bootstrap/` (new `config_resolution.rs` module)
- Extract the main() config loading orchestration (resolve path → load → fallback to system defaults) into a `resolve_app_config() -> Result<AppConfig, ConfigResolutionError>` function
- Delete duplicate `apply_profile_suffix()` from main.rs (assembly.rs:632 is authoritative, has slash sanitization)
- Delete `resolve_keyslot_store_vault_dir()` from main.rs (subsumed by assembly.rs `resolve_app_paths()` which already resolves `AppPaths::vault_dir`)
- Eliminate inline key_slot_store path resolution in run_app() lines 540-554 by using `AppPaths::vault_dir` from existing `get_storage_paths()`
- Move existing main.rs tests (lines 273-349) to the new module
- Verify run_app() uses existing `get_storage_paths()` (already `pub` and re-exported from bootstrap/mod.rs)

**Out of scope:**

- Moving assembly.rs functions (resolve_app_dirs, resolve_app_paths, apply_profile_suffix, get_storage_paths) out of uc-tauri — Phase 40
- Moving load_config out of uc-tauri — Phase 40
- Creating new crates — Phase 40
- Daemon/CLI entry points — Phase 41
- Modifying the Tauri Builder setup block
- Changing tracing initialization

</domain>

<decisions>
## Implementation Decisions

### Target module location: uc-tauri/src/bootstrap/config_resolution.rs

The new module lives in `uc-tauri/src/bootstrap/` alongside assembly.rs and config.rs. It is NOT in uc-app.

**Rationale:**

- uc-app does not depend on uc-platform (production deps). The config loading fallback needs `DirsAppDirsAdapter` (uc-platform) for system default dirs. Putting this in uc-app would violate the layering constraint `uc-app → uc-core ← uc-platform`.
- assembly.rs already contains the authoritative path resolution functions. The new module sits alongside it in the same bootstrap namespace.
- Phase 40 creates uc-bootstrap which will pull these functions out of uc-tauri into a proper composition root crate. This phase prepares clean extraction boundaries without prematurely breaking the crate graph.

**SC#1 partial satisfaction note:** The extracted functions are pure (take inputs, return outputs) and have no Tauri API calls, but they live in `uc-tauri` which has a crate-level `tauri` dependency. This means non-Tauri entry points cannot yet depend on them without pulling in tauri. This phase satisfies the "extract and purify" part of SC#1; true crate-level accessibility for daemon/CLI is delivered by Phase 40 (uc-bootstrap).

### resolve_app_config() API design

```rust
pub fn resolve_app_config() -> Result<AppConfig, ConfigResolutionError>
```

Returns `Result`, NOT a bare `AppConfig`. The entry point (main.rs, future daemon, future CLI) decides how to handle errors:

- Config file not found → `Ok(AppConfig::with_system_defaults(...))` (recoverable, logged as debug)
- Config file found but malformed → `Err(ConfigResolutionError::InvalidConfig { .. })`
- Platform directory resolution failed → `Err(ConfigResolutionError::PlatformDirsFailed { .. })`

main.rs `main()` becomes:

```rust
fn main() {
    init_tracing_subscriber().unwrap_or_else(|e| { eprintln!(...); exit(1) });
    let config = match resolve_app_config() {
        Ok(config) => config,
        Err(e) => { error!("{}", e); exit(1); }
    };
    run_app(config);
}
```

The function internally calls `resolve_config_path()` → `load_config()` with fallback, and on fallback calls `DirsAppDirsAdapter` → `AppConfig::with_system_defaults()`. Fatal vs recoverable error distinction is preserved in the error type.

### Deduplication: main.rs functions to delete

| main.rs function                                 | Why delete                                                          | Replacement                                              |
| ------------------------------------------------ | ------------------------------------------------------------------- | -------------------------------------------------------- |
| `apply_profile_suffix()` (line 377)              | Duplicate of assembly.rs:632 — missing slash sanitization           | assembly.rs version (already `pub(crate)`)               |
| `resolve_keyslot_store_vault_dir()` (line 393)   | Subsumed by `resolve_app_paths()` vault_dir logic                   | `AppPaths::vault_dir` from `get_storage_paths()`         |
| inline key_slot_store resolution (lines 540-554) | Manual re-derivation of paths already computed by get_storage_paths | `storage_paths.vault_dir` (already computed at line 557) |

After deduplication, main.rs loses ~80 lines.

### Key slot store resolution consolidation

Currently `run_app()` has TWO separate path resolution flows:

```rust
// Line 557 — already computes all paths including vault_dir
let storage_paths = get_storage_paths(&config).expect("failed to get storage paths");

// Lines 540-554 — REDUNDANT manual vault_dir resolution
let key_slot_store: Arc<dyn KeySlotStore> = {
    let app_data_root = if config.database_path... { ... };
    let vault_dir = resolve_keyslot_store_vault_dir(&config, app_data_root);
    Arc::new(JsonKeySlotStore::new(vault_dir))
};
```

**Fix:** Replace the inline resolution with:

```rust
let storage_paths = get_storage_paths(&config).expect("failed to get storage paths");
let key_slot_store: Arc<dyn KeySlotStore> = Arc::new(JsonKeySlotStore::new(storage_paths.vault_dir.clone()));
```

This eliminates `resolve_keyslot_store_vault_dir` entirely — `resolve_app_paths()` already handles all the same cases (empty vault_key_path, vault relative to db, profile suffix).

### resolve_config_path() stays pure

`resolve_config_path()` is extracted as-is — a pure function that:

1. Checks `UC_CONFIG_PATH` env var
2. Walks directory ancestors looking for `config.toml` / `src-tauri/config.toml`
3. Returns `Option<PathBuf>`

No dependencies beyond `std`. Trivially testable.

### Existing test migration

main.rs tests (lines 273-349) move to the new module:

- `test_cors_headers_*` — stay in main.rs (they test CORS, not config resolution)
- `test_resolve_config_path_finds_parent_directory` → new module
- `test_resolve_config_path_finds_src_tauri_config_from_repo_root` → new module

**Preserve CWD_TEST_LOCK pattern** — the static Mutex serializing CWD manipulation tests must be kept since `resolve_config_path()` depends on `current_dir()`.

assembly.rs already has existing tests for `apply_profile_suffix` (wiring.rs:5307) and `resolve_app_paths` vault_dir logic (wiring.rs:5156+). These tests stay where they are — no migration needed.

### ConfigResolutionError type

Dedicated error enum (not anyhow) to give entry points structured error handling:

```rust
pub enum ConfigResolutionError {
    InvalidConfig { path: PathBuf, source: anyhow::Error },
    PlatformDirsFailed { source: anyhow::Error },
}
```

### Claude's Discretion

- Exact module file structure (single file vs sub-module)
- Whether `resolve_config_path` and `resolve_app_config` are free functions or methods on a struct
- ConfigResolutionError variant naming and Display impl
- Whether to re-export from bootstrap/mod.rs for external visibility
- Commit split granularity
- Whether `get_storage_paths` call ordering in run_app() needs adjustment (currently storage_paths computed after key_slot_store)

</decisions>

<canonical_refs>

## Canonical References

**Downstream agents MUST read these before planning or implementing.**

### Requirements and phase definition

- `.planning/ROADMAP.md` Phase 39 — 4 success criteria (SC#1-4), requirement RNTM-03
- `.planning/ROADMAP.md` Phase 40 — uc-bootstrap scope boundary (assembly.rs migration belongs there)

### Prior phase context

- `.planning/phases/38-coreruntime-extraction/38-CONTEXT.md` — CoreRuntime in uc-app, assembly.rs as composition module
- `.planning/phases/36-event-emitter-abstraction/36-CONTEXT.md` — HostEventEmitterPort, event emitter architecture

### Primary code: extraction sources (main.rs)

- `src-tauri/src/main.rs:352-375` — `resolve_config_path()` (extract)
- `src-tauri/src/main.rs:377-391` — `apply_profile_suffix()` (delete — duplicate)
- `src-tauri/src/main.rs:393-422` — `resolve_keyslot_store_vault_dir()` (delete — subsumed)
- `src-tauri/src/main.rs:437-480` — main() config loading flow (extract to resolve_app_config)
- `src-tauri/src/main.rs:540-554` — inline key_slot_store path resolution (replace with storage_paths.vault_dir)
- `src-tauri/src/main.rs:273-349` — existing tests (migrate config-related ones)

### Primary code: authoritative versions (assembly.rs)

- `src-tauri/crates/uc-tauri/src/bootstrap/assembly.rs:632-646` — `apply_profile_suffix()` (authoritative, has slash sanitization)
- `src-tauri/crates/uc-tauri/src/bootstrap/assembly.rs:544-549` — `get_storage_paths()` (returns AppPaths with vault_dir)
- `src-tauri/crates/uc-tauri/src/bootstrap/assembly.rs:552-630` — `resolve_app_dirs()` + `resolve_app_paths()` (full path resolution)

### Key types

- `src-tauri/crates/uc-app/src/app_paths.rs:6-18` — `AppPaths` struct (already has `vault_dir: PathBuf` at line 8)
- `src-tauri/crates/uc-tauri/src/bootstrap/config.rs:55` — `load_config()` (pure TOML loading)
- `src-tauri/crates/uc-core/src/config.rs` — `AppConfig` struct, `AppConfig::with_system_defaults()`

### Existing test coverage (DO NOT lose)

- `src-tauri/crates/uc-tauri/src/bootstrap/wiring.rs:5156+` — resolve_app_paths vault_dir tests
- `src-tauri/crates/uc-tauri/src/bootstrap/wiring.rs:5307` — apply_profile_suffix sanitization test
- `src-tauri/src/main.rs:312-349` — resolve_config_path tests (migrate to new module)

### Dependency constraints

- `src-tauri/crates/uc-app/Cargo.toml` — uc-app depends on uc-core only (NOT uc-platform) — config resolution CANNOT live in uc-app
- `src-tauri/crates/uc-platform/src/ports/app_dirs.rs` — `AppDirsPort` defined in uc-platform

</canonical_refs>

<code_context>

## Existing Code Insights

### Reusable Assets

- `assembly.rs:apply_profile_suffix` — Authoritative version with `replace('/', "_").replace('\\', "_")` sanitization; main.rs version lacks this
- `assembly.rs:resolve_app_paths` — Complete vault_dir resolution handling all cases: empty vault_key_path, vault relative to db root, profile suffix, in-memory db
- `assembly.rs:get_storage_paths` — Composes `get_default_app_dirs()` + `resolve_app_paths()` into single call
- `config.rs:load_config` — Pure TOML loading, no validation; called by resolve_app_config
- `DirsAppDirsAdapter` (uc-platform) — Platform directory resolution for system defaults fallback

### Established Patterns

- Config env vars: `UC_CONFIG_PATH` (explicit config file path), `UC_PROFILE` (profile suffix for multi-instance)
- Fallback chain: env var → directory search → system defaults
- Profile suffix: `{dir_name}_{profile}` with separator sanitization
- bootstrap/mod.rs re-exports pub items for external consumers

### Integration Points

- main.rs `main()` → calls `resolve_app_config()` instead of inline logic
- main.rs `run_app()` → replaces lines 540-554 with `storage_paths.vault_dir`
- bootstrap/mod.rs → re-exports new module's public API
- assembly.rs functions unchanged — just consumed by the new module

### Known Pitfalls

- **CWD dependency in tests**: `resolve_config_path()` uses `std::env::current_dir()`. Tests need the static Mutex guard pattern (CWD_TEST_LOCK) to avoid flaky parallel test interference.
- **storage_paths ordering**: In run_app(), `get_storage_paths(&config)` is called at line 557 AFTER the inline key_slot_store resolution at lines 540-554. After this phase, the call must happen BEFORE key_slot_store creation, or key_slot_store must use the same storage_paths instance.
- **Existing test assets**: wiring.rs tests for resolve_app_paths and apply_profile_suffix must NOT be moved or broken. They stay in wiring.rs and continue testing assembly.rs functions.

</code_context>

<specifics>
## Specific Ideas

No specific requirements — open to standard approaches

</specifics>

<deferred>
## Deferred Ideas

- Move assembly.rs path functions (resolve_app_dirs, resolve_app_paths, apply_profile_suffix, get_storage_paths) to uc-bootstrap — Phase 40
- Move load_config to uc-bootstrap — Phase 40
- Move config_resolution module to uc-bootstrap — Phase 40
- Tracing initialization extraction from main.rs — Phase 40
- Config validation layer (port range, path existence) — future enhancement

</deferred>

---

_Phase: 39-config-resolution-extraction_
_Context gathered: 2026-03-18_
_Revised: 2026-03-18 (scope narrowed per review: no uc-app migration, no assembly.rs function moves)_

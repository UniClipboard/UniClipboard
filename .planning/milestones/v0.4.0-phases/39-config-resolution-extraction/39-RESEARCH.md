# Phase 39: Config Resolution Extraction - Research

**Researched:** 2026-03-18
**Domain:** Rust module extraction / bootstrap refactoring within uc-tauri
**Confidence:** HIGH

## Summary

Phase 39 is a focused Rust refactoring inside a single crate (`uc-tauri`). All the functions to be extracted already exist and are well-understood. The work involves:

1. Creating `src-tauri/crates/uc-tauri/src/bootstrap/config_resolution.rs` as a new module
2. Moving `resolve_config_path()` from `main.rs` into that module as a `pub fn`
3. Composing a new `pub fn resolve_app_config() -> Result<AppConfig, ConfigResolutionError>` that encapsulates the current main() config-loading block
4. Deleting two duplicate functions from `main.rs` (`apply_profile_suffix`, `resolve_keyslot_store_vault_dir`) and the inline key_slot_store path resolution block (lines 540-554)
5. Reordering `run_app()` so `get_storage_paths()` is called before `key_slot_store` construction, then using `storage_paths.vault_dir`
6. Migrating the two `resolve_config_path` tests from `main.rs::tests` to the new module, preserving the `CWD_TEST_LOCK` mutex pattern

No new dependencies are required. All types already exist in the crate graph. The phase is purely a mechanical extraction with no behavioral change.

**Primary recommendation:** Create the new module as a single flat file (`config_resolution.rs`) alongside the existing bootstrap modules, expose its public API via `bootstrap/mod.rs` re-exports, and make `main()` a thin delegator to `resolve_app_config()`.

<user_constraints>

## User Constraints (from CONTEXT.md)

### Locked Decisions

**Target module location: uc-tauri/src/bootstrap/config_resolution.rs**

The new module lives in `uc-tauri/src/bootstrap/` alongside assembly.rs and config.rs. It is NOT in uc-app.

**Rationale:**

- uc-app does not depend on uc-platform (production deps). The config loading fallback needs `DirsAppDirsAdapter` (uc-platform) for system default dirs. Putting this in uc-app would violate the layering constraint `uc-app → uc-core ← uc-platform`.
- assembly.rs already contains the authoritative path resolution functions. The new module sits alongside it in the same bootstrap namespace.
- Phase 40 creates uc-bootstrap which will pull these functions out of uc-tauri into a proper composition root crate. This phase prepares clean extraction boundaries without prematurely breaking the crate graph.

**SC#1 partial satisfaction note:** The extracted functions are pure (take inputs, return outputs) and have no Tauri API calls, but they live in `uc-tauri` which has a crate-level `tauri` dependency. This means non-Tauri entry points cannot yet depend on them without pulling in tauri. This phase satisfies the "extract and purify" part of SC#1; true crate-level accessibility for daemon/CLI is delivered by Phase 40 (uc-bootstrap).

**resolve_app_config() API design**

```rust
pub fn resolve_app_config() -> Result<AppConfig, ConfigResolutionError>
```

Returns `Result`, NOT a bare `AppConfig`. main.rs `main()` becomes:

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

**Deduplication: main.rs functions to delete**

| main.rs function                                 | Why delete                                                          | Replacement                                              |
| ------------------------------------------------ | ------------------------------------------------------------------- | -------------------------------------------------------- |
| `apply_profile_suffix()` (line 377)              | Duplicate of assembly.rs:632 — missing slash sanitization           | assembly.rs version (already `pub(crate)`)               |
| `resolve_keyslot_store_vault_dir()` (line 393)   | Subsumed by `resolve_app_paths()` vault_dir logic                   | `AppPaths::vault_dir` from `get_storage_paths()`         |
| inline key_slot_store resolution (lines 540-554) | Manual re-derivation of paths already computed by get_storage_paths | `storage_paths.vault_dir` (already computed at line 557) |

After deduplication, main.rs loses ~80 lines.

**Key slot store resolution consolidation**

Replace lines 540-554 with:

```rust
let storage_paths = get_storage_paths(&config).expect("failed to get storage paths");
let key_slot_store: Arc<dyn KeySlotStore> = Arc::new(JsonKeySlotStore::new(storage_paths.vault_dir.clone()));
```

**resolve_config_path() stays pure**

Pure function that:

1. Checks `UC_CONFIG_PATH` env var
2. Walks directory ancestors looking for `config.toml` / `src-tauri/config.toml`
3. Returns `Option<PathBuf>`

No dependencies beyond `std`. Trivially testable.

**Existing test migration**

- `test_cors_headers_*` — stay in main.rs (they test CORS, not config resolution)
- `test_resolve_config_path_finds_parent_directory` → new module
- `test_resolve_config_path_finds_src_tauri_config_from_repo_root` → new module

Preserve CWD_TEST_LOCK pattern — the static Mutex serializing CWD manipulation tests must be kept.

**ConfigResolutionError type**

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

### Deferred Ideas (OUT OF SCOPE)

- Move assembly.rs path functions (resolve_app_dirs, resolve_app_paths, apply_profile_suffix, get_storage_paths) to uc-bootstrap — Phase 40
- Move load_config to uc-bootstrap — Phase 40
- Move config_resolution module to uc-bootstrap — Phase 40
- Tracing initialization extraction from main.rs — Phase 40
- Config validation layer (port range, path existence) — future enhancement
  </user_constraints>

<phase_requirements>

## Phase Requirements

| ID      | Description                                                                                                              | Research Support                                                                                                                                                                           |
| ------- | ------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| RNTM-03 | Configuration resolution functions (path resolve, profile suffix, keyslot dir) extracted from main.rs to reusable module | New `config_resolution.rs` module with `resolve_config_path()` and `resolve_app_config()` as public free functions; duplicate functions deleted from main.rs; tests migrated to new module |

</phase_requirements>

## Standard Stack

### Core

| Library                                     | Version   | Purpose                                         | Why Standard                                              |
| ------------------------------------------- | --------- | ----------------------------------------------- | --------------------------------------------------------- |
| `std` (Rust stdlib)                         | —         | `PathBuf`, `env::var`, `current_dir`            | No external deps needed for pure path logic               |
| `anyhow`                                    | workspace | Error chaining in `load_config` (already used)  | Already a workspace dep; consistent error context pattern |
| `uc_core::config::AppConfig`                | workspace | Config DTO, `AppConfig::with_system_defaults()` | Established config type                                   |
| `uc_platform::app_dirs::DirsAppDirsAdapter` | workspace | Platform dirs for system-default fallback       | Already used in main.rs at line 462                       |

### Supporting

| Library            | Version   | Purpose                                           | When to Use                                                 |
| ------------------ | --------- | ------------------------------------------------- | ----------------------------------------------------------- |
| `tempfile`         | workspace | Temp dirs for `resolve_config_path` tests         | Required by migrated tests                                  |
| `std::sync::Mutex` | —         | `CWD_TEST_LOCK` static for CWD test serialization | Required by migrated tests that manipulate env::current_dir |

**No new Cargo.toml dependencies are required.** All types are already in the workspace.

## Architecture Patterns

### Recommended Project Structure

New file to create:

```
src-tauri/crates/uc-tauri/src/bootstrap/
├── assembly.rs          # (unchanged) authoritative path resolution, get_storage_paths
├── config.rs            # (unchanged) load_config — pure TOML loading
├── config_resolution.rs # NEW — resolve_config_path, resolve_app_config, ConfigResolutionError
├── mod.rs               # add new module decl + re-exports
└── ...
```

### Pattern 1: Free Functions in a New Module File

**What:** `config_resolution.rs` contains free functions (`pub fn resolve_config_path()`, `pub fn resolve_app_config()`) and a `pub enum ConfigResolutionError`. No struct wrapper needed.

**When to use:** Functions have no shared mutable state. Each is independently callable. A struct would add noise without benefit.

**Example (resolve_app_config body):**

```rust
// config_resolution.rs
use std::path::PathBuf;
use uc_core::config::AppConfig;
use crate::bootstrap::config::load_config;

pub enum ConfigResolutionError {
    InvalidConfig { path: PathBuf, source: anyhow::Error },
    PlatformDirsFailed { source: anyhow::Error },
}

impl std::fmt::Display for ConfigResolutionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidConfig { path, source } =>
                write!(f, "Config file '{}' is invalid: {}", path.display(), source),
            Self::PlatformDirsFailed { source } =>
                write!(f, "Platform directory resolution failed: {}", source),
        }
    }
}

pub fn resolve_app_config() -> Result<AppConfig, ConfigResolutionError> {
    let config_path = resolve_config_path().unwrap_or_else(|| PathBuf::from("config.toml"));

    match load_config(config_path.clone()) {
        Ok(config) => {
            tracing::info!("Loaded config from {} (development mode)", config_path.display());
            Ok(config)
        }
        Err(e) => {
            // Check if file was found but malformed vs simply not present
            if config_path.is_file() {
                return Err(ConfigResolutionError::InvalidConfig { path: config_path, source: e });
            }
            tracing::debug!("No config.toml found, using system defaults: {}", e);
            let app_dirs = uc_platform::app_dirs::DirsAppDirsAdapter::new()
                .get_app_dirs()
                .map_err(|e| ConfigResolutionError::PlatformDirsFailed {
                    source: anyhow::anyhow!("{}", e),
                })?;
            Ok(AppConfig::with_system_defaults(app_dirs.app_data_root))
        }
    }
}
```

### Pattern 2: mod.rs Re-export

**What:** Add `pub mod config_resolution;` and a re-export of `resolve_app_config` + `ConfigResolutionError` in `bootstrap/mod.rs`.

**Example:**

```rust
// bootstrap/mod.rs — additions
pub mod config_resolution;
pub use config_resolution::{resolve_app_config, ConfigResolutionError};
```

### Pattern 3: CWD_TEST_LOCK for Serialized CWD Tests

**What:** Static `Mutex<()>` guards tests that call `std::env::set_current_dir()`, preventing parallel test runs from corrupting each other's working directory.

**Example (migrated from main.rs):**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::{env, fs, sync::Mutex};
    use tempfile::TempDir;

    static CWD_TEST_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn test_resolve_config_path_finds_parent_directory() {
        let _guard = CWD_TEST_LOCK.lock().unwrap();
        // ... same body as current main.rs:312-329
    }
}
```

### Pattern 4: run_app() Key Slot Store Consolidation

**What:** Move `get_storage_paths()` call before `key_slot_store` construction and use `storage_paths.vault_dir` instead of the inline manual derivation.

**Before (main.rs lines 540-557):**

```rust
// lines 540-554: manual inline resolution using apply_profile_suffix + resolve_keyslot_store_vault_dir
let key_slot_store: Arc<dyn KeySlotStore> = {
    let app_data_root = ...;
    let vault_dir = resolve_keyslot_store_vault_dir(&config, app_data_root);
    Arc::new(JsonKeySlotStore::new(vault_dir))
};

// line 557: LATER — storage_paths also computes vault_dir
let storage_paths = get_storage_paths(&config).expect("...");
```

**After:**

```rust
// storage_paths first — includes vault_dir
let storage_paths = get_storage_paths(&config).expect("failed to get storage paths");
let key_slot_store: Arc<dyn KeySlotStore> =
    Arc::new(JsonKeySlotStore::new(storage_paths.vault_dir.clone()));
```

### Anti-Patterns to Avoid

- **Putting config_resolution in uc-app:** uc-app has no uc-platform dependency (production). DirsAppDirsAdapter is in uc-platform. Violates layer constraint.
- **Moving assembly.rs functions:** Out of scope for Phase 39. They stay where they are.
- **Using `anyhow::Error` directly as the return type of resolve_app_config:** The CONTEXT.md specifies a structured enum so callers can distinguish fatal vs recoverable errors.
- **Leaving apply_profile_suffix in main.rs after extracting config_resolution:** The main.rs copy lacks slash sanitization. Delete it; let run_app() use assembly.rs's `pub(crate)` version if needed (or if run_app() no longer calls it directly, it simply vanishes with the deleted function).

## Don't Hand-Roll

| Problem                      | Don't Build                          | Use Instead                                               | Why                                                                                                                   |
| ---------------------------- | ------------------------------------ | --------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------- |
| Platform data dir resolution | Custom platform detection            | `DirsAppDirsAdapter::new().get_app_dirs()`                | Already handles macOS/Linux/Windows differences                                                                       |
| Vault dir path computation   | New manual path join logic           | `get_storage_paths(&config).vault_dir`                    | `resolve_app_paths()` already handles all cases: empty vault_key_path, relative db root, profile suffix, in-memory db |
| Profile suffix application   | Re-implement in config_resolution.rs | `assembly::apply_profile_suffix()` (already `pub(crate)`) | Authoritative version has slash sanitization that main.rs copy lacks                                                  |

**Key insight:** The infrastructure for path resolution already exists in assembly.rs. Phase 39 is about removing duplication and providing a composing entry point — not rebuilding logic.

## Common Pitfalls

### Pitfall 1: Calling get_storage_paths Before Moving It

**What goes wrong:** `run_app()` currently computes `key_slot_store` at lines 540-554 using `apply_profile_suffix` and `resolve_keyslot_store_vault_dir`, then calls `get_storage_paths()` at line 557. If the planner leaves the ordering unchanged and only replaces the inline block, `storage_paths` won't exist at the `key_slot_store` construction site yet.

**Why it happens:** The inline block came before `get_storage_paths` call in the original code. Naive replacement without reordering will fail to compile.

**How to avoid:** Move the `get_storage_paths()` call to before `key_slot_store` construction in `run_app()`. Use the same `storage_paths` instance for both `key_slot_store` (via `.vault_dir`) and any subsequent uses (already at line 557+).

**Warning signs:** Compiler error `use of undeclared variable storage_paths` when building run_app.

### Pitfall 2: Breaking CWD Test Isolation

**What goes wrong:** `resolve_config_path()` calls `std::env::current_dir()`. If the migrated tests don't preserve the `CWD_TEST_LOCK` static Mutex pattern, parallel tests that manipulate `current_dir` will interfere, producing flaky failures.

**Why it happens:** Rust test runner uses multiple threads. `current_dir` is a process-wide global.

**How to avoid:** Copy the `static CWD_TEST_LOCK: Mutex<()> = Mutex::new(());` pattern from main.rs tests verbatim into the new module's test block.

**Warning signs:** Tests pass individually (`cargo test test_name`) but fail when run in parallel.

### Pitfall 3: Silencing the InvalidConfig vs FileNotFound Distinction

**What goes wrong:** The current main.rs `main()` calls `load_config(config_path)` and treats ALL errors as "no config file found, use system defaults". This silently swallows a malformed config.toml.

**Why it happens:** `load_config` returns `anyhow::Error` for both I/O errors (file not found) and parse errors (bad TOML). The current code doesn't distinguish them.

**How to avoid:** In `resolve_app_config()`, check `config_path.is_file()` before deciding how to handle the error. If the file exists and load_config failed, it's `InvalidConfig` (fatal). If the file doesn't exist, it's the expected dev fallback.

**Warning signs:** A malformed config.toml silently causes the app to start with system defaults instead of exiting with an error message.

### Pitfall 4: Leaving apply_profile_suffix Accessible in main.rs Scope

**What goes wrong:** If `apply_profile_suffix` in main.rs is deleted but `run_app()` still calls it (before the inline vault dir block is also removed), the code won't compile.

**Why it happens:** The inline `key_slot_store` block at lines 540-554 calls both `apply_profile_suffix` and `resolve_keyslot_store_vault_dir`. Both must be deleted together.

**How to avoid:** Delete the entire lines 540-554 block as a unit, then delete both function definitions. The replacement with `storage_paths.vault_dir` doesn't call either.

**Warning signs:** `error[E0425]: cannot find function apply_profile_suffix in this scope` or `resolve_keyslot_store_vault_dir`.

### Pitfall 5: Breaking the app_dirs Usage in run_app()

**What goes wrong:** `run_app()` currently resolves `app_dirs` (via `DirsAppDirsAdapter`) at line 532 for use in the key_slot_store block. After the inline block is removed, this `app_dirs` variable may become unused if nothing else in `run_app()` consumes it.

**Why it happens:** The `app_dirs` resolved at line 532 feeds into the manual `app_data_root` computation that disappears with the inline block deletion.

**How to avoid:** Check whether `app_dirs` is used elsewhere in `run_app()` after the inline block. If the only consumer was lines 540-554, remove the `app_dirs` resolution at line 532 too to avoid `unused variable` warnings (or errors in strict mode).

## Code Examples

### resolve_config_path (extracted as-is from main.rs:352-375)

```rust
// Source: src-tauri/src/main.rs:352-375 (authoritative — extracted verbatim)
pub fn resolve_config_path() -> Option<PathBuf> {
    if let Ok(explicit) = std::env::var("UC_CONFIG_PATH") {
        let explicit_path = PathBuf::from(explicit);
        if explicit_path.is_file() {
            return Some(explicit_path);
        }
    }

    let current_dir = std::env::current_dir().ok()?;

    for ancestor in current_dir.ancestors() {
        let candidate = ancestor.join("config.toml");
        if candidate.is_file() {
            return Some(candidate);
        }

        let src_tauri_candidate = ancestor.join("src-tauri").join("config.toml");
        if src_tauri_candidate.is_file() {
            return Some(src_tauri_candidate);
        }
    }

    None
}
```

### assembly.rs apply_profile_suffix (authoritative version — DO NOT duplicate)

```rust
// Source: src-tauri/crates/uc-tauri/src/bootstrap/assembly.rs:632-646
pub(crate) fn apply_profile_suffix(path: PathBuf) -> PathBuf {
    let profile = match std::env::var("UC_PROFILE") {
        Ok(value) if !value.is_empty() => value.replace('/', "_").replace('\\', "_"),
        _ => return path,
    };
    let file_name = match path.file_name().and_then(|name| name.to_str()) {
        Some(name) => name.to_string(),
        None => return path,
    };
    let mut updated = path;
    updated.set_file_name(format!("{file_name}_{profile}"));
    updated
}
```

The main.rs version (line 377) is MISSING the `.replace('/', "_").replace('\\', "_")` sanitization. This is why it must be deleted, not migrated.

### get_storage_paths (returns AppPaths including vault_dir)

```rust
// Source: src-tauri/crates/uc-tauri/src/bootstrap/assembly.rs:544-549
pub fn get_storage_paths(config: &AppConfig) -> WiringResult<AppPaths> {
    let platform_dirs = get_default_app_dirs()?;
    resolve_app_paths(&platform_dirs, config)
}
// AppPaths.vault_dir is set at assembly.rs:602-626 — handles all vault_key_path cases
```

### AppConfig::with_system_defaults (system-default fallback constructor)

```rust
// Source: src-tauri/crates/uc-core/src/config/mod.rs:127-136
pub fn with_system_defaults(data_dir: PathBuf) -> Self {
    Self {
        device_name: String::new(),
        vault_key_path: data_dir.join("vault/key"),
        vault_snapshot_path: data_dir.join("vault/snapshot"),
        webserver_port: 0,
        database_path: data_dir.join("uniclipboard.db"),
        silent_start: false,
    }
}
```

## State of the Art

| Old Approach                              | Current Approach                                   | When Changed | Impact                                                               |
| ----------------------------------------- | -------------------------------------------------- | ------------ | -------------------------------------------------------------------- |
| Inline path resolution in main()          | Delegating to `resolve_app_config()` in new module | Phase 39     | main.rs shrinks ~80 lines; logic becomes unit-testable without Tauri |
| Duplicate apply_profile_suffix in main.rs | Single authoritative copy in assembly.rs           | Phase 39     | Eliminates missing slash-sanitization bug in main.rs copy            |
| Manual vault_dir derivation in run_app()  | `get_storage_paths().vault_dir`                    | Phase 39     | Single path derivation code path; eliminates divergence risk         |

**Deprecated/outdated after this phase:**

- `main.rs::apply_profile_suffix()` (line 377): deleted, use `assembly::apply_profile_suffix`
- `main.rs::resolve_keyslot_store_vault_dir()` (line 393): deleted, subsumed by `get_storage_paths`
- Inline key_slot_store path block (main.rs lines 540-554): deleted, replaced with `storage_paths.vault_dir`

## Open Questions

1. **apply_profile_suffix visibility after deletion from main.rs**
   - What we know: `apply_profile_suffix` in assembly.rs is `pub(crate)`, so it's accessible within `uc-tauri` but not from `main.rs` (which is in the `src-tauri` binary crate, not `uc-tauri`)
   - What's unclear: Does run_app() in main.rs need to call `apply_profile_suffix` after the inline block is removed? If not, no visibility change needed. If yes, `assembly::apply_profile_suffix` would need to become `pub`.
   - Recommendation: The inline block deletion eliminates the only call site of `apply_profile_suffix` in `run_app()`. The function does not need to be pub. Verify after deletion.

2. **app_dirs resolution at main.rs:532 after inline block removed**
   - What we know: `app_dirs` at line 532 is used only by the inline `key_slot_store` block (lines 540-554)
   - What's unclear: Are there other consumers of `app_dirs` in `run_app()` below line 557?
   - Recommendation: Search `run_app()` for other `app_dirs` uses before removing the resolution call. If none, remove lines 532-538 to avoid unused variable.

## Validation Architecture

### Test Framework

| Property           | Value                                                      |
| ------------------ | ---------------------------------------------------------- |
| Framework          | Rust built-in test harness (cargo test)                    |
| Config file        | `src-tauri/Cargo.toml` (workspace)                         |
| Quick run command  | `cd src-tauri && cargo test -p uc-tauri config_resolution` |
| Full suite command | `cd src-tauri && cargo test -p uc-tauri`                   |

### Phase Requirements → Test Map

| Req ID  | Behavior                                                                 | Test Type | Automated Command                                                                                       | File Exists?                     |
| ------- | ------------------------------------------------------------------------ | --------- | ------------------------------------------------------------------------------------------------------- | -------------------------------- |
| RNTM-03 | `resolve_config_path` finds config.toml in parent dir                    | unit      | `cd src-tauri && cargo test -p uc-tauri test_resolve_config_path_finds_parent_directory`                | ❌ Wave 0 (migrate from main.rs) |
| RNTM-03 | `resolve_config_path` finds src-tauri/config.toml from repo root         | unit      | `cd src-tauri && cargo test -p uc-tauri test_resolve_config_path_finds_src_tauri_config_from_repo_root` | ❌ Wave 0 (migrate from main.rs) |
| RNTM-03 | `resolve_app_config` returns system defaults when no config.toml present | unit      | `cd src-tauri && cargo test -p uc-tauri resolve_app_config_returns_system_defaults`                     | ❌ Wave 0 (new test)             |
| RNTM-03 | GUI app compiles and `run_app()` uses `storage_paths.vault_dir`          | smoke     | `cd src-tauri && cargo check`                                                                           | ❌ Wave 0 (existing infra)       |

### Sampling Rate

- **Per task commit:** `cd src-tauri && cargo test -p uc-tauri config_resolution`
- **Per wave merge:** `cd src-tauri && cargo test -p uc-tauri`
- **Phase gate:** Full suite green before `/gsd:verify-work`

### Wave 0 Gaps

- [ ] `src-tauri/crates/uc-tauri/src/bootstrap/config_resolution.rs` — create module with migrated + new tests
- [ ] Migrate `test_resolve_config_path_finds_parent_directory` from `src-tauri/src/main.rs` to new module
- [ ] Migrate `test_resolve_config_path_finds_src_tauri_config_from_repo_root` from `src-tauri/src/main.rs` to new module
- [ ] Add `test_resolve_app_config_returns_system_defaults_when_no_config_file` in new module

## Sources

### Primary (HIGH confidence)

- `src-tauri/src/main.rs:352-480` — exact functions to extract and exact inline block to replace
- `src-tauri/crates/uc-tauri/src/bootstrap/assembly.rs:544-646` — authoritative `get_storage_paths`, `apply_profile_suffix`, `resolve_app_paths`
- `src-tauri/crates/uc-tauri/src/bootstrap/config.rs` — `load_config` signature and behavior
- `src-tauri/crates/uc-core/src/config/mod.rs` — `AppConfig`, `AppConfig::with_system_defaults`
- `src-tauri/crates/uc-app/src/app_paths.rs` — `AppPaths` struct with `vault_dir: PathBuf`
- `src-tauri/crates/uc-platform/src/app_dirs.rs` — `DirsAppDirsAdapter::new().get_app_dirs()`
- `src-tauri/crates/uc-tauri/src/bootstrap/mod.rs` — existing re-export pattern to follow
- `.planning/phases/39-config-resolution-extraction/39-CONTEXT.md` — all locked decisions

### Secondary (MEDIUM confidence)

- `src-tauri/crates/uc-tauri/src/bootstrap/wiring.rs:5156-5313` — existing test patterns (with_uc_profile, resolve_app_paths tests, apply_profile_suffix sanitization test) demonstrating the test style used in this codebase

## Metadata

**Confidence breakdown:**

- Standard stack: HIGH — all deps already exist in workspace, verified by reading source files
- Architecture: HIGH — extraction targets and authoritative replacements verified from source
- Pitfalls: HIGH — identified from direct reading of the code (CWD lock, ordering, duplicate apply_profile_suffix, unused app_dirs)

**Research date:** 2026-03-18
**Valid until:** Stable — pure Rust refactoring with no external API surface changes

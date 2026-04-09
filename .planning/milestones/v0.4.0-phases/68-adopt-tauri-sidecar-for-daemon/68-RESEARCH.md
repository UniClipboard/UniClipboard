# Phase 68: Adopt Tauri Sidecar for Daemon - Research

**Researched:** 2026-03-28
**Domain:** Tauri v2 sidecar / externalBin, Rust build scripts, subprocess management
**Confidence:** HIGH

<user_constraints>

## User Constraints (from CONTEXT.md)

### Locked Decisions

- **D-01:** Use Tauri `externalBin` in `tauri.conf.json` to declare daemon as a sidecar binary. Tauri handles platform-specific target-triple naming and inclusion in app bundle.
- **D-02:** Binary naming follows Tauri sidecar convention: `uniclipboard-daemon-{target-triple}` (e.g., `uniclipboard-daemon-aarch64-apple-darwin`).
- **D-03:** Replace `std::process::Command` in `run.rs` with Tauri sidecar API (`app.shell().sidecar("uniclipboard-daemon")`). Delete custom `resolve_daemon_binary_path()` and `daemon_binary_name()` functions.
- **D-04:** Pass `--gui-managed` argument via sidecar `.args()`.
- **D-05:** Use `build.rs` in uc-tauri to copy the compiled daemon binary from `target/{profile}/uniclipboard-daemon` to `src-tauri/binaries/uniclipboard-daemon-{target-triple}` after build. Tauri's workspace cargo build already compiles all workspace members including uc-daemon.
- **D-06:** Migrate existing stdin pipe tether (GUI-managed daemon shutdown) to sidecar `CommandChild` stdin. The sidecar API supports `.write(bytes)` for async stdin communication. Daemon-side stdin monitoring logic (`--gui-managed` mode) remains unchanged.
- **D-07:** CLI (`uc-cli`) path resolution stays as-is in this phase.

### Claude's Discretion

- Exact `build.rs` implementation details (how to detect target triple, copy logic)
- Whether to use `tauri-plugin-shell` permissions model or bypass for sidecar
- Supervision loop adaptation details (sidecar `CommandChild` vs std `Child` for health monitoring)

### Deferred Ideas (OUT OF SCOPE)

- Unified CLI distribution via Homebrew — single `uniclipboard` command, daemon binary bundled inside.
- CLI daemon path resolution unification — extract shared resolve module to uc-daemon-client when CLI distribution is addressed.
  </user_constraints>

---

## Summary

Phase 68 migrates the daemon launch mechanism from manual `std::process::Command` with custom path resolution to Tauri's first-class sidecar API (`tauri-plugin-shell`). This gives Tauri ownership of binary path resolution, cross-platform bundling, and macOS code signing of the daemon binary.

The implementation has three sub-problems: (1) declare the binary in `tauri.conf.json` as `externalBin`, (2) create a `build.rs` in `uc-tauri` that copies the compiled daemon into `src-tauri/binaries/` with the correct target-triple suffix so both dev and CI builds work, and (3) replace `spawn_daemon_process()` with `app.shell().sidecar("uniclipboard-daemon")` and adapt the `GuiOwnedDaemonState` to hold a `CommandChild` instead of `std::process::Child`.

The supervision loop (`supervise_daemon`) already uses an HTTP health-probe model (not `child.try_wait()`), so it does not fundamentally depend on the child process handle type. The main adaptation is holding the `CommandChild` for stdin writes (the `--gui-managed` tether) and for exit cleanup at app shutdown.

**Primary recommendation:** Add `tauri-plugin-shell = "2"` to both workspace `Cargo.toml` and `uniclipboard/Cargo.toml`; write a minimal `build.rs` in `uc-tauri` that reads `TAURI_ENV_TARGET_TRIPLE` (injected by Tauri CLI) or falls back to `CARGO_CFG_TARGET_ARCH`/`CARGO_CFG_TARGET_OS`; replace the spawn path in `run.rs`; update `GuiOwnedDaemonState` to hold `CommandChild`; add shell capability permission.

---

## Standard Stack

### Core

| Library            | Version | Purpose                                       | Why Standard                                                  |
| ------------------ | ------- | --------------------------------------------- | ------------------------------------------------------------- |
| tauri-plugin-shell | 2.3.5   | Sidecar launch API, CommandChild stdin/stdout | Official Tauri plugin; required for sidecar spawn in Tauri v2 |
| tauri-build        | 2.x     | Build-time Tauri code gen                     | Already in use; `build.rs` calls `tauri_build::build()`       |

**Version verified:** `cargo search tauri-plugin-shell --limit 1` returned `2.3.5` (2026-03-28).

**Installation:**

```bash
# In src-tauri/Cargo.toml (main uniclipboard binary) and workspace Cargo.toml
tauri-plugin-shell = "2"

# In src-tauri/crates/uc-tauri/Cargo.toml (where shell API is used)
tauri-plugin-shell = "2"
```

---

## Architecture Patterns

### Recommended Binary Layout

```
src-tauri/
├── binaries/                          # Sidecar staging directory (NEW)
│   └── uniclipboard-daemon-{triple}   # Created by build.rs, .gitignored
├── tauri.conf.json                    # Add bundle.externalBin
├── build.rs                           # Existing: only calls tauri_build::build()
│                                      # (No changes needed here — daemon copy
│                                      #  belongs in uc-tauri's build.rs)
└── crates/uc-tauri/
    ├── build.rs                       # NEW: copy daemon binary to binaries/
    └── src/bootstrap/run.rs           # MODIFY: replace spawn logic
```

### Pattern 1: Tauri externalBin Configuration

**What:** Declare daemon stem path in `tauri.conf.json` under `bundle.externalBin`.
**When to use:** Any binary that must be bundled into the app and launched at runtime.

```json
// src-tauri/tauri.conf.json
{
  "bundle": {
    "externalBin": ["binaries/uniclipboard-daemon"]
  }
}
```

The path `"binaries/uniclipboard-daemon"` is relative to `src-tauri/`. Tauri appends the target-triple suffix automatically — it expects to find `src-tauri/binaries/uniclipboard-daemon-{triple}` (or `.exe` on Windows).

### Pattern 2: build.rs Binary Copy (TAURI_ENV_TARGET_TRIPLE)

**What:** Copy compiled daemon to `src-tauri/binaries/` with target-triple name during Cargo build.
**When to use:** Any workspace member compiled binary that needs to become a Tauri sidecar.

The Tauri CLI injects `TAURI_ENV_TARGET_TRIPLE` into the build environment. This is the canonical way to determine the triple inside `build.rs`.

```rust
// src-tauri/crates/uc-tauri/build.rs (NEW FILE)
use std::path::PathBuf;

fn main() {
    // Determine target triple from Tauri CLI env var (set when building via `bun tauri build`)
    // Fall back to constructing from Cargo CFG vars for `cargo build` direct invocation.
    let target_triple = std::env::var("TAURI_ENV_TARGET_TRIPLE")
        .unwrap_or_else(|_| construct_triple_from_cfg());

    let profile = std::env::var("PROFILE").unwrap_or_else(|_| "debug".to_string());

    // Workspace root: go up from OUT_DIR (target/{profile}/build/uc-tauri-*/out)
    let out_dir = PathBuf::from(std::env::var("OUT_DIR").unwrap());
    // target/{profile}/build/uc-tauri-xxx/out -> target/{profile}
    let target_dir = out_dir
        .ancestors()
        .find(|p| p.ends_with(&profile))
        .unwrap_or_else(|| &out_dir);

    let daemon_src = target_dir.join("uniclipboard-daemon");
    #[cfg(target_os = "windows")]
    let daemon_src = daemon_src.with_extension("exe");

    // src-tauri/binaries/ is relative to manifest dir (uc-tauri crate root),
    // so go up to src-tauri/
    let manifest_dir = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let src_tauri_dir = manifest_dir
        .ancestors()
        .find(|p| p.file_name().map(|n| n == "src-tauri").unwrap_or(false))
        .expect("src-tauri directory not found in ancestors");
    let binaries_dir = src_tauri_dir.join("binaries");
    std::fs::create_dir_all(&binaries_dir).expect("failed to create binaries dir");

    let binary_ext = if cfg!(target_os = "windows") { ".exe" } else { "" };
    let dest_name = format!("uniclipboard-daemon-{}{}", target_triple, binary_ext);
    let dest = binaries_dir.join(&dest_name);

    if daemon_src.exists() {
        std::fs::copy(&daemon_src, &dest)
            .unwrap_or_else(|e| panic!("failed to copy daemon binary to {}: {}", dest.display(), e));
        println!("cargo:warning=Copied daemon binary to {}", dest.display());
    } else {
        // Not an error during first build (daemon may not be compiled yet)
        println!("cargo:warning=Daemon binary not found at {} (run cargo build first)", daemon_src.display());
    }

    // Re-run if daemon binary changes
    println!("cargo:rerun-if-changed={}", daemon_src.display());
}

fn construct_triple_from_cfg() -> String {
    let arch = std::env::var("CARGO_CFG_TARGET_ARCH").unwrap_or_default();
    let os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    let env = std::env::var("CARGO_CFG_TARGET_ENV").unwrap_or_default();
    match (arch.as_str(), os.as_str(), env.as_str()) {
        ("aarch64", "macos", _) => "aarch64-apple-darwin".to_string(),
        ("x86_64", "macos", _) => "x86_64-apple-darwin".to_string(),
        ("x86_64", "linux", "gnu") => "x86_64-unknown-linux-gnu".to_string(),
        ("x86_64", "windows", "msvc") => "x86_64-pc-windows-msvc".to_string(),
        _ => format!("{}-unknown-{}-{}", arch, os, env),
    }
}
```

**Key insight:** `TAURI_ENV_TARGET_TRIPLE` is set by the Tauri CLI during `bun tauri build` / `bun tauri dev`. For bare `cargo build` invocations (e.g., in `cargo test` runs or IDE), the fallback CFG construction covers the common platforms.

### Pattern 3: Sidecar Spawn via tauri-plugin-shell

**What:** Replace `std::process::Command` spawn with `app.shell().sidecar()`.
**When to use:** All daemon spawn sites in `run.rs`.

```rust
// Source: https://v2.tauri.app/develop/sidecar/
use tauri_plugin_shell::ShellExt;
use tauri_plugin_shell::process::{CommandChild, CommandEvent};

// In spawn_daemon_process(), takes &AppHandle<R> instead of no args
fn spawn_daemon_process<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
) -> Result<CommandChild, DaemonBootstrapError> {
    let (mut rx, child) = app
        .shell()
        .sidecar("uniclipboard-daemon")
        .map_err(|e| DaemonBootstrapError::Spawn(anyhow::Error::new(e)))?
        .args(["--gui-managed"])
        .spawn()
        .map_err(|e| DaemonBootstrapError::Spawn(anyhow::Error::new(e)))?;

    // Drain rx in background to prevent channel blocking
    tauri::async_runtime::spawn(async move {
        while let Some(_event) = rx.recv().await {}
    });

    Ok(child)
}
```

**Important distinction from std::process::Child:**

- `CommandChild` is from `tauri_plugin_shell::process` — NOT `std::process::Child`
- `CommandChild::write(bytes)` sends to stdin (async-safe, equivalent to piped stdin)
- `CommandChild` does NOT have `try_wait()` or `id()` — process management differs

### Pattern 4: stdin Tether via CommandChild::write

**What:** Send a sentinel byte to daemon's stdin to keep the tether alive (EOF on stdin triggers daemon shutdown).
**When to use:** When the GUI owns the daemon and wants the daemon to exit when the GUI exits.

```rust
// Write to sidecar stdin — equivalent to the old Stdio::piped pattern
child.write(b"\n").map_err(|e| {
    tracing::warn!(error = %e, "Failed to write to daemon stdin tether");
})?;

// On app exit: dropping CommandChild closes stdin, sending EOF to daemon
drop(child);
```

The daemon's existing stdin monitoring code (`--gui-managed` mode) remains unchanged. It waits for stdin EOF to trigger graceful shutdown.

### Pattern 5: Shell Permission in Capabilities

**What:** Grant `shell:allow-spawn` permission for the daemon sidecar.
**When to use:** Required by Tauri v2 capability system for any sidecar launch.

```json
// src-tauri/capabilities/default.json — add to "permissions" array
{
  "identifier": "shell:allow-spawn",
  "allow": [
    {
      "name": "binaries/uniclipboard-daemon",
      "sidecar": true,
      "args": ["--gui-managed"]
    }
  ]
}
```

Note: `shell:allow-spawn` (not `shell:allow-execute`) is required for long-running processes that return a `CommandChild`. Use `shell:allow-execute` only for processes that run and return output.

### Anti-Patterns to Avoid

- **Using the full path in `sidecar()`:** Call `sidecar("uniclipboard-daemon")` not `sidecar("binaries/uniclipboard-daemon")`. Tauri resolves the path internally from `externalBin` config.
- **Hardcoding target triple in `build.rs`:** Always read `TAURI_ENV_TARGET_TRIPLE` first; CFG vars are fallback only.
- **Calling `child.id()` on `CommandChild`:** `CommandChild` does not expose a PID accessor like `std::process::Child`. PID tracking in `GuiOwnedDaemonState` must change to store `CommandChild` directly (or obtain PID via other means if needed for termination).
- **Holding rx without draining:** The sidecar command's `rx` receiver must be consumed or drained. If rx is not polled, the sidecar's stdout buffer will block.
- **Not gitignoring `src-tauri/binaries/`:** The copied binary is a build artifact and should be `.gitignore`d.

---

## Don't Hand-Roll

| Problem                              | Don't Build                           | Use Instead                    | Why                                                                        |
| ------------------------------------ | ------------------------------------- | ------------------------------ | -------------------------------------------------------------------------- |
| Binary path resolution for daemon    | Custom `resolve_daemon_binary_path()` | `app.shell().sidecar()`        | Tauri handles platform-specific path, macOS notarization, bundle inclusion |
| Platform binary naming               | `daemon_binary_name()` with cfg flags | Tauri target-triple convention | Tauri bundler applies correct suffix per target                            |
| Cross-platform subprocess stdin pipe | `Stdio::piped()` + manual stdin write | `CommandChild::write()`        | Tauri plugin provides async-safe stdin API                                 |

**Key insight:** The existing `resolve_daemon_binary_path()` + `daemon_binary_name()` functions are a partial reimplementation of what Tauri's sidecar system provides natively — including macOS bundle resource path resolution, which the current code gets wrong in production bundles.

---

## Critical Type Migration: GuiOwnedDaemonState

The current `GuiOwnedDaemonState` holds `std::process::Child`. After the migration, it must hold `tauri_plugin_shell::process::CommandChild`. This is the most invasive change.

### What changes in `daemon_lifecycle.rs`:

| Current                        | After migration                                        |
| ------------------------------ | ------------------------------------------------------ |
| `pub child: Child` (std)       | `pub child: CommandChild` (tauri-plugin-shell)         |
| `child.id()` → PID u32         | CommandChild has no `.id()` — need alternative for PID |
| `child.try_wait()` → poll exit | CommandChild has no `.try_wait()`                      |
| `child.kill()` → force kill    | CommandChild has no `.kill()`                          |
| `child.wait()` → reap          | CommandChild has no `.wait()`                          |

### Recommended approach for PID / exit tracking:

The `shutdown_owned_daemon()` function in `daemon_lifecycle.rs` currently uses `child.try_wait()` / `child.kill()` / `child.wait()`. With `CommandChild`, these are not available.

**Option A (recommended):** Exit detection stays HTTP-probe-based (already is — `supervise_daemon` probes health endpoint). Exit cleanup uses `terminate_local_daemon_pid()` via the PID file (existing `read_pid_file()` infrastructure). The `CommandChild` is held only for stdin writes and dropped on exit.

**Why Option A is correct:** `supervise_daemon()` already does NOT poll `child.try_wait()`. It probes the HTTP health endpoint. Exit cleanup reads the PID from the daemon's PID file via `read_pid_file()` and calls `terminate_local_daemon_pid()`. The only remaining use of `Child` in `daemon_lifecycle.rs` is inside `shutdown_owned_daemon()`.

**Option B (alternative, more invasive):** Spawn the sidecar in a Tokio task and monitor the `CommandEvent::Terminated` event from `rx`, storing the result in an `AtomicBool`. This makes exit detection event-driven but requires restructuring the task ownership.

**Recommendation: Option A.** Adapt `GuiOwnedDaemonState` to store `CommandChild` instead of `OwnedDaemonChild` (which holds `std::process::Child`). For exit cleanup, rely on: (1) drop stdin by dropping `CommandChild` (sends EOF, daemon shuts down gracefully), then (2) wait for daemon absence via HTTP probe (already done). Remove the `child.try_wait()` / `child.kill()` / `child.wait()` polling loop from `shutdown_owned_daemon()` and replace with PID-file-based termination + HTTP absence poll.

---

## Common Pitfalls

### Pitfall 1: build.rs in wrong crate

**What goes wrong:** Placing the daemon copy logic in `src-tauri/build.rs` (the main `uniclipboard` crate) instead of `src-tauri/crates/uc-tauri/build.rs`.
**Why it happens:** The existing `src-tauri/build.rs` only calls `tauri_build::build()` — it is the canonical place. The context decision says to put the copy in `uc-tauri/build.rs`.
**How to avoid:** Create `src-tauri/crates/uc-tauri/build.rs` as a new file. The main `src-tauri/build.rs` remains unchanged (only `tauri_build::build()`).
**Warning signs:** Copy logic in `src-tauri/build.rs` will run before `tauri_build::build()` has set up paths, causing issues.

**Note:** An alternative approach is to add the copy logic directly to `src-tauri/build.rs` after `tauri_build::build()`. This is simpler because `TAURI_ENV_TARGET_TRIPLE` is set in that build context. The decision D-05 says "build.rs in uc-tauri" — this is valid but `TAURI_ENV_TARGET_TRIPLE` may not be propagated to crate-level build scripts (it's a Tauri CLI env var for the top-level build). The main `src-tauri/build.rs` is the safest location.

**Revised recommendation (Claude's Discretion):** Put the daemon copy in `src-tauri/build.rs` (main crate), not in `uc-tauri/build.rs`. This ensures `TAURI_ENV_TARGET_TRIPLE` is available (Tauri CLI sets it before invoking cargo build at the workspace root). The uc-tauri crate build.rs currently does not exist and creating one just for the copy adds complexity.

### Pitfall 2: CommandChild has no PID accessor

**What goes wrong:** Code tries to call `.id()` on `CommandChild` expecting a `u32` PID.
**Why it happens:** `std::process::Child` has `.id()`, `CommandChild` does not (at least not in the same API).
**How to avoid:** For PID-based operations (force kill), use `read_pid_file()` which reads the daemon's self-written PID file. This is already used by `terminate_incompatible_daemon_from_pid_file()`.
**Warning signs:** Compile error on `.id()` call; code that stores PID from child.id() at spawn time.

### Pitfall 3: rx channel not drained — sidecar blocks

**What goes wrong:** Sidecar stdout/stderr fills up and blocks because `rx` is never polled.
**Why it happens:** `CommandChild.spawn()` returns `(rx, child)`. If `rx` is dropped or never polled, the pipe buffer fills.
**How to avoid:** Always drain `rx` in a background `tauri::async_runtime::spawn`. The daemon's stdout/stderr go to null in production anyway, but the rx must be consumed.
**Warning signs:** Daemon appears to hang after startup; stdout output silently lost.

### Pitfall 4: Missing shell plugin initialization in main.rs

**What goes wrong:** `app.shell()` panics or returns error at runtime.
**Why it happens:** `tauri_plugin_shell::init()` must be registered with `.plugin()` in the builder before `app.shell()` can be called.
**How to avoid:** Add `.plugin(tauri_plugin_shell::init())` to the Tauri builder in `src-tauri/src/main.rs`.
**Warning signs:** Runtime panic or `shell not found` error.

### Pitfall 5: Capability permission missing for sidecar args

**What goes wrong:** Sidecar spawns but `--gui-managed` arg is rejected at runtime with capability error.
**Why it happens:** Tauri v2 capability system requires explicit arg allowlisting in the `shell:allow-spawn` permission scope.
**How to avoid:** Include `"args": ["--gui-managed"]` in the capability allow rule.
**Warning signs:** `IPC error: permission denied` or `command not allowed` in logs.

### Pitfall 6: build.rs not finding daemon binary on first build

**What goes wrong:** `bun tauri dev` fails because the copy step runs before uc-daemon is compiled.
**Why it happens:** Cargo build order: top-level crates build before their dependents, and `uc-tauri` is a dependency of the top-level crate but `uc-daemon` is a sibling — build order is not guaranteed.
**How to avoid:** Make the copy non-fatal if the source doesn't exist (emit `cargo:warning` but don't panic). The user must run `cd src-tauri && cargo build -p uc-daemon` before `bun tauri dev` on first setup, OR the `beforeDevCommand` script handles this. The copy in `build.rs` is idempotent on subsequent builds.
**Warning signs:** Build fails with "daemon binary not found" on a clean checkout.

### Pitfall 7: binaries/ directory not gitignored

**What goes wrong:** Built binaries are committed to git, causing CI bloat and cross-platform conflicts.
**How to avoid:** Add `src-tauri/binaries/` to `.gitignore`. Keep the directory itself under version control if needed with a `.gitkeep`.

### Pitfall 8: supervise_daemon calls spawn_daemon_process without AppHandle

**What goes wrong:** `supervise_daemon()` currently calls `spawn_daemon_process()` which takes no arguments. After migration, it needs access to `AppHandle` to call `app.shell().sidecar()`.
**Why it happens:** The AppHandle is not currently threaded through the supervision loop.
**How to avoid:** Add `AppHandle<R>` parameter to `supervise_daemon()` and `spawn_daemon_process()`. The `AppHandle` is `Clone` and can be passed from the `.setup()` closure.

---

## Code Examples

### Full spawn with arg and stdin tether

```rust
// Source: https://v2.tauri.app/develop/sidecar/
use tauri_plugin_shell::ShellExt;
use tauri_plugin_shell::process::{CommandChild, CommandEvent};

fn spawn_daemon_process<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
) -> Result<CommandChild, DaemonBootstrapError> {
    let (rx, child) = app
        .shell()
        .sidecar("uniclipboard-daemon")
        .map_err(|e| DaemonBootstrapError::Spawn(anyhow::Error::msg(format!("sidecar create: {e}"))))?
        .args(["--gui-managed"])
        .spawn()
        .map_err(|e| DaemonBootstrapError::Spawn(anyhow::Error::msg(format!("sidecar spawn: {e}"))))?;

    // Drain stdout/stderr events to prevent pipe blocking
    tauri::async_runtime::spawn(async move {
        let mut rx = rx;
        while let Some(event) = rx.recv().await {
            match event {
                CommandEvent::Stderr(line) => {
                    tracing::debug!(
                        line = %String::from_utf8_lossy(&line),
                        "daemon sidecar stderr"
                    );
                }
                CommandEvent::Terminated(payload) => {
                    tracing::info!(?payload, "daemon sidecar terminated");
                    break;
                }
                _ => {}
            }
        }
    });

    Ok(child)
}
```

### Capability JSON

```json
// src-tauri/capabilities/default.json — add to permissions array
{
  "identifier": "shell:allow-spawn",
  "allow": [
    {
      "name": "binaries/uniclipboard-daemon",
      "sidecar": true,
      "args": ["--gui-managed"]
    }
  ]
}
```

### tauri.conf.json externalBin

```json
{
  "bundle": {
    "active": true,
    "targets": "all",
    "externalBin": [
      "binaries/uniclipboard-daemon"
    ],
    ...
  }
}
```

### build.rs daemon copy (in src-tauri/build.rs)

```rust
fn main() {
    // Existing Tauri build step (must remain first)
    tauri_build::build();

    // Copy daemon binary to binaries/ staging dir for sidecar bundling
    copy_daemon_binary_to_binaries();
}

fn copy_daemon_binary_to_binaries() {
    // TAURI_ENV_TARGET_TRIPLE is set by Tauri CLI during bun tauri build/dev
    let target_triple = match std::env::var("TAURI_ENV_TARGET_TRIPLE") {
        Ok(triple) => triple,
        Err(_) => {
            // Fallback for bare cargo build (CI pre-build step, IDE)
            construct_triple_from_cfg()
        }
    };

    let profile = std::env::var("PROFILE").unwrap_or_else(|_| "debug".to_string());
    let manifest_dir = std::path::PathBuf::from(
        std::env::var("CARGO_MANIFEST_DIR").unwrap()
    );

    // From src-tauri/target/{profile}/uniclipboard-daemon
    let target_dir = manifest_dir.join("target").join(&profile);
    let binary_name = if cfg!(target_os = "windows") {
        "uniclipboard-daemon.exe"
    } else {
        "uniclipboard-daemon"
    };
    let src = target_dir.join(binary_name);

    // To src-tauri/binaries/uniclipboard-daemon-{triple}[.exe]
    let binaries_dir = manifest_dir.join("binaries");
    let _ = std::fs::create_dir_all(&binaries_dir);
    let ext = if cfg!(target_os = "windows") { ".exe" } else { "" };
    let dest_name = format!("uniclipboard-daemon-{}{}", target_triple, ext);
    let dest = binaries_dir.join(&dest_name);

    if src.exists() {
        if let Err(e) = std::fs::copy(&src, &dest) {
            eprintln!("cargo:warning=Failed to copy daemon binary: {}", e);
        } else {
            println!("cargo:warning=daemon binary staged to {}", dest.display());
        }
    } else {
        println!(
            "cargo:warning=Daemon binary not found at {} — run cargo build -p uc-daemon first",
            src.display()
        );
    }

    println!("cargo:rerun-if-changed={}", src.display());
}

fn construct_triple_from_cfg() -> String {
    let arch = std::env::var("CARGO_CFG_TARGET_ARCH").unwrap_or_default();
    let os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    let env = std::env::var("CARGO_CFG_TARGET_ENV").unwrap_or_default();
    match (arch.as_str(), os.as_str(), env.as_str()) {
        ("aarch64", "macos", _) => "aarch64-apple-darwin".to_string(),
        ("x86_64", "macos", _) => "x86_64-apple-darwin".to_string(),
        ("x86_64", "linux", "gnu") => "x86_64-unknown-linux-gnu".to_string(),
        ("x86_64", "windows", "msvc") => "x86_64-pc-windows-msvc".to_string(),
        _ => format!("{}-unknown-{}-{}", arch, os, env),
    }
}
```

---

## State of the Art

| Old Approach                          | Current Approach                 | When Changed    | Impact                                                      |
| ------------------------------------- | -------------------------------- | --------------- | ----------------------------------------------------------- |
| `std::process::Command` + manual path | `tauri-plugin-shell` sidecar API | Tauri v2 (2024) | macOS notarization, bundle path resolution handled by Tauri |
| `Stdio::piped()` stdin                | `CommandChild::write()`          | Tauri v2        | Async-safe stdin writes                                     |
| Manual target-triple suffix code      | Tauri `externalBin` convention   | Tauri v2        | Bundler handles platform-specific naming                    |

**Deprecated/outdated:**

- Manual `daemon_binary_name()` with `#[cfg(target_os = "windows")]`: replaced by Tauri bundler
- Manual `resolve_daemon_binary_path()` sibling lookup: replaced by sidecar path resolution
- `Stdio::null()` for stdout/stderr: replaced by rx channel drain (though behavior is equivalent)

---

## Open Questions

1. **CommandChild PID for exit detection**
   - What we know: `CommandChild` does not expose `.id()` like `std::process::Child`
   - What's unclear: Whether `tauri_plugin_shell::process::CommandChild` in v2.3.5 has a PID accessor (docs.rs page was not fully readable)
   - Recommendation: Rely on `read_pid_file()` for PID-based operations. The daemon writes its own PID file on startup — this is already used for incompatible daemon termination. Planner should add a fallback: if no PID file, use `terminate_local_daemon_pid()` from a stored PID obtained immediately after spawn.

2. **TAURI_ENV_TARGET_TRIPLE propagation to src-tauri/build.rs**
   - What we know: Tauri CLI sets `TAURI_ENV_TARGET_TRIPLE` as an env var during `bun tauri build`
   - What's unclear: Whether this env var is available to the top-level `src-tauri/build.rs` invoked by cargo
   - Recommendation: Test during implementation. If not available via `src-tauri/build.rs`, the fallback using `CARGO_CFG_TARGET_ARCH`/OS covers all 4 target platforms. The CFG fallback is sufficient for this project's target matrix.

3. **bun tauri dev first-run order**
   - What we know: `beforeDevCommand` in `tauri.conf.json` currently runs `bun run dev:sweep && bun run dev`
   - What's unclear: Whether the daemon binary will be compiled before Tauri processes `externalBin`
   - Recommendation: The `beforeDevCommand` or dev instructions should include `cd src-tauri && cargo build -p uc-daemon` OR the `build.rs` makes the missing binary non-fatal (warn only). The phase plan should include a note about first-run setup.

---

## Environment Availability

| Dependency              | Required By             | Available             | Version                     | Fallback            |
| ----------------------- | ----------------------- | --------------------- | --------------------------- | ------------------- |
| rustc                   | target triple detection | ✓                     | `aarch64-apple-darwin` host | —                   |
| tauri-plugin-shell      | sidecar spawn API       | ✗ (not in Cargo.toml) | 2.3.5 (latest)              | none — must add     |
| src-tauri/binaries/ dir | sidecar staging         | ✗ (dir doesn't exist) | —                           | created by build.rs |

**Missing dependencies with no fallback:**

- `tauri-plugin-shell` must be added to `src-tauri/Cargo.toml` and `src-tauri/crates/uc-tauri/Cargo.toml`
- `src-tauri/binaries/` directory must be created (build.rs does this automatically)

**Missing dependencies with fallback:**

- None

---

## Validation Architecture

### Test Framework

| Property           | Value                                    |
| ------------------ | ---------------------------------------- |
| Framework          | cargo test (Rust unit tests)             |
| Config file        | src-tauri/crates/uc-tauri/Cargo.toml     |
| Quick run command  | `cd src-tauri && cargo test -p uc-tauri` |
| Full suite command | `cd src-tauri && cargo test`             |

### Phase Requirements → Test Map

| Req ID  | Behavior                                         | Test Type           | Automated Command                                             | File Exists?                   |
| ------- | ------------------------------------------------ | ------------------- | ------------------------------------------------------------- | ------------------------------ |
| PH68-01 | externalBin declared in tauri.conf.json          | smoke (build check) | `cd src-tauri && cargo check`                                 | ✅ (tauri.conf.json exists)    |
| PH68-02 | build.rs copies daemon to binaries/              | unit                | `cd src-tauri && cargo build -p uc-tauri` (inspect binaries/) | ❌ Wave 0                      |
| PH68-03 | spawn_daemon_process uses sidecar API            | unit                | `cd src-tauri && cargo test -p uc-tauri -- spawn`             | ✅ (tests in run.rs)           |
| PH68-04 | supervise_daemon compiles with AppHandle         | compile             | `cd src-tauri && cargo check -p uc-tauri`                     | ✅                             |
| PH68-05 | Capability permission includes shell:allow-spawn | smoke               | `cd src-tauri && bun tauri build` (capability validation)     | ✅ (capabilities/default.json) |

### Sampling Rate

- **Per task commit:** `cd src-tauri && cargo check -p uc-tauri`
- **Per wave merge:** `cd src-tauri && cargo test -p uc-tauri`
- **Phase gate:** Full `cd src-tauri && cargo test` green before `/gsd:verify-work`

### Wave 0 Gaps

- [ ] `src-tauri/binaries/.gitkeep` — staging directory
- [ ] `src-tauri/crates/uc-tauri/build.rs` — daemon copy script (OR copy logic added to `src-tauri/build.rs`)

_(If no gaps: "None — existing test infrastructure covers all phase requirements")_

---

## Project Constraints (from CLAUDE.md)

- All Rust/Cargo commands MUST run from `src-tauri/` directory
- Never use `unwrap()` or `expect()` in production code (build.rs is build-time, exceptions acceptable with context)
- Use `tracing::warn!` (not `println!`) for daemon process errors; use `cargo:warning=` in build.rs
- `tauri-plugin-shell` not currently in any `Cargo.toml` — must be added
- Tauri state pattern: register new managed state with `.manage()` before app starts
- AppHandle is Clone — safe to pass through async boundaries

---

## Sources

### Primary (HIGH confidence)

- https://v2.tauri.app/develop/sidecar/ — externalBin config, binary naming, sidecar spawn Rust example
- https://v2.tauri.app/plugin/shell/ — tauri-plugin-shell setup, capabilities
- `cargo search tauri-plugin-shell --limit 1` — verified version 2.3.5 (2026-03-28)
- `rustc --print host-tuple` — verified dev machine triple: `aarch64-apple-darwin`

### Secondary (MEDIUM confidence)

- WebSearch results (multiple sources agreeing): `TAURI_ENV_TARGET_TRIPLE` env var injected by Tauri CLI into build environment; confirmed by Tauri v2 docs DeepWiki reference
- WebSearch: `CommandChild::write(bytes)` is the stdin API; `spawn()` returns `(rx, child)` where rx is `Receiver<CommandEvent>`
- WebSearch: `shell:allow-spawn` permission required for long-running sidecar processes (vs `shell:allow-execute` for one-shot)

### Tertiary (LOW confidence)

- Assumption: `TAURI_ENV_TARGET_TRIPLE` is available in `src-tauri/build.rs` (top-level crate) when invoked via `bun tauri build` — needs runtime verification
- Assumption: `CommandChild` in tauri-plugin-shell 2.3.5 does not expose `.id()` — needs verification against docs.rs source

---

## Metadata

**Confidence breakdown:**

- Standard stack: HIGH — tauri-plugin-shell version verified via cargo search
- Architecture patterns: HIGH — from official Tauri v2 docs
- Build.rs approach: MEDIUM — TAURI_ENV_TARGET_TRIPLE availability in crate build.rs not verified empirically
- CommandChild PID absence: MEDIUM — inferred from API surface, not explicitly confirmed
- Pitfalls: HIGH — derived from direct code analysis of existing run.rs + daemon_lifecycle.rs

**Research date:** 2026-03-28
**Valid until:** 2026-04-28 (stable Tauri v2 plugin APIs)

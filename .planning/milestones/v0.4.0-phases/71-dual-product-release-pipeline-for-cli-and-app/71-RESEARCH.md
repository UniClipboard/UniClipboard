# Phase 71: Dual-Product Release Pipeline for CLI and App - Research

**Researched:** 2026-03-28
**Domain:** CI/CD, GitHub Actions, Cargo cross-compilation, artifact distribution, version management
**Confidence:** HIGH

## Summary

The project currently ships a single Tauri GUI app through a well-structured release pipeline. Phase 71 adds a second product — the `uniclipboard-cli` binary — that must be built, packaged, and distributed alongside the app from the same repository. The CLI is a standalone Rust binary (no Tauri, no frontend), so its build path is a plain `cargo build --release -p uc-cli` invocation, and its distribution model is direct binary download (no installer, no update manifest).

The key challenge is that the CLI crate (`uc-cli`) has a **hardcoded version `"0.1.0"`** instead of inheriting `version.workspace = true`. This means version bumps to the workspace do not propagate to the CLI binary. The fix is mechanical but must happen in Phase 71. Once the CLI follows the workspace version, both products share a single version number throughout the release process.

All other pipeline changes are additive: a new reusable workflow builds CLI binaries per-platform and uploads them as artifacts, the release workflow downloads and includes those artifacts alongside Tauri bundles, the release notes template gains a CLI download section, and the R2 upload step handles both sets of files without ambiguity.

**Primary recommendation:** Align CLI to `version.workspace = true`, add a parallel `build-cli.yml` reusable workflow, update `release.yml` to call both build workflows and merge their artifacts, and extend `scripts/generate-release-notes.js` to emit a CLI section in release notes.

---

## Project Constraints (from CLAUDE.md)

- All Rust commands run from `src-tauri/`, never from project root.
- No `unwrap()` / `expect()` in production Rust code.
- No fixed pixel values in frontend styling (not applicable to this phase).
- Commit and tag conventions must be preserved.
- Package manager is Bun (not npm/yarn) for frontend; Cargo for Rust.

---

## Current Pipeline Archaeology

### Workflow Graph

```
prepare-release.yml   (manual trigger)
  └─ bumps version, creates release/vX.Y.Z branch, opens PR

tag-on-merge.yml      (PR closed trigger)
  └─ creates annotated tag vX.Y.Z, deletes release branch

release.yml           (tag push trigger OR manual trigger)
  ├─ validate job        (bumps version if manual, sets outputs)
  ├─ build job           (calls build.yml)
  └─ create-release job  (downloads artifacts, releases, uploads to R2)

build.yml             (reusable workflow_call + manual workflow_dispatch)
  └─ matrix: 4 platforms × tauri-action → src-tauri/target/**/release/bundle/**
```

### Key Artifact Path: App Build

`tauri-action` produces bundles in `src-tauri/target/{target}/release/bundle/`:

- macOS: `.dmg`, `UniClipboard.app.tar.gz`, `.sig`
- Linux: `.deb`, `.AppImage`, `.AppImage.tar.gz.sig`
- Windows: `.exe` (NSIS), `.msi`, `.nsis.zip.sig`

Upload artifact name pattern: `uniclipboard-{target}` (e.g., `uniclipboard-aarch64-apple-darwin`)

### Prepare-Release Commits These Files

```
git add package.json src-tauri/tauri.conf.json src-tauri/Cargo.toml src-tauri/Cargo.lock docs/changelog/
```

### Version Sources (Current State)

| File                                                 | Current Value              | Updated By                                                    |
| ---------------------------------------------------- | -------------------------- | ------------------------------------------------------------- |
| `package.json`                                       | `0.4.0-alpha.1`            | `scripts/bump-version.js`                                     |
| `src-tauri/tauri.conf.json`                          | `0.4.0-alpha.1`            | `scripts/bump-version.js`                                     |
| `src-tauri/Cargo.toml` `[package].version`           | `0.4.0-alpha.1`            | `scripts/bump-version.js`                                     |
| `src-tauri/Cargo.toml` `[workspace.package].version` | `0.4.0-alpha.1`            | propagated to workspace crates via `version.workspace = true` |
| `uc-daemon/Cargo.toml`                               | `version.workspace = true` | inherits workspace                                            |
| **`uc-cli/Cargo.toml`**                              | **`"0.1.0"` (HARDCODED)**  | **NOT bumped by any script**                                  |

### R2 Storage Structure

```
uniclipboard-releases/
├── manifests/
│   ├── stable.json
│   ├── alpha.json
│   ├── beta.json
│   └── rc.json
└── artifacts/
    └── v{VERSION}/
        ├── UniClipboard_aarch64-apple-darwin.dmg
        ├── UniClipboard_x86_64-apple-darwin.dmg
        ├── ...
        └── (all release bundles flat)
```

### Update Manifest Format (Tauri-specific)

`assemble-update-manifest.js` scans for `.sig` files and produces:

```json
{
  "version": "0.4.0-alpha.1",
  "notes": "...",
  "pub_date": "2026-...",
  "platforms": {
    "darwin-aarch64": { "signature": "...", "url": "..." },
    "darwin-x86_64": { "signature": "...", "url": "..." },
    "linux-x86_64": { "signature": "...", "url": "..." },
    "windows-x86_64": { "signature": "...", "url": "..." }
  }
}
```

This manifest is **App-only**. The CLI does not use Tauri's updater mechanism and does not need a manifest in this format. CLI users download binaries directly.

---

## Standard Stack

### Core

| Tool                             | Version     | Purpose                           | Why Standard          |
| -------------------------------- | ----------- | --------------------------------- | --------------------- |
| GitHub Actions                   | current     | CI/CD orchestration               | Already in use        |
| `cargo build --release`          | Rust stable | CLI binary compilation            | Native Rust toolchain |
| `actions/upload-artifact@v4`     | v4          | Artifact sharing between jobs     | Already in use        |
| `actions/download-artifact@v4`   | v4          | Artifact retrieval in release job | Already in use        |
| `softprops/action-gh-release@v2` | v2          | GitHub Release creation           | Already in use        |
| `Swatinem/rust-cache@v2`         | v2          | Rust build cache                  | Already in use        |
| `wrangler`                       | via npm     | R2 uploads                        | Already in use        |

### CLI Binary Naming Convention

Tauri uses target-triple suffixes for sidecars (`uniclipboard-daemon-aarch64-apple-darwin`). For direct CLI distribution, the convention is to include the target triple in the artifact filename:

```
uniclipboard-cli-{VERSION}-{target}.tar.gz   (macOS, Linux — tar.gz with the binary)
uniclipboard-cli-{VERSION}-{target}.zip      (Windows — zip with .exe)
```

Examples:

```
uniclipboard-cli-0.4.0-aarch64-apple-darwin.tar.gz
uniclipboard-cli-0.4.0-x86_64-apple-darwin.tar.gz
uniclipboard-cli-0.4.0-x86_64-unknown-linux-gnu.tar.gz
uniclipboard-cli-0.4.0-x86_64-pc-windows-msvc.zip
```

This pattern is consistent with what `rustup`, `cargo-binstall`, and similar tools expect.

### Alternatives Considered

| Instead of                | Could Use                                     | Tradeoff                                                                                 |
| ------------------------- | --------------------------------------------- | ---------------------------------------------------------------------------------------- |
| Single shared version     | CLI-specific semver                           | CLI at `version.workspace = true` is simpler — one version to manage, consistent tagging |
| tar.gz for all            | platform-native packages (.deb, brew formula) | Too early for package manager distribution; direct download is sufficient now            |
| Separate R2 prefix `cli/` | Flat `artifacts/v{VERSION}/`                  | Separate prefix avoids any filename collision with app artifacts                         |

---

## Architecture Patterns

### Recommended Pipeline Structure After Phase 71

```
prepare-release.yml
  └─ bump-version.js now also updates uc-cli/Cargo.toml

build.yml               (existing — unchanged)
  └─ builds Tauri app per platform

build-cli.yml           (NEW reusable workflow)
  └─ builds uniclipboard-cli per platform
  └─ uploads artifacts: cli-{target}

release.yml
  ├─ validate job (unchanged)
  ├─ build-app job → calls build.yml
  ├─ build-cli job → calls build-cli.yml (NEW)
  └─ create-release job
      ├─ downloads both sets of artifacts
      ├─ includes CLI binaries in GitHub Release files
      ├─ uploads CLI binaries to R2 under artifacts/v{VERSION}/
      ├─ generates update manifest (App only — unchanged)
      └─ generates release notes (with CLI section)
```

### Pattern 1: build-cli.yml as Reusable Workflow

Mirror the structure of `build.yml`. Key differences:

- No `bun install` / frontend step
- No `tauri-action` — plain `cargo build --release`
- Must build `uc-daemon` first (CLI sidecar dependency in production runs, but not needed for the CLI binary itself during build — the CLI binary embeds the daemon path at runtime, not at compile time)
- Produce one binary per platform, archive it, upload artifact

```yaml
# .github/workflows/build-cli.yml
name: 'Build CLI'
on:
  workflow_call:
    inputs:
      platform:
        required: false
        type: string
        default: 'all'
      branch:
        required: false
        type: string
        default: ''

jobs:
  setup-matrix:
    # Same matrix logic as build.yml
    ...

  build-cli:
    name: Build CLI ${{ matrix.target }}
    runs-on: ${{ matrix.platform }}
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - uses: Swatinem/rust-cache@v2
        with:
          workspaces: 'src-tauri -> target'
          shared-key: 'cli-${{ matrix.target }}'
      - name: install Linux deps
        if: matrix.platform == 'ubuntu-22.04'
        run: sudo apt-get install -y libwebkit2gtk-4.1-dev ... # needed for clipboard-rs
      - name: build CLI
        working-directory: src-tauri
        run: cargo build --release -p uc-cli ${{ matrix.args }}
      - name: package CLI binary
        shell: bash
        run: |
          VERSION=$(node -p "require('./package.json').version")
          TARGET="${{ matrix.target }}"
          BIN_NAME="uniclipboard-cli"
          if [ "${{ matrix.platform }}" = "windows-latest" ]; then
            7z a "uniclipboard-cli-${VERSION}-${TARGET}.zip" \
              "src-tauri/target/${TARGET}/release/${BIN_NAME}.exe"
          else
            tar -czf "uniclipboard-cli-${VERSION}-${TARGET}.tar.gz" \
              -C "src-tauri/target/${TARGET}/release" "${BIN_NAME}"
          fi
      - uses: actions/upload-artifact@v4
        with:
          name: cli-${{ matrix.target }}
          path: uniclipboard-cli-*.{tar.gz,zip}
```

### Pattern 2: Version Alignment

Fix `uc-cli/Cargo.toml` to use workspace version:

```toml
[package]
name = "uc-cli"
version.workspace = true   # was: version = "0.1.0"
edition = "2021"
```

Update `scripts/bump-version.js` to also update `uc-cli/Cargo.toml` in Cargo.lock. Since the workspace root `Cargo.toml` already uses `bump-version.js`'s `updateCargoToml` function (which updates `src-tauri/Cargo.toml` `[package].version` AND `[workspace.package].version`), and `uc-cli` will inherit via `version.workspace = true`, the Cargo.lock entry for `uc-cli` must also be updated.

The current `updateCargoLock()` function only patches the `uniclipboard` package entry in Cargo.lock. After setting `version.workspace = true` in `uc-cli`, the lock file will have a `uc-cli` entry that Cargo will update automatically on the next build. However, the CI version commit step must regenerate or patch the lock file correctly.

**Recommended approach:** After `bump-version.js` writes the new workspace version, run `cargo update --workspace --manifest-path src-tauri/Cargo.toml` in CI to regenerate Cargo.lock with the correct uc-cli version before committing.

### Pattern 3: Release Notes CLI Section

Extend `scripts/generate-release-notes.js` to detect CLI artifacts and emit download links:

```javascript
function buildCliInstallerLines({ artifactsDir, baseUrl }) {
  const macosArm64 = findFirstFile(artifactsDir,
    f => f.includes('aarch64-apple-darwin') && (f.endsWith('.tar.gz')))
  const macosX64 = findFirstFile(artifactsDir,
    f => f.includes('x86_64-apple-darwin') && f.endsWith('.tar.gz'))
  const linux = findFirstFile(artifactsDir,
    f => f.includes('linux-gnu') && f.endsWith('.tar.gz'))
  const windows = findFirstFile(artifactsDir,
    f => f.includes('windows-msvc') && f.endsWith('.zip'))
  ...
}
```

Add `{{CLI_SECTION}}` placeholder to `.github/release-notes/release.md.tmpl`.

### Pattern 4: R2 Upload Namespace

CLI artifacts go to the same R2 path structure as App artifacts:

```
artifacts/v{VERSION}/uniclipboard-cli-{VERSION}-{target}.tar.gz
```

The current release workflow uploads everything in `release-assets/` flat to `artifacts/v{VERSION}/`. Since CLI binary names (`uniclipboard-cli-*`) do not collide with App bundle names (`UniClipboard_*`, `*.dmg`, `*.deb`, etc.), no namespace prefix is needed. The flat structure works as-is.

### Pattern 5: build-cli Platform Dependencies

`uc-cli` depends on `uc-daemon` which depends on `uc-platform` which uses `clipboard-rs`. On Linux, `clipboard-rs` requires `libxcb` and related X11 dev packages (same as the Tauri app build). The Ubuntu build step must install the same apt packages.

### Anti-Patterns to Avoid

- **Hardcoded version in uc-cli**: Leave `version = "0.1.0"` — this will mean CLI binary shows wrong version. Must fix to `version.workspace = true`.
- **Building CLI inside Tauri action**: `tauri-action` wraps `bun tauri build`, which builds the full app. CLI must be built separately with `cargo build -p uc-cli`.
- **Uploading CLI to the updater manifest**: The Tauri updater manifest (`stable.json`) is app-only. CLI binaries must NOT be listed in it or `assemble-update-manifest.js` will fail/corrupt the manifest.
- **Not caching Rust between CLI and app jobs**: If both jobs share the same runner via matrix, the Rust build cache key should differentiate CLI vs App to avoid cache invalidation.
- **Forgetting the Windows binary extension**: The CLI binary on Windows is `uniclipboard-cli.exe`. The packaging step must handle the `.exe` extension.

---

## Don't Hand-Roll

| Problem                         | Don't Build                | Use Instead                                                         | Why                                                 |
| ------------------------------- | -------------------------- | ------------------------------------------------------------------- | --------------------------------------------------- |
| Cross-platform archive creation | Custom shell scripts       | `tar` (Unix) + `7z` (Windows, pre-installed on GH runners)          | Standard, already available on all GH runner images |
| Platform matrix management      | Separate jobs per platform | Reuse existing `setup-matrix` pattern from `build.yml`              | Already proven, consistent                          |
| Cargo.lock updates              | Custom regex in Node.js    | `cargo update --workspace`                                          | Cargo owns lock file format; regex is fragile       |
| Binary stripping                | Custom strip invocations   | Cargo `[profile.release]` `strip = true` (already set in workspace) | Already configured in root `Cargo.toml` profile     |

---

## Runtime State Inventory

This phase does not rename or refactor existing runtime state. No Runtime State Inventory needed.

Step 2.5: SKIPPED (not a rename/refactor/migration phase).

---

## Environment Availability Audit

| Dependency                   | Required By                                     | Available                                              | Version                       | Fallback                          |
| ---------------------------- | ----------------------------------------------- | ------------------------------------------------------ | ----------------------------- | --------------------------------- |
| Rust stable toolchain        | CLI binary build                                | ✓ (GH runner)                                          | dtolnay/rust-toolchain@stable | —                                 |
| `7z` / `7-Zip`               | Windows CLI archive                             | ✓ (pre-installed on windows-latest)                    | built-in                      | PowerShell Compress-Archive       |
| `tar`                        | macOS/Linux CLI archive                         | ✓ (all runners)                                        | system                        | —                                 |
| `wrangler` (npm)             | R2 upload                                       | installed via `npm install -g wrangler` in release.yml | via npm                       | —                                 |
| `cargo update`               | Cargo.lock refresh after workspace version bump | ✓ (part of Rust toolchain)                             | bundled                       | manual Cargo.lock patch (fragile) |
| `libwebkit2gtk-4.1-dev` etc. | clipboard-rs on Ubuntu                          | ✓ (via apt)                                            | ubuntu-22.04                  | —                                 |

**Missing dependencies with no fallback:** None.

---

## Common Pitfalls

### Pitfall 1: CLI Version Drift

**What goes wrong:** `uc-cli/Cargo.toml` has hardcoded `version = "0.1.0"`. After a workspace version bump to `0.4.0-alpha.2`, the CLI binary still reports `0.1.0` via `--version` and the archive filename uses the wrong version.

**Why it happens:** The current `scripts/bump-version.js` only patches `src-tauri/Cargo.toml` `[package].version` and `[workspace.package].version`. Crates that use `version.workspace = true` inherit automatically; crates with hardcoded versions do not.

**How to avoid:** Change `uc-cli/Cargo.toml` to `version.workspace = true`. Then `cargo update --workspace` after the bump script ensures Cargo.lock reflects the correct version.

**Warning signs:** `uniclipboard-cli --version` outputs `0.1.0` when workspace is at a different version.

### Pitfall 2: CLI Build Requires libxcb on Linux

**What goes wrong:** `cargo build -p uc-cli` on Ubuntu fails with linker errors about missing `libxcb`, `libx11`, etc.

**Why it happens:** `uc-cli` depends on `uc-daemon` which depends on `uc-platform` which depends on `clipboard-rs`. Even though the CLI only calls daemon functionality at runtime (spawning a subprocess), the `uc-daemon` crate is a compile-time dependency and pulls in clipboard native libs.

**How to avoid:** The CLI build job must install the same system dependencies as the Tauri app build: `libwebkit2gtk-4.1-dev libappindicator3-dev librsvg2-dev patchelf`. Additionally, `libxcb-*` packages may be needed. Test on Ubuntu before shipping.

**Warning signs:** Build job fails with `error: could not find native library` or linker errors involving X11/clipboard libraries.

### Pitfall 3: Artifact Name Collision in release-assets/

**What goes wrong:** The `prepare-release-assets` step in `release.yml` flattens all artifacts into `release-assets/`. If a CLI file has the same name as an App file, one overwrites the other silently.

**Why it happens:** The flattening logic uses `basename` and only special-cases the macOS `.app.tar.gz` collision. CLI binaries with generic names like `uniclipboard-cli.tar.gz` would collide across platforms.

**How to avoid:** Include the target triple in the CLI archive filename at packaging time: `uniclipboard-cli-{VERSION}-{target}.tar.gz`. This guarantees uniqueness across platforms. The flattening script does not need modification since filenames are already unique.

**Warning signs:** `ls release-assets/` shows fewer CLI files than expected.

### Pitfall 4: assemble-update-manifest.js Rejects CLI .sig-less Archives

**What goes wrong:** The manifest assembly script scans for `.sig` files. CLI tarballs have no `.sig` files (they are not Tauri-signed updater artifacts). The script emits a warning but does not fail — this is acceptable behavior. However, if someone accidentally generates a `.sig` file for a CLI archive, the script may try to parse it as a platform artifact.

**Why it happens:** The script uses filename patterns (`.app.tar.gz.sig`, `.AppImage.sig`, etc.) to detect platforms. A CLI `.tar.gz.sig` would not match any known pattern and would be skipped with a "Skipping unrecognized .sig file" warning.

**How to avoid:** Do not sign CLI archives with the Tauri signing key. CLI binaries can optionally be checksummed (`sha256sum`) but that is separate from Tauri's minisign-based updater signature.

**Warning signs:** `assemble-update-manifest.js` emits unexpected "Skipping unrecognized .sig file" warnings for CLI artifacts.

### Pitfall 5: Cargo.lock Not Updated in Version Bump Commit

**What goes wrong:** `scripts/bump-version.js` patches the Cargo.lock entry for the `uniclipboard` workspace root package only. After `uc-cli` switches to `version.workspace = true`, the `uc-cli` entry in Cargo.lock still shows the old version. This causes `cargo check` to fail or produce an inconsistent state when the release workflow builds.

**Why it happens:** `updateCargoLock()` in `bump-version.js` uses a regex to find and replace only the root package entry. Workspace members are not touched.

**How to avoid:** After `bump-version.js` runs in CI, run `cargo update --workspace -p uc-cli --manifest-path src-tauri/Cargo.toml` to regenerate the uc-cli lock entry. This single command is idempotent and fast (only touches one package). Add it to the `prepare-release.yml` commit step and to `release.yml`'s validate job.

**Warning signs:** CI build fails with Cargo.lock mismatch error after version bump.

---

## Code Examples

### CLI Binary Build Step (GitHub Actions)

```yaml
# Source: mirrors existing build.yml pattern
- name: build CLI binary
  shell: bash
  working-directory: src-tauri
  run: cargo build --release -p uc-cli ${{ matrix.args }}
```

### CLI Archive Packaging (GitHub Actions)

```bash
# Source: standard Unix/Windows archive conventions
VERSION=$(node -p "require('./package.json').version")
TARGET="${{ matrix.target }}"
BIN_SRC="src-tauri/target/${TARGET}/release/uniclipboard-cli"

if [ "${{ matrix.platform }}" = "windows-latest" ]; then
  7z a "uniclipboard-cli-${VERSION}-${TARGET}.zip" "${BIN_SRC}.exe"
else
  tar -czf "uniclipboard-cli-${VERSION}-${TARGET}.tar.gz" \
    -C "src-tauri/target/${TARGET}/release" uniclipboard-cli
fi
```

### uc-cli Cargo.toml Version Fix

```toml
[package]
name = "uc-cli"
version.workspace = true   # Fix: was version = "0.1.0"
edition = "2021"
description = "Command-line interface for UniClipboard"
```

### Cargo.lock Refresh After Version Bump

```bash
# In prepare-release.yml and release.yml after bump-version.js
cd src-tauri && cargo update -p uc-cli
```

### Release Notes CLI Section Addition to Template

```markdown
# .github/release-notes/release.md.tmpl additions

## CLI Downloads

### macOS

{{CLI_MACOS_INSTALLERS}}

### Linux

{{CLI_LINUX_INSTALLERS}}

### Windows

{{CLI_WINDOWS_INSTALLERS}}
```

### Collect CLI Artifacts in create-release Job

```bash
# In release.yml create-release job, after existing artifact collection
# CLI archives are already uniquely named — no collision handling needed
while IFS= read -r src; do
  filename="$(basename "$src")"
  cp "$src" "release-assets/$filename"
done < <(find artifacts -type f \( \
  -name "uniclipboard-cli-*.tar.gz" -o \
  -name "uniclipboard-cli-*.zip" \
\) | sort)
```

---

## State of the Art

| Old Approach                      | Current Approach           | When Changed | Impact                                                         |
| --------------------------------- | -------------------------- | ------------ | -------------------------------------------------------------- |
| Single product release            | Dual product (App + CLI)   | Phase 71     | Build matrix adds CLI job; release assets include raw binaries |
| Version managed in 3 files        | Version in 4+ files        | Phase 71     | `uc-cli/Cargo.toml` must join workspace versioning             |
| Only Tauri bundles uploaded to R2 | App bundles + CLI archives | Phase 71     | R2 `artifacts/v{VERSION}/` will contain both sets              |

**Deprecated/outdated:**

- `uc-cli` hardcoded `version = "0.1.0"`: replaced by `version.workspace = true` in this phase.

---

## Open Questions

1. **Should CLI artifacts be code-signed?**
   - What we know: App artifacts are signed (Apple codesign + Tauri minisign). CLI archives are not currently signed.
   - What's unclear: Whether macOS Gatekeeper will block unsigned CLI binaries downloaded by users.
   - Recommendation: For v0.4.0-alpha, skip code signing for CLI. Add a note in release notes that users may need `xattr -d com.apple.quarantine ./uniclipboard-cli` on macOS. Plan code signing for stable release.

2. **Should the CLI have its own update mechanism?**
   - What we know: Tauri's updater is App-only. The CLI would need a separate mechanism (e.g., `cargo-binstall`, `gh release download`, or a custom check).
   - What's unclear: User preference for CLI update flow.
   - Recommendation: Out of scope for Phase 71. CLI users check GitHub Releases manually or use `uniclipboard-cli update` (future CLI command).

3. **Does `uc-cli` compilation require all Linux GUI libs?**
   - What we know: `uc-cli` → `uc-daemon` → `uc-platform` → `clipboard-rs`. The clipboard-rs crate may require X11/xcb dev headers even when no GUI is started.
   - What's unclear: Exact set of Linux packages needed for a pure CLI build.
   - Recommendation: Start with the same `apt-get install` as the Tauri build. Remove packages iteratively if compilation succeeds without them. This is low-risk since the packages are already specified.

---

## Validation Architecture

### Test Framework

| Property           | Value                                              |
| ------------------ | -------------------------------------------------- |
| Framework          | Vitest (frontend scripts), `cargo test` (Rust)     |
| Config file        | `vitest.config.ts` (frontend), `src-tauri/` (Rust) |
| Quick run command  | `bun test -- scripts/__tests__/`                   |
| Full suite command | `bun test && cd src-tauri && cargo test`           |

### Phase Requirements → Test Map

| Behavior                                      | Test Type | Automated Command                                              | Notes                        |
| --------------------------------------------- | --------- | -------------------------------------------------------------- | ---------------------------- |
| `bump-version.js` updates uc-cli Cargo.toml   | unit      | `bun test -- scripts/__tests__/bump-version.test.ts`           | Extend existing test         |
| CLI archive created with correct filename     | smoke     | Manual inspection in CI artifact                               | Script-level test not needed |
| `generate-release-notes.js` emits CLI section | unit      | `bun test -- scripts/__tests__/generate-release-notes.test.ts` | Extend existing test         |
| CLI binary reports correct version            | smoke     | `./uniclipboard-cli --version` in CI                           | Manual or CI output check    |

### Wave 0 Gaps

- `scripts/__tests__/bump-version.test.ts` — extend to cover uc-cli Cargo.toml update
- `scripts/__tests__/generate-release-notes.test.ts` — extend to cover CLI section rendering

_(Existing test infrastructure covers most; only extension needed, not new test files)_

---

## Sources

### Primary (HIGH confidence)

- Direct file inspection of `.github/workflows/*.yml` — all workflow behavior documented above
- Direct file inspection of `scripts/bump-version.js`, `scripts/assemble-update-manifest.js`, `scripts/generate-release-notes.js`
- Direct file inspection of `src-tauri/crates/uc-cli/Cargo.toml` and `src-tauri/Cargo.toml`
- Direct file inspection of `src-tauri/tauri.conf.json`, `src-tauri/build.rs`
- Direct file inspection of `workers/update-server/src/index.ts`

### Secondary (MEDIUM confidence)

- GitHub Actions documentation on `workflow_call` and matrix strategies — verified by existing workflows
- Cargo workspace inheritance via `version.workspace = true` — standard Cargo behavior, verified by examining uc-daemon/Cargo.toml which already uses this pattern

### Tertiary (LOW confidence)

- macOS Gatekeeper behavior on unsigned CLI binaries — assumption based on common macOS security model, not verified against current Apple docs
- Exact Linux packages required for clipboard-rs in CLI build — inferred from uc-platform dependencies, not tested

---

## Metadata

**Confidence breakdown:**

- Standard stack: HIGH — all tooling already in use in the project
- Architecture: HIGH — based on direct inspection of existing workflows
- Pitfalls: HIGH (version drift, archive naming) / MEDIUM (Linux deps, Gatekeeper)

**Research date:** 2026-03-28
**Valid until:** 2026-04-28 (stable — GitHub Actions APIs and Cargo workspace behavior are stable)

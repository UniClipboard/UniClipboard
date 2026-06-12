# Release & Versioning Guide

This document describes the versioning policy, upstream-sync workflow, and
release process for UniClip.

## Versioning Policy

UniClip follows [Semantic Versioning](https://semver.org/) (`MAJOR.MINOR.PATCH`)
**independently** from the upstream `Jeric-X/syncclipboard-mobile` project.

- **Snapshot point:** version `1.0.11` is the alignment snapshot where UniClip
  last shared a version number with upstream. From `1.0.12` onward, UniClip's
  version numbers evolve on their own schedule and have no relation to upstream
  release numbers.
- **PATCH (`1.0.X`)** — bug fixes only, no API or user-visible behavior change.
- **MINOR (`1.X.0`)** — new features, backward compatible.
- **MAJOR (`X.0.0`)** — breaking changes (protocol break, removed feature,
  data-format migration).

### versionCode Rule

`android.versionCode` is an integer that **must increase monotonically** across
every published artifact, including beta builds. The rule is simple:

> Every published tag bumps `versionCode` by 1.

| Release       | version | versionCode |
| ------------- | ------- | ----------- |
| v1.0.11-beta1 | 1.0.11  | 151         |
| v1.0.11       | 1.0.11  | 152         |
| v1.0.12-beta1 | 1.0.12  | 153         |
| v1.0.12       | 1.0.12  | 154         |

This guarantees that any user who installed an earlier build (stable or beta)
can upgrade to a newer one.

### Tag Naming

- **Stable:** `vX.Y.Z` (e.g. `v1.0.12`)
- **Pre-release:** `vX.Y.Z-betaN` (e.g. `v1.0.12-beta1`)

The CI workflow detects the channel via `contains(github.ref_name, 'beta')` and
marks the GitHub Release accordingly.

## CHANGES.md Format

The first line of each version section is the bare tag (`vX.Y.Z`); the CI
extracts release notes with:

```sh
awk 'NR==1{next} /^$/{exit} {print}' CHANGES.md > feature.txt
```

So **keep the first-line tag format unchanged**. Every bullet must be prefixed
with a provenance tag:

- `[upstream]` — change ported from `Jeric-X/syncclipboard-mobile` or
  `Jeric-X/SyncClipboard`. Include a ref to the upstream commit/PR when
  possible.
- `[uc]` — UniClip-specific change (feature, refactor, branding, UI, etc.).

### Example

```
v1.0.12
- [upstream] Fix: clipboard sync stops after device sleep (ref: Jeric-X/syncclipboard-mobile@abc1234)
- [uc] Feature: add in-app APK update notification
- [uc] Refactor: extract sync coordinator from HomeScreen

v1.0.11
- [upstream] Fix: auto-upload SMS verification code stops working after 6h background runtime on Android 14+
- [uc] Rebrand to UniClip, migrate to UniClipboard/uc-android
- [uc] Built-in APK update download
- [uc] New app icon
```

## Upstream Sync Workflow

UniClip is a **clean-room re-implementation** of the SyncClipboard protocol
(TypeScript / Expo), not a git-level fork of the upstream Android app. Merging
is therefore manual.

Routine:

1. **Watch** `Jeric-X/syncclipboard-mobile` and `Jeric-X/SyncClipboard` on
   GitHub for new releases.
2. **Triage** new commits since the last sync. Focus on:
   - **Protocol-layer** changes (API fields, signatures, file chunking,
     SignalR contracts) — must port; servers are shared.
   - **Cross-platform logic** bugs (sync race conditions, retry logic) —
     usually applicable.
   - Android-platform fixes (background services, permissions) — port only if
     UniClip exhibits the same issue.
   - **Skip** anything specific to upstream's stack (MAUI, .NET, their UI).
3. **Port manually** into UniClip's corresponding module
   (`modules/*`, `src/services/*`, plugins, etc.). Do not `cherry-pick` —
   technical stacks differ.
4. **Commit** with an upstream reference, e.g.:

   ```
   fix: handle sync retry after token refresh

   Ref: Jeric-X/syncclipboard-mobile@abc1234
   ```

5. **Record** the change in `CHANGES.md` under the current development
   version, prefixed with `[upstream]`.

### Tracking the Sync Point

When a UniClip release ships, optionally record the upstream version it has
been synced to in the release notes or the in-app About page, e.g.
"Tracked upstream: v1.0.13 (2026-06-XX)". UniClip's own version stays
independent.

## Release Workflow

### Pre-release Checklist

- [ ] All upstream fixes intended for this release have been ported and
      recorded in `CHANGES.md`.
- [ ] Beta build (if one was published) has been verified on a physical
      device, including any long-running scenarios mentioned in the
      changelog.
- [ ] `CHANGES.md` top section reflects the final release notes (this becomes
      the GitHub/Gitee release body).
- [ ] `app.json`:
  - `expo.version` matches the tag without the `v` prefix.
  - `expo.android.versionCode` is bumped to `previous + 1`.
- [ ] Working tree is clean.
- [ ] `main` is up to date with the latest CI green.

### Steps

```sh
# 1. Bump version metadata on a clean main
#    edit app.json: version, versionCode
#    edit CHANGES.md: new vX.Y.Z section at top

# 2. Commit
git add app.json CHANGES.md
git commit -m "chore: release vX.Y.Z"

# 3. Tag and push
git push origin main
git tag vX.Y.Z
git push origin vX.Y.Z
```

The tag push triggers the `build.yml` workflow on GitHub Actions, which in
order:

1. Runs `code-style` + `unit-tests` + `android-build` (4-ABI matrix).
2. Publishes a GitHub Release with all 4 APKs (`arm64-v8a`, `armeabi-v7a`,
   `x86_64`, `universal`). This job does **not** wait for the Gitee jobs.
3. Mirrors the repository to Gitee.
4. Creates a matching Gitee Release.
5. Uploads 3 ABI APKs to Gitee (`universal` is skipped due to Gitee's
   upload size limits).

### Beta Release

Same workflow, but with `vX.Y.Z-betaN` tags. The CI auto-marks the release as
pre-release.

## Identifier Reference

| Field            | Value                      |
| ---------------- | -------------------------- |
| Android package  | `app.uniclipboard.android` |
| iOS bundle       | `app.uniclipboard.ios`     |
| App display name | `UniClip`                  |
| Expo slug        | `uniclip`                  |

These are independent namespaces from upstream. UniClip can be installed
alongside any other SyncClipboard-protocol client on the same device.

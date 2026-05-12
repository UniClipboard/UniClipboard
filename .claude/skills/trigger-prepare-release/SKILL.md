---
name: trigger-prepare-release
description: Trigger the prepare-release workflow on GitHub Actions. Supports optional arguments for version, bump type, and channel. Defaults to bump=patch, channel=stable.
metadata:
  author: mark
  version: "1.2.0"
allowed-tools: Bash(gh:*), Bash(git:*)
---

# Trigger Prepare Release

Trigger the `prepare-release.yml` workflow on GitHub Actions.

The workflow always runs against `main` (hardcoded inside the workflow) and produces a `release/v<version>` branch + PR.

## Arguments

All arguments are optional.

| Parameter | Default   | Description                                                                                                          |
| --------- | --------- | -------------------------------------------------------------------------------------------------------------------- |
| `version` | *(empty)* | Exact semver, no `v` prefix. **Include the channel suffix if you want one** (e.g. `0.9.0-alpha.1`). Overrides `bump` AND `channel`. |
| `bump`    | `patch`   | Version bump type: `patch`, `minor`, `major`. Only used when `version` is empty.                                     |
| `channel` | `stable`  | Release channel: `stable`, `alpha`, `beta`, `rc`. Only used when `version` is empty.                                 |

### Critical: how `version` interacts with `channel`

The workflow's `Determine version` step is:

```sh
if [ -n "$version" ]; then
  VERSION="$version"        # used verbatim, channel ignored
else
  VERSION=$(bump-version.js --type "$bump" --channel "$channel" --dry-run ...)
fi
```

So:

- ✅ `version=0.9.0-alpha.1` → release `v0.9.0-alpha.1` (correct way to ship an alpha at an exact version)
- ❌ `version=0.9.0` + `channel=alpha` → release `v0.9.0` (channel **silently ignored**, this is the trap)
- ✅ `bump=minor` + `channel=alpha` → script computes next alpha (e.g. `v0.9.0-alpha.1`)

## Workflow

### 1. Parse arguments

Accept either `key=value` form (`channel=beta bump=minor`) or positional tokens (`/trigger-prepare-release 0.9.0-alpha.1`). For each positional token, classify by shape:

| Token shape                                              | Classified as | Examples                              |
| -------------------------------------------------------- | ------------- | ------------------------------------- |
| Looks like semver (digit-dot-digit, optional `-suffix.N`) | `version`     | `0.9.0`, `0.9.0-alpha.1`, `1.2.3-rc.2` |
| One of `stable` / `alpha` / `beta` / `rc` (bare word)    | `channel`     | `alpha`, `rc`                         |
| One of `patch` / `minor` / `major`                       | `bump`        | `minor`                               |

**Ambiguity rule:** if the user supplies a semver-shaped token AND a bare channel word (e.g. `0.9.0 alpha.1` or `0.9.0 alpha`), do NOT split them across `version`/`channel` — that triggers the trap above. Instead, ask the user whether they meant:

- `version=0.9.0-alpha.1` (exact version with suffix), or
- `bump=minor channel=alpha` (let the script compute the next alpha)

### 2. Confirm with user

Display the resolved parameters and ask for confirmation before triggering:

```
Will trigger prepare-release with:
  version: 0.9.0-alpha.1
  bump:    (ignored — version is set)
  channel: (ignored — version is set)
Proceed? (y/n)
```

If `version` is empty, show resolved `bump` and `channel` instead.

### 3. Trigger the workflow

```bash
gh workflow run prepare-release.yml \
  --repo UniClipboard/UniClipboard \
  --ref main \
  -f version=<version> \
  -f bump=<bump> \
  -f channel=<channel>
```

Omit `-f version=` when `version` is empty. `--ref` is always `main` because the workflow checks out `main` internally regardless.

### 4. Confirm dispatch

After triggering, wait a few seconds and show the latest run:

```bash
gh run list --repo UniClipboard/UniClipboard --workflow=prepare-release.yml --limit 1
```

Report the run URL so the user can monitor progress. The run's name (`prepare-release-v<resolved-version>`) is the easiest way to verify the version came out as expected — check it before walking away.

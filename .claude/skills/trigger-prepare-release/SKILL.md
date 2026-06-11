---
name: trigger-prepare-release
description: Trigger the prepare-release workflow on GitHub Actions. Supports optional arguments for version, bump type, channel, and base branch. Defaults to bump=patch, channel=stable, base_branch=main.
metadata:
  author: mark
  version: "1.3.0"
allowed-tools: Bash(gh:*), Bash(git:*)
---

# Trigger Prepare Release

Trigger the `prepare-release.yml` workflow on GitHub Actions.

The workflow produces a `release/v<version>` branch + PR. By default it bases the release off `main`, but the `base_branch` input lets you base it off (and target the PR at) any branch — see [Choosing `base_branch`](#choosing-base_branch).

## Arguments

All arguments are optional.

| Parameter     | Default   | Description                                                                                                          |
| ------------- | --------- | -------------------------------------------------------------------------------------------------------------------- |
| `version`     | *(empty)* | Exact semver, no `v` prefix. **Include the channel suffix if you want one** (e.g. `0.9.0-alpha.1`). Overrides `bump` AND `channel`. |
| `bump`        | `patch`   | Version bump type: `patch`, `minor`, `major`. Only used when `version` is empty.                                     |
| `channel`     | `stable`  | Release channel: `stable`, `alpha`, `beta`, `rc`. Only used when `version` is empty.                                 |
| `base_branch` | `main`    | Branch the release is checked out from **and** the branch the PR targets. Use `release/vX.Y.Z` when iterating prereleases on a release branch whose commits are not yet merged back to `main`. |

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

### Choosing `base_branch`

`base_branch` controls **two** things at once: the branch the release is checked out from, and the branch the resulting PR targets (`--base`).

- **`main` (default)** — the normal case: cut a release off the mainline.
- **`release/vX.Y.Z`** — iterating prereleases on a release branch whose commits are NOT yet merged back to `main`. The workflow checks out that branch (so the release includes its un-merged commits) and opens the PR back against it. Example: you committed fixes onto `release/v0.14.1` and want the next `alpha` to include them without first merging to `main`.

Two things to verify before triggering with a non-default `base_branch`:

- **The branch must be pushed.** The workflow checks out the **remote** `origin/<base_branch>`, so anything still local-only won't be in the release. Confirm `git status -sb` shows the branch in sync (not `ahead`) before triggering.
- **Version is computed from that branch.** The `Determine version` dry-run runs against `base_branch`'s `package.json`, so the resolved version reflects that branch, not `main`. The created branch is always named `release/v<resolved-version>` regardless of `base_branch`.

## Workflow

### 1. Parse arguments

Accept either `key=value` form (`channel=beta bump=minor`) or positional tokens (`/trigger-prepare-release 0.9.0-alpha.1`). For each positional token, classify by shape:

| Token shape                                              | Classified as | Examples                              |
| -------------------------------------------------------- | ------------- | ------------------------------------- |
| Looks like semver (digit-dot-digit, optional `-suffix.N`) | `version`     | `0.9.0`, `0.9.0-alpha.1`, `1.2.3-rc.2` |
| One of `stable` / `alpha` / `beta` / `rc` (bare word)    | `channel`     | `alpha`, `rc`                         |
| One of `patch` / `minor` / `major`                       | `bump`        | `minor`                               |

`base_branch` is **never** inferred from a positional token (a branch name like `release/v0.14.1` is too free-form to classify safely) — accept it only via the explicit `base_branch=<branch>` form.

**Ambiguity rule:** if the user supplies a semver-shaped token AND a bare channel word (e.g. `0.9.0 alpha.1` or `0.9.0 alpha`), do NOT split them across `version`/`channel` — that triggers the trap above. Instead, ask the user whether they meant:

- `version=0.9.0-alpha.1` (exact version with suffix), or
- `bump=minor channel=alpha` (let the script compute the next alpha)

### 2. Confirm with user

Display the resolved parameters and ask for confirmation before triggering:

```
Will trigger prepare-release with:
  version:     0.9.0-alpha.1
  bump:        (ignored — version is set)
  channel:     (ignored — version is set)
  base_branch: main
Proceed? (y/n)
```

If `version` is empty, show resolved `bump` and `channel` instead. Always show `base_branch` — and when it is not `main`, restate the resolved version so the user can confirm it was computed from the intended branch.

### 3. Trigger the workflow

```bash
gh workflow run prepare-release.yml \
  --repo UniClipboard/UniClipboard \
  --ref main \
  -f version=<version> \
  -f bump=<bump> \
  -f channel=<channel> \
  -f base_branch=<base_branch>
```

Omit `-f version=` when `version` is empty. Omit `-f base_branch=` to default to `main`.

**`--ref` vs `base_branch` — don't confuse them:**

- `--ref main` selects which copy of the workflow *file* to run. Keep it `main`: it's the default branch, so GitHub validates the dispatch inputs against `main`'s workflow definition. (Both `main` and the release branches carry the `base_branch` input, so this is safe.)
- `base_branch` is what actually controls which branch the release is based on and where the PR is targeted. `--ref` does **not** affect that.

(Older versions of this skill claimed the workflow "always checks out `main` regardless" — that is no longer true; `base_branch` is honored.)

### 4. Confirm dispatch

After triggering, wait a few seconds and show the latest run:

```bash
gh run list --repo UniClipboard/UniClipboard --workflow=prepare-release.yml --limit 1
```

Report the run URL so the user can monitor progress.

To verify the version came out as intended, do **not** rely on the run name: it is `prepare-release-v${version || bump}`, so when `version` is empty it shows the **bump type** (e.g. `prepare-release-vpatch`), not the resolved version. Instead check the `Determine version` step's log, or the PR it opens — the PR title is `release: v<resolved-version>` and its body states the base branch.

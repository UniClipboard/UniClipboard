---
name: dual-side-sync
description: Push the macOS working-tree changes to the Windows peer's repo via rsync over SSH. Use when the user wants to "sync to win", "push my changes to windows", "mirror the working tree", or otherwise propagate uncommitted edits from this Mac to the paired Windows machine for cross-platform testing of uniclipboard. Replaces the older SMB-mount strategy.
---

# dual-side-sync

This project pairs a macOS host with a Windows peer to test uniclipboard's cross-platform sync. During active development the same uncommitted change often needs to run on both sides at once. This skill mirrors the **mac working tree** onto a **remote Windows checkout** of the same repo, using `rsync` over `ssh`.

The helper script lives at `.agents/skills/dual-side-sync/sync-to-win.sh`. It is the **only** thing you should invoke for this task — do not hand-roll `scp`, `git apply`, or SMB copy commands.

## What the script does

1. Confirms both repos are at the **same git HEAD** (refuses if not — otherwise we'd silently overwrite a different commit).
2. Confirms the **win working tree is clean** (or pass `--force` to reset it first).
3. rsyncs the set of files that mac considers `tracked + untracked-not-ignored` to the win repo.
4. Removes (via `ssh` + `rm`) any files that mac has deleted from its working tree.

`.git/`, `target/`, `node_modules/`, and `dist/` are always excluded. The win-side `.git` is never touched, no commits are pushed, and win HEAD never moves. The win repo stays "owned" by git on the win side; we only mirror the working tree.

## Configuration (one-time)

The script needs the developer to provide:

| Field | Required | Notes |
|---|---|---|
| `WIN_HOST` | yes | IPv4/IPv6 or hostname of the Windows machine. |
| `WIN_PORT` | no  | SSH port. Defaults to `22`. |
| `WIN_USER` | yes | Windows account (the SSH login). |
| `WIN_REPO` | yes | Absolute path of the repo on win, in a form the remote shell understands (e.g. `/c/Users/mark/projects/UniClipboard`). |
| `WIN_PASS` | one of | Password auth. Requires `sshpass` on the Mac. |
| `WIN_KEY`  | one of | Private-key auth. Or leave both empty and use `ssh-agent` / `~/.ssh/config`. |

Initial setup (run from project root):

```bash
cp .agents/skills/dual-side-sync/config.example.sh \
   .agents/skills/dual-side-sync/config.local.sh
$EDITOR .agents/skills/dual-side-sync/config.local.sh
```

`config.local.sh` is gitignored. The script auto-sources it on every invocation.

Verify:

```bash
.agents/skills/dual-side-sync/sync-to-win.sh config   # echo back what it loaded
.agents/skills/dual-side-sync/sync-to-win.sh check    # ssh login + remote rsync + remote git
```

## Windows-side prerequisites

* OpenSSH server enabled (Windows ships it; run `Get-Service sshd` in PowerShell to confirm).
* `rsync` available on the user's `PATH` for the SSH login. Easiest path: install **Git for Windows** and ensure its `usr\bin\` directory (which contains `rsync.exe`) is on the SSH user's `PATH`. MSYS2, Cygwin, or WSL all work too.
* The win repo must be a working git checkout at the same HEAD as mac.

If `sync-to-win.sh check` reports "rsync not found on the win side", fix the PATH on the win user before continuing.

## Commands

Always invoke via the script. From the project root:

```bash
.agents/skills/dual-side-sync/sync-to-win.sh <command> [options]
```

| Command | When to use |
| --- | --- |
| `config` | First call after editing `config.local.sh`. Confirms the values were picked up. |
| `check`  | Verify ssh login, remote `rsync`, and remote git repo are all reachable. Run this whenever something fails mysteriously. |
| `status` | Show both sides: branch, HEAD, dirty files. Always run before `push` if you're unsure. |
| `diff`   | Show the diffstat of what `push` would send (tracked changes + untracked files + deletions). |
| `push`   | Mirror the mac working tree to win. Refuses if HEADs differ or win is dirty. |
| `push --force` | Reset win first if it's dirty (drops any local edits on win). |
| `push --dry-run` | Show rsync's plan without modifying win. Best last sanity check. |
| `reset`  | `git reset --hard` + `git clean -fd` on win, preserving `target/` and `node_modules/`. |
| `ssh`    | Open an interactive ssh shell on win, or run a one-off remote command (`sync-to-win.sh ssh "git -C /c/.../repo log -1"`). |
| `paths`  | Print resolved `MAC_REPO`, `WIN_REPO`, and the remote endpoint. |

## Recommended workflow

1. **Confirm config once.** `sync-to-win.sh config && sync-to-win.sh check`.
2. **Ground yourself.** `sync-to-win.sh status` — make sure both sides are at the same HEAD and you understand what's dirty on each.
3. **Preview.** `sync-to-win.sh diff` (logical view) and/or `sync-to-win.sh push --dry-run` (rsync's actual plan) before any destructive run.
4. **Push.** `sync-to-win.sh push`. If win is dirty by design (build artifacts only), use `--force`. If win has real local edits you don't want to lose, save them first.
5. **Verify on win.** `sync-to-win.sh ssh "git -C \"$WIN_REPO\" status --short"` or open an interactive shell.

## When to refuse / pause

* **HEAD mismatch** — never sync. Tell the user to align both repos to the same commit (`git push` from one, `git pull` on the other) and re-run `status`.
* **`check` fails** — surface the exact failure (ssh, rsync, or repo) and fix that before any push. Do not retry `push` blindly.
* **Win has uncommitted edits the user might want** — confirm with the user before running `push --force` or `reset`.

## Things to avoid

* Don't `scp` or `cp` files manually — bypassing the script means deletions and excludes get out of sync.
* Don't run `push` without first confirming `status` is clean on win (or being explicit with `--force`).
* Don't put `WIN_PASS` anywhere except `config.local.sh`. Never paste it into a chat, commit, or terminal history.
* Don't add new always-excluded paths in `EXTRA_EXCLUDES` for one-shot use — pass them as `--exclude=...` into a fork of the rsync call instead, or add them deliberately to the config when they apply long-term.

## When this skill does *not* apply

* Pulling **from** win **to** mac — this script is one-way. Use git for that.
* Inspecting Windows logs — use the `dual-side-debug` skill.
* Pushing committed work — that's just `git push`/`git pull`, no skill required.
* Building/installing on win — out of scope; the script only mirrors source.

---
name: create-pr
description: Push the current branch and open a GitHub pull request against `main`. Use when the user says "create PR", "open PR", "make a PR", "提 PR", "开 PR", "打 PR", or otherwise asks to publish their committed work for review. Before pushing, the skill (1) verifies the current branch name actually reflects the change and proposes a rename if it doesn't, and (2) scans the PR's diff against `docs-site/` for docs that look stale and lists update candidates for the user to approve.
user-invocable: true
allowed-tools: Bash(git:*), Bash(gh:*), Bash(grep:*), Bash(rg:*), Read, Edit, Write, AskUserQuestion
---

# create-pr

## Purpose

Opening a PR on this repo is mechanically simple (`gh pr create`) but two things consistently go wrong if rushed:

1. **The branch name doesn't describe the change.** Codenames like `agate-surgeon`, throwaways like `wip`, or repurposed branches like `fix-bug` survive into the merged history and make `git log --graph` useless. The branch name is the only summary that shows up in *every* future `git log` line for these commits — get it right before publishing.
2. **`docs-site/` rots silently.** This repo ships its own user-facing docs (`docs-site/content/docs/{en,zh}/`). Code changes that touch user-visible behavior — CLI flags, settings, timeouts, file formats, error messages, supported environments — almost always have a paragraph somewhere in `docs-site/` that now lies. The PR author is the only person with the context to spot it; reviewers will miss it.

This skill enforces both checks *before* pushing, so the PR that lands on GitHub is already named correctly and either includes the docs update or has an explicit user decision to skip.

## When to trigger

Trigger on any of:

- "create PR", "open PR", "make a PR", "raise a PR"
- "提 PR", "开 PR", "打 PR", "建 PR"
- User pastes / mentions an issue URL and says something like "PR it" or "close that with a PR"

Do **not** trigger on "commit" / "push" alone — those are pre-PR steps that the user is doing manually.

## Refuse / pause conditions

| Condition | What to do |
| --- | --- |
| Current branch is `main` | Refuse. Ask the user to pick a branch name; offer to `git switch -c <name>` for them. Never `gh pr create` from `main`. |
| `git log main..HEAD` is empty | Refuse — there's nothing to PR. Ask whether they meant to commit first. |
| Working tree dirty | Pause. Show `git status` and ask: commit now, stash, or abort? Don't push half-finished work. |
| `gh` not authenticated (`gh auth status` fails) | Surface the exact error and ask the user to run `gh auth login` themselves — auth is interactive. |
| `--no-verify`, `--force` to a shared branch, or amending an already-pushed commit | Don't do these without an explicit user request that names the destructive flag. |

## Workflow

### Step 1 — Take stock (run in parallel)

```bash
git status                                       # working tree state (never -uall)
git rev-parse --abbrev-ref HEAD                  # current branch
git log main..HEAD --format="%h %s"              # commits this PR will include
git diff main...HEAD --stat                      # files touched
git rev-parse --abbrev-ref --symbolic-full-name @{u} 2>&1 || echo NO_UPSTREAM
gh auth status 2>&1 | head -5                    # auth sanity
```

Also scan the recent conversation for:

- An issue number or URL the user wants this PR to close (e.g. `#652`, `https://github.com/.../issues/652`). If present, fetch it once so the Summary can reference it accurately:

  ```bash
  gh issue view <N> --repo UniClipboard/UniClipboard --json number,title,body,state
  ```

- Any user statement that this PR is partial / draft / WIP — that changes the PR body wording and may want `gh pr create --draft`.

Apply the refuse-conditions in the table above before going further.

### Step 2 — Branch-name review

Evaluate the current branch name against the commits in `git log main..HEAD`. A good name:

- Is **kebab-case** (matches the existing style in `git branch -a`).
- Reflects the **scope of the change**, not its codename or a ticket id alone. Read the commit subjects and ask: would a teammate skimming `git log --oneline main..` understand what this branch was for from the name?
- Is short (≤ 40 chars is a soft target).

If the name passes, say so in one line and move on. If it doesn't:

1. Propose **one** new name derived from the commit subjects. Don't dump three options — pick the best one and let the user counter.
2. Show: `current name` → `proposed name`, with a one-sentence justification ("the commits are all about X; current name is a codename that doesn't say that").
3. Ask the user: accept, counter-propose, or skip.
4. On accept:

   ```bash
   git branch -m <new>                            # rename locally
   # If the old name was already pushed:
   git push origin -u <new>                       # push under the new name
   git push origin --delete <old>                 # remove the old remote branch
   ```

   If the old branch already has an open PR pointing at it, **stop and tell the user** — renaming will orphan that PR. Don't try to rewire it silently.

### Step 3 — `docs-site` impact review (the part that's easy to skip)

The repo's user docs live in `docs-site/content/docs/{en,zh}/`. Skim the PR diff and identify any change whose **user-visible contract** may now disagree with the docs. The categories below are the high-yield ones — search the diff for these before declaring no impact.

| Diff category | What to look for | Where docs likely live |
| --- | --- | --- |
| New / renamed / removed CLI subcommand or flag | `clap::Subcommand`, `#[arg(long = ...)]`, `--help` strings | `docs-site/content/docs/{en,zh}/reference/cli.mdx`, related guides |
| Changed default value, timeout, TTL, port, retry count | numeric literal changes, `Duration::from_*`, `const ... = ...` | `troubleshooting.mdx`, `settings.mdx`, the relevant guide |
| New / renamed setting in the GUI settings panel | `setting`, `preference`, `config_key` symbols touched | `docs-site/content/docs/{en,zh}/guides/settings.mdx` |
| New / changed user-visible error message or exit code | string literals in error variants, `exit_codes.rs` | `troubleshooting.mdx`, `reference/cli.mdx` |
| Behavior change visible to users (filter rule, capability gate, network path) | comments saying "this is the source of truth for X", new modules with policy logic | `pairing.mdx`, `sync.mdx`, `quick-panel.mdx`, etc. |
| New hidden / dev command worth documenting as "exists but hidden" | files under `commands/dev*`, comments saying "hidden from --help" | `cli.mdx` "Hidden commands" section |
| Bumped supported version of a feature | `since = "X.Y.Z"`, version-gated branches | `<Feature since="...">` blocks across guides |

For each candidate hit, produce a one-line entry:

```
- <docs file>:<line> — current text says "<short quote>"; this PR changes <X> so it should say "<short suggested replacement>". (Reason: <which commit / which line in the code>.)
```

**Always show the candidate list to the user before editing anything.** Even if the list is empty, say so explicitly ("Scanned docs-site; found nothing that depends on this PR's surface area"). Then ask:

- Apply all? Apply some (which)? Apply none?
- If the answer is "apply none", record the explicit decision so the PR body can mention "docs left as-is — no user-facing surface changed" (rather than silently omitting it, which leaves reviewers guessing).

On approval, edit the listed files, then commit them as a **separate** commit:

```
docs(docs-site): <one-line summary that matches the code change>
```

Don't fold docs edits into a fix/feat commit by amending — keep them as their own commit in the PR. (Amending also breaks the rule about not modifying already-pushed commits.)

Both `en/` and `zh/` versions must be updated in lockstep when both exist — never ship a PR that updates only one language.

### Step 4 — Build and open the PR

Title:

- Max 70 chars.
- Imperative mood, follow the `<type>(<scope>): <subject>` style already used on `main` (`git log --oneline -10` if unsure).
- Reflects the *primary* change, not all of them — secondary work goes in the body.

Body, using a heredoc to preserve formatting:

```markdown
## Summary

<2–4 bullets. One per commit cluster. State the *why* and the user-visible effect; reference commit short SHAs in parentheses so reviewers can map bullet → diff.>

<If an issue is being closed:>

Closes #<N>.

## Test plan

- [x] <Things already verified — `cargo test -p ...`, manual repro, e2e run, etc.>
- [ ] <Things the reviewer or follow-up should verify; be specific enough that someone else can actually do them.>
```

Then push and create:

```bash
git push -u origin <branch>

gh pr create --title "<title>" --body "$(cat <<'EOF'
<body>
EOF
)"
```

Add `--draft` if the user signaled the PR is incomplete.

If `git push` reports the remote is ahead (someone pushed to your branch), **stop** — don't `--force` without confirmation. Ask the user what's on the remote and how they want to reconcile.

### Step 5 — Report back

Print the PR URL (`gh pr create` outputs it on success). Add a one-line recap:

- Title used.
- Whether docs were updated (and how many files), or the explicit "no docs changes" decision.
- Issue that will close on merge, if any.

Stop there. Don't auto-request reviewers, add labels, or run any other workflow unless the user asks.

## Anti-patterns

- **Skipping the docs scan because "it's just a refactor".** Refactors that rename a CLI flag, change an error message, or alter a default break docs the same way features do. Scan first; the diff tells you whether there's anything to update.
- **Listing every candidate doc edit as if it's mandatory.** The skill's output is a *menu*, not a checklist. Be honest about which entries are weak — if a candidate is "maybe relevant", say "low confidence" so the user can dismiss it fast.
- **Renaming the branch silently when an open PR exists for the old name.** That orphans the PR and confuses reviewers. Always check `gh pr list --head <old>` before renaming.
- **Folding the docs commit into a code commit via `--amend`.** Past-tense `--amend` on already-pushed commits forces `--force-push`. Keep docs as a separate commit; it makes review easier anyway.
- **Inventing facts about a referenced issue.** If the user says "closes #X", actually `gh issue view X` once — don't paraphrase the title from memory.
- **`gh pr create` without `--body` (interactive editor).** This skill runs non-interactively; always pass `--body` via heredoc.

## When this skill does *not* apply

- Pushing committed work that doesn't need a PR (a direct push to a personal branch you'll keep iterating on). Just `git push`; come back to this skill when you're ready to publish.
- Updating an existing PR (new commits on an already-open PR). Just `git push`; GitHub picks them up automatically. Use this skill only for the *initial* PR creation.
- Triggering release workflows — that's `trigger-prepare-release`.
- Reviewing someone else's PR — that's `/review` or `/ultrareview`.

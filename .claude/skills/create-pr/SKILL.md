---
name: create-pr
description: Push the current branch and open a GitHub pull request against `main`. Use when the user says "create PR", "open PR", "make a PR", "提 PR", "开 PR", "打 PR", or otherwise asks to publish their committed work for review. Before pushing it gates the diff on four things — branch name accuracy, stale `docs-site/` docs, missing PostHog telemetry, and tracing-spec conformance — and surfaces each as a candidate list for the user to approve.
user-invocable: true
allowed-tools: Bash(git:*), Bash(gh:*), Bash(grep:*), Bash(rg:*), Read, Edit, Write, AskUserQuestion
---

# create-pr

## Purpose

Opening a PR on this repo is mechanically simple (`gh pr create`) but four things consistently go wrong if rushed:

1. **The branch name doesn't describe the change.** Codenames like `agate-surgeon`, throwaways like `wip`, or repurposed branches like `fix-bug` survive into the merged history and make `git log --graph` useless. The branch name is the only summary that shows up in *every* future `git log` line for these commits — get it right before publishing.
2. **`docs-site/` rots silently.** This repo ships its own user-facing docs (`docs-site/content/docs/{en,zh}/`). Code changes that touch user-visible behavior — CLI flags, settings, timeouts, file formats, error messages, supported environments — almost always have a paragraph somewhere in `docs-site/` that now lies. The PR author is the only person with the context to spot it; reviewers will miss it.
3. **New user-facing flows ship without telemetry.** This repo has a structured PostHog event model (`crates/uc-observability/src/analytics/`, schema doc `docs/architecture/telemetry-events.md`). When a PR adds or changes an activation/reliability flow — pairing, sync, setup, update, mobile-sync — but doesn't `capture(Event::...)` the new milestone or failure path, the funnel silently goes blind. The author is the only one who knows the flow is new.
4. **Tracing drifts from the spec.** The repo has a strict layered tracing spec (`docs/guides/tracing.md`): only the use-case layer owns spans, domain stays span-free, fields must be low-cardinality and privacy-safe, and log levels have defined meanings. New code routinely forgets a use-case span, logs at the wrong level, or leaks high-cardinality / clipboard data into a field.

This skill enforces all four checks *before* pushing, so the PR that lands on GitHub is already named correctly, has its docs/telemetry/tracing gaps either addressed or explicitly deferred with the user's sign-off.

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

### Step 4 — PostHog telemetry gap review

**Gate first — skip cheaply.** Telemetry is only ever emitted from the use-case layer via `self.analytics.capture(Event::...)`; `uc-core` and infra never emit. So run this before reading anything:

```bash
git diff main...HEAD --name-only | grep -qE '^crates/uc-application/|mobile.sync' || echo SKIP_TELEMETRY
```

If it prints `SKIP_TELEMETRY`, state one line ("diff doesn't touch uc-application / mobile-sync — no telemetry surface") and go to Step 5. Do **not** read the spec docs for an unrelated diff.

If the gate passes, the source of truth (not your memory) is:

- **Event catalog** — `crates/uc-observability/src/analytics/events.rs` (the `Event` enum + its enumerable property types). The closed set of events that already exist.
- **Schema doc** — `docs/architecture/telemetry-events.md`: §7 v1 event list, §12 roadmap of *planned-but-not-yet-emitted* events, §6 privacy contract.

You don't need to read these end-to-end. Scan the diff for the **key positions** below; only when you're about to suggest a specific `Event::Variant` do you `rg '<Variant>' crates/uc-observability/src/analytics/events.rs` to confirm it exists (or check §12 if it's planned).

Scan the diff for **key positions** where a milestone or failure outcome now happens but no `capture(...)` accompanies it:

| Diff signal | Telemetry that's likely missing |
| --- | --- |
| New activation milestone (a setup step, pairing handshake completes, first sync/file succeeds) | An Activation event (`SetupStarted/Completed`, `Pairing*`, `FirstClipboardSync*`, …) |
| New or changed sync / transfer outcome branch (success, failure, deferral) | A Reliability event (`SyncAttempted/Succeeded/Failed/Deferred`) with the right `failure_reason` / `defer_reason` enum |
| New failure / error variant on a user-facing flow | The matching `*_failed` event + a new enum value in its reason type (reasons are closed enums in `events.rs`) |
| New feature line (update lifecycle, mobile-sync, a new capability) | Check §12 roadmap — the event may already be specified there and just needs wiring |
| A new use case that completes a user-visible action | Decide whether it's a funnel/reliability anchor worth an event at all (not everything is) |

For each candidate, emit a one-line entry:

```
- <code file>:<line> — <flow> reaches <milestone/outcome> but emits no event. Suggest capture(Event::<Variant>{...}) (or: already specced in telemetry-events.md §<n>). Confidence: <high/low>.
```

**Privacy & double-count red lines** (from schema doc §6 and the existing call sites) — call these out if the diff violates them, they are bugs not suggestions:

- Never put clipboard content, raw file names, full settings, or raw sizes into a property. Sizes go through `PayloadSizeBucket`; latency-sensitive durations (e.g. `sync_latency_ms`) report a precise `u32` while coarse durations use `LatencyBucket`; everything must stay low-cardinality.
- Inbound-sync paths that write the *local* clipboard (e.g. a `RemotePush` origin) must **not** emit capture/DAU events — that double-counts. See the red-line comment around `clipboard_capture/usecase.rs`.
- Event names and property values are **frozen once shipped** — never rename an existing variant; evolve with a `*_v2`.

**Always show the candidate list to the user before adding any instrumentation.** If the diff has no telemetry-worthy surface, say so explicitly ("Scanned the uc-application diff; no new activation/reliability milestone, nothing to instrument"). Then ask: add now, or defer (and note it in the PR body)? Telemetry is normal code — if added, it belongs in the relevant `feat`/`fix` commit, **not** the docs commit.

### Step 5 — Tracing / logging best-practice review

**Gate first.** This step only applies to Rust changes:

```bash
git diff main...HEAD --name-only | grep -qE '\.rs$' || echo SKIP_TRACING
```

If `SKIP_TRACING`, say so in one line and go to Step 6.

**Source of truth is the `tracing-best-practices` skill** (which itself encodes `docs/guides/tracing.md`). Don't restate its full checklist here — invoke/follow it against the diff's Rust files so the rules can't drift out of sync with this skill. The handful of checks that catch the most regressions on *this* repo:

- **New use case with no span** — the use-case layer (`uc-application`) must wrap execution in one `usecase.<name>.execute` span via `.instrument(span)`. A new use case missing it is the most common gap.
- **Span in the wrong layer** — a span created in `uc-core` (domain is zero-span), infra, or a Tauri command is a violation, not a gap.
- **Wrong log level** — `info!` = use-case start/success + key business transition; `debug!` = infra detail; `warn!` = recoverable / user-input error; `error!` = unrecoverable / corruption. Flag clear mismatches only.
- **Privacy / cardinality leak** — clipboard content, full settings, raw paths, blob contents, or gratuitous UUIDs in any span/log field. This is a bug, flag it high-confidence.
- **New `Err(...)` branch or state transition with no breadcrumb** — flag *only* genuinely new failure/transition paths, mark `confidence: low`, and don't add speculative "might-need-it-later" logs (the spec forbids those).

For each finding, emit:

```
- <file>:<line> — <which rule> — <what's wrong> → <suggested change>. Confidence: <high/low>.
```

Show the list to the user (or "tracing in this diff conforms — one use-case span, levels and fields look right"). Ask: apply now or defer? Applied tracing fixes go in the relevant code commit, not the docs commit.

### Step 6 — Build and open the PR

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

### Step 7 — Report back

Print the PR URL (`gh pr create` outputs it on success). Add a one-line recap:

- Title used.
- Whether docs were updated (and how many files), or the explicit "no docs changes" decision.
- Telemetry/tracing outcome: gaps fixed, or the explicit "nothing to instrument / tracing conforms" / "deferred" decision.
- Issue that will close on merge, if any.

Stop there. Don't auto-request reviewers, add labels, or run any other workflow unless the user asks.

## Anti-patterns

- **Skipping the docs scan because "it's just a refactor".** Refactors that rename a CLI flag, change an error message, or alter a default break docs the same way features do. Scan first; the diff tells you whether there's anything to update.
- **Listing every candidate doc edit as if it's mandatory.** The skill's output is a *menu*, not a checklist. Be honest about which entries are weak — if a candidate is "maybe relevant", say "low confidence" so the user can dismiss it fast.
- **Renaming the branch silently when an open PR exists for the old name.** That orphans the PR and confuses reviewers. Always check `gh pr list --head <old>` before renaming.
- **Folding the docs commit into a code commit via `--amend`.** Past-tense `--amend` on already-pushed commits forces `--force-push`. Keep docs as a separate commit; it makes review easier anyway.
- **Inventing facts about a referenced issue.** If the user says "closes #X", actually `gh issue view X` once — don't paraphrase the title from memory.
- **Inventing an `Event` variant.** The `Event` enum in `events.rs` is a closed, frozen set. Before suggesting `capture(Event::Foo)`, confirm `Foo` exists (or that §12 of the schema doc specs it). Never propose renaming an existing variant.
- **Auto-adding telemetry/tracing without asking.** Both reviews produce a *menu*. Adding a `capture(...)` or a span is a code change with privacy/cardinality implications — show the candidates and let the user decide; don't silently edit and fold it into an unrelated commit.
- **Treating every diff as needing telemetry.** Infra refactors, UI tweaks, and `uc-core` changes usually have nothing to instrument. Say "nothing to instrument" rather than forcing a weak event.
- **`gh pr create` without `--body` (interactive editor).** This skill runs non-interactively; always pass `--body` via heredoc.

## When this skill does *not* apply

- Pushing committed work that doesn't need a PR (a direct push to a personal branch you'll keep iterating on). Just `git push`; come back to this skill when you're ready to publish.
- Updating an existing PR (new commits on an already-open PR). Just `git push`; GitHub picks them up automatically. Use this skill only for the *initial* PR creation.
- Triggering release workflows — that's `trigger-prepare-release`.
- Reviewing someone else's PR — that's `/review` or `/ultrareview`.

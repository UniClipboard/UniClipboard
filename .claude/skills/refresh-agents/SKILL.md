---
name: refresh-agents
description: "Refresh fact-based sections of crates/AGENTS.md (STRUCTURE, WHERE TO LOOK, COMPLEXITY HOTSPOTS, KNOWN BROKEN) from current codebase state. Use after refactoring, renaming files, adding crates, or when the agent notices AGENTS.md info is stale."
---

# Refresh AGENTS.md

Bring the fact-based sections of `crates/AGENTS.md` up to date with the current codebase.

## When to Use

- After adding/removing/renaming crates (STRUCTURE — also handled by pre-commit hook)
- After moving or renaming key files (WHERE TO LOOK)
- Periodically or before a milestone (COMPLEXITY HOTSPOTS)
- After discovering tests that are broken on HEAD (KNOWN BROKEN)
- When you notice AGENTS.md references stale paths during a session

## Procedure

### Step 1 — Refresh STRUCTURE

Run the pre-commit script to sync crate inventory:

```bash
node scripts/refresh-agents-structure.mjs
```

### Step 2 — Refresh WHERE TO LOOK

Verify each entry in the `## WHERE TO LOOK` table still points to an existing file:

```bash
```

For each row in the table:
1. Check if the file path still exists (e.g., `crates/uc-tauri/src/run.rs`)
2. If moved/renamed, update the Location column
3. If deleted with no replacement, remove the row
4. If a new major entry point was added (e.g., new daemon binary, new CLI subcommand), add a row

Do NOT add every file — only keep entries for the ~10 most common "where do I start?" lookups.

### Step 3 — Refresh COMPLEXITY HOTSPOTS

Compute current hotspots using git churn + file size:

```bash
# Top 10 files by recent change frequency (last 30 commits)
git log --oneline -30 --name-only --pretty=format: -- 'crates/' | sort | uniq -c | sort -rn | head -10

# Files over 400 lines
find crates/ -name '*.rs' -exec wc -l {} + | sort -rn | head -15
```

Update `## COMPLEXITY HOTSPOTS` with files that are BOTH large (>300 lines) AND frequently changed. Keep to 4-6 entries max. Format:

```
- `path/to/file.rs`: one-sentence description of why it's complex.
```

### Step 4 — Refresh KNOWN BROKEN (optional)

Only update this if you know of currently-broken tests on HEAD:

```bash
cargo test --workspace --lib --no-fail-fast 2>&1 | grep "FAILED\|error\[" | head -10
```

If failures exist that are known/accepted (not regressions), list them:

```
## KNOWN BROKEN
- `uc-daemon-local` auth.rs doctests: 7 tests, known broken since ADR-008 migration
- `uc-platform` effective_mime doctest: fixture issue, skip with --lib
```

If all tests pass, either remove the section or write "None currently known."

### Step 5 — Update datestamp

Change the "Last refreshed" line at the top:

```
**Last refreshed:** YYYY-MM-DD (manual; <reason>)
```

### Step 6 — Commit

Stage and commit the updated file:

```bash
git add crates/AGENTS.md
git commit -m "docs(agents): refresh AGENTS.md fact sections"
```

## What NOT to change

- `## OVERVIEW` — rarely changes; only update on major architecture shifts
- `## CONVENTIONS` — updated by `/learn` skill, not here
- `## ANTI-PATTERNS` — updated by `/learn` skill, not here
- `## COMMANDS` — only update if build system actually changed
- `## NOTES` — historical notes, leave as-is unless clearly wrong

## Output

Report what changed:
- Which sections were updated
- How many entries were added/removed/modified
- Any stale entries that were removed

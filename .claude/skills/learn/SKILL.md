---
name: learn
description: "Record a lesson learned into crates/AGENTS.md (CONVENTIONS or ANTI-PATTERNS section). Use after discovering a project-specific gotcha, resolving a tricky bug, or when the user says 'remember this' about a coding pattern. Append one concise rule that will prevent the same mistake in future sessions."
---

# Learn — Record a Lesson

Append a project-specific lesson to `crates/AGENTS.md` so future sessions avoid the same mistake.

## When to Use

- After resolving a bug caused by a project-specific gotcha
- When the user explicitly says "remember this", "don't do that again", "add this to rules"
- After discovering a convention that differs from Rust/React defaults
- When a code review reveals a recurring pattern mistake
- Use PROACTIVELY when you notice you made an avoidable mistake that a rule would have prevented

## Procedure

### Step 1 — Identify the Lesson Type

| Type | Target Section | Criteria |
|------|---------------|----------|
| Convention | `## CONVENTIONS` | "Do X this way" — a positive pattern to follow |
| Anti-pattern | `## ANTI-PATTERNS` | "Never do Y" — a thing that looks correct but breaks |

### Step 2 — Qualify the Lesson

Before adding, verify ALL of these:

1. **Project-specific**: Would a competent Rust/React developer NOT know this without seeing this codebase? If it's general knowledge (e.g., "don't unwrap in async"), DON'T add it.
2. **Actionable**: Can it be stated as a clear do/don't rule? If it requires paragraphs of explanation, it belongs in `docs/`, not AGENTS.md.
3. **Not already covered**: Check existing CONVENTIONS and ANTI-PATTERNS entries. No duplicates.
4. **Not derivable from code**: If `cargo check` or a linter would catch it, DON'T add it.

If any check fails, inform the user why you're not adding it (or suggest the right place: docs/, VISION.md, memory, etc.)

### Step 3 — Draft the Entry

Format: One line, starts with a dash, concise. Under 120 characters preferred.

**Convention example:**
```
- Event payloads emitted via `app.emit()` must use `#[serde(rename_all = "camelCase")]`.
```

**Anti-pattern example:**
```
- Using `{param}` in axum 0.7 `.route()` — compiles but silently 404s; must use `:param`.
```

Rules for good entries:
- Lead with the WHAT (what to do / not do)
- No explanation of WHY in the entry itself (that's in git commit or docs)
- Include the specific technology/module if relevant
- Be grep-friendly (someone searching for "axum" or "emit" should find it)

### Step 4 — Append to the Right Section

Read `crates/AGENTS.md`, find the target section, and append the new entry at the end of the bullet list (before the next `##` heading).

### Step 5 — Trim if Needed

Each section should stay under 10 entries. If adding a new one pushes over 10:
1. Check if any existing entry is now redundant (covered by a linter, removed feature, etc.)
2. If so, remove the stale entry
3. If not, ask the user which older entry to drop or whether to exceed the limit

### Step 6 — Confirm

Show the user:
- The exact line added
- Which section it was added to
- The current total count for that section

Do NOT commit automatically — let the user decide when to commit.

## What NOT to Record Here

| Instead use... | For... |
|----------------|--------|
| `VISION.md` | Architectural principles, locked decisions, product direction |
| `docs/agent/architecture-rules.md` | Cross-crate dependency rules, commit structure |
| `docs/agent/rust-tauri-rules.md` | Detailed Rust/Tauri coding patterns |
| Claude memory (auto-memory) | User preferences, per-session context |
| ADR docs | Decision rationale with full context |

## Examples

User: "I just spent 30 minutes debugging because I used `{id}` in an axum route and it silently 404'd"
→ Add to ANTI-PATTERNS: `- Using {param} in axum 0.7 .route() — compiles but silently 404s; must use :param.`

User: "Remember that daemon API responses are always wrapped in ApiEnvelope"
→ Add to CONVENTIONS: `- Daemon HTTP responses use `ApiEnvelope<T>` wrapper; clients must unwrap `.data` field.`

User: "Stop importing from uc-application internals, only use facade/"
→ Add to ANTI-PATTERNS: `- Importing from `uc-application` internal modules — only `facade/` is public API for external crates.`

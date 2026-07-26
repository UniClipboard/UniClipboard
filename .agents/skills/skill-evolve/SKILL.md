---
name: skill-evolve
description: Meta-skill for evolving other skills. Invoke after resolving a real-world issue to extract lessons and merge them into the referenced SKILL.md. Usage - $skill-evolve @path/to/SKILL.md
---

# Skill Evolve

Turn real-world experience into durable skill knowledge.

## When to Use

User invokes this skill after completing work that revealed a gap, pitfall, or new pattern not yet captured in an existing SKILL.md. Typical trigger: `$skill-evolve @path/to/SKILL.md`

## Procedure

### Step 1 — Gather Context

1. Read the referenced SKILL.md in full.
2. Review the current conversation to identify:
   - **What went wrong or was discovered** (the incident/finding)
   - **Root cause** (why it happened)
   - **The fix or pattern** (what the correct approach is)
   - **Why the existing skill didn't prevent it** (the gap)

### Step 2 — Classify the Learning

Determine which type of update is needed:

| Type                   | Description                                                | Example                                             |
| ---------------------- | ---------------------------------------------------------- | --------------------------------------------------- |
| **New Rule**           | A pattern/anti-pattern not covered at all                  | `.entered()` in async is forbidden                  |
| **Rule Refinement**    | Existing rule needs caveats or edge cases                  | `skip_all` needed when params include trait objects |
| **New Example**        | Correct/incorrect code pattern to illustrate existing rule | `async {}.instrument(span).await` for match arms    |
| **Checklist Addition** | New review checkpoint                                      | "`.entered()` held across `.await`? FORBIDDEN"      |
| **Section Expansion**  | Existing section needs a new subsection                    | Adding async lifecycle patterns to spawn section    |

### Step 3 — Draft the Update

Write the proposed additions following these principles:

- **Match the existing style** — same heading levels, table formats, code block conventions
- **Place it where it belongs** — near related content, not appended randomly at the end
- **Lead with the rule, then the why, then the example**
- **Include both CORRECT and FORBIDDEN patterns** when adding anti-patterns
- **Keep it concise** — skills are reference material, not tutorials

### Step 4 — Apply and Summarize

1. Edit the SKILL.md with the new content.
2. Present a summary to the user:

```text
## Skill Evolution Summary

**Skill**: <skill name>
**Trigger**: <what happened that revealed the gap>
**Update type**: <New Rule | Rule Refinement | New Example | Checklist Addition | Section Expansion>
**What was added**:
- <bullet summary of each change>
**Section(s) modified**: <list of modified section numbers/names>
```

## Quality Gates

Before applying the update, verify:

- [ ] The new content does NOT duplicate existing rules (check the full SKILL first)
- [ ] The new content is grounded in a real incident from this conversation (not hypothetical)
- [ ] Code examples compile conceptually (correct syntax, realistic types)
- [ ] The learning is **generalizable** — useful beyond this one specific case
- [ ] If adding a FORBIDDEN pattern, a CORRECT alternative is shown alongside it

## Anti-Patterns for This Skill

- **Don't bloat**: If the learning is too narrow or one-off, suggest the user add it to memory instead of a skill
- **Don't rewrite**: Evolve the skill incrementally. Don't restructure or rewrite existing sections unless they are wrong
- **Don't speculate**: Only add patterns that were validated in practice during this conversation
- **Don't duplicate**: If the fix was specific to this codebase and not transferable, it belongs in the nearest `AGENTS.md`, not a skill

---
name: research-then-implement
description: "Agent Loop for tasks that need research before implementation: check existing research first, run structured investigation (bb-browser/context7/code study), produce a Brief, then implement against the Brief. Prevents repeated research on already-studied topics and research-to-implementation drift."
user-invocable: true
allowed-tools: Bash(git:*), Bash(grep:*), Bash(rg:*), Bash(jq:*), Bash(find:*), Bash(cat:*), Bash(bb-browser:*), Read, Edit, Write, AskUserQuestion, Agent, mcp__context7__resolve-library-id, mcp__context7__query-docs
---

# research-then-implement

## Purpose

An Agent Loop for tasks where the right approach isn't obvious and requires investigation before coding. It prevents two recurring problems:

1. **Repeated research**: Re-investigating topics that were already studied in prior sessions (e.g., researching Expo UI components twice because the first research wasn't captured)
2. **Research-implementation drift**: Research conclusions are in the agent's context but not in a durable artifact, so the implementation silently diverges from what was learned

**Before:**
```
Session N: [researches via bb-browser, learns best practices] → implements → session ends
Session N+1: [re-researches the same topic from scratch because findings weren't saved]
```

**After:**
```
Session N: /research-then-implement → checks .planning/research/ → no prior research →
           researches → writes BRIEF.md → implements against BRIEF → done
Session N+1: /research-then-implement → checks .planning/research/ → finds BRIEF.md →
           skips research → implements directly
```

## When to trigger

- `/research-then-implement` or `/rti` — start the full loop
- `/research-then-implement research-only` — just do the research, save Brief, don't implement
- User says "先研究一下再做", "research this first", "我不确定怎么做"
- Tasks involving unfamiliar frameworks, APIs, or design patterns
- Cross-platform porting (iOS → Android, native → Expo)
- Evaluating multiple approaches before choosing one

## When NOT to use

- The implementation approach is already clear → just do it
- Bug fixes with known root cause → use `systematic-debugging` or `error-diagnose-fix`
- Pure research without implementation → use `deep-research` skill instead

## Directory convention

```
.planning/research/
  <topic-slug>/
    BRIEF.md          ← the deliverable: findings + recommendation + constraints
    sources.md        ← raw notes, URLs, code snippets from investigation
```

This convention already partially exists in the project (`.planning/research/` has prior work). The skill formalizes it.

## Phase 1 — Check for existing research

Before doing any new research, check if the topic was already studied:

```bash
# Check .planning/research/ for matching topics
ls .planning/research/ 2>/dev/null

# Check memory for relevant entries
grep -i "<topic keywords>" \
  ~/.claude/projects/-Users-mark-MyProjects-uniclipboard/memory/MEMORY.md 2>/dev/null

# Check if there's a planning doc that covers this
find .planning -name "*.md" -newer .planning/research 2>/dev/null | \
  xargs grep -li "<topic>" 2>/dev/null
```

If existing research is found:
```
📚 Found existing research on this topic:
  .planning/research/expo-ui-migration/BRIEF.md (2026-06-21)

  Summary: Expo UI (@expo/ui) provides native iOS components...
  Recommendation: Use @expo/ui for iOS, custom components for Android
  
  A) Use this research and proceed to implementation
  B) Research is stale — redo from scratch
  C) Supplement — do additional research on specific gaps
```

## Phase 2 — Structured Investigation

If no existing research, or user wants fresh research, run the investigation.

### 2a — Define the research question

Clarify what needs to be learned:
```
Research question: How to implement a native-feel bottom sheet in Expo
                   that matches iOS UISheetPresentationController behavior?

Sub-questions:
  1. Does @expo/ui provide a native BottomSheet component?
  2. What's the standard pattern for iOS Sheet in React Native/Expo?
  3. How does the iOS 26 Liquid Glass design affect this?
  4. What are the platform-specific considerations (iOS vs Android)?
```

### 2b — Multi-source investigation

Use the appropriate tool for each source type:

**Official documentation (context7):**
```
mcp__context7__resolve-library-id("expo-ui")
mcp__context7__query-docs(id, "BottomSheet sheet presentation")
```

**Design trends and community practices (bb-browser):**
```bash
bb-browser site google/search "expo bottom sheet native iOS 2026 best practice"
bb-browser open <relevant-url>
bb-browser eval "document.querySelector('article')?.innerText?.substring(0, 5000)"
bb-browser close
```

**Reference implementations (code study):**
```bash
# If porting from an existing implementation:
find /Users/mark/MyProjects/iOSApp/UniClipboard -name "*.swift" | \
  xargs grep -l "sheet\|presentation" 2>/dev/null

# Read the reference implementation
Read <file>
```

**Package ecosystem:**
```bash
# Check what's available
bb-browser site google/search "npm expo bottom sheet native 2026"
```

### 2c — Record raw findings

Write sources and raw notes to `sources.md`:

```markdown
# Sources: <topic>

## Official docs
- expo-ui BottomSheet: [url] — supports detents, grabber, native iOS sheet
- ...

## Community
- Blog post: [url] — recommends X over Y because...
- GitHub issue: [url] — known limitation with...

## Reference implementation
- iOS app uses UISheetPresentationController with custom detents
- File: /Users/mark/.../ServerSwitcherView.swift:42-80
- Key pattern: .presentationDetents([.medium, .large])

## Raw code snippets
...
```

## Phase 3 — Produce the Brief

The Brief is the key deliverable — a concise, actionable document that locks the research findings.

### BRIEF.md template

```markdown
# Brief: <topic>

**Date:** <date>
**Status:** Draft | Reviewed | Locked
**Research question:** <the original question>

## Recommendation

<1-3 sentences: what to do and why>

## Key findings

1. <finding 1 — with source reference>
2. <finding 2>
3. <finding 3>

## Approach

<The specific implementation approach chosen>

### What to use
- <library/API/pattern>: <why>

### What NOT to use
- <rejected alternative>: <why not>

## Constraints

- <constraint from docs/API limitations>
- <constraint from project architecture>
- <platform-specific constraint>

## Implementation checklist

- [ ] <step 1>
- [ ] <step 2>
- [ ] <step 3>

## Cross-platform considerations

| Aspect | iOS | Android |
|--------|-----|---------|
| Component | ... | ... |
| Behavior | ... | ... |

## Open questions

- <anything not resolved by research>
```

### Brief quality gate

Before proceeding to implementation, verify:
1. The recommendation is specific enough to implement (not "use the best library")
2. At least one alternative was considered and rejected with reasons
3. Constraints are concrete (not "be careful about performance")
4. The checklist has actionable items

If the user passed `research-only`, **stop here**.

## Phase 4 — Implement against the Brief

### 4a — Load the Brief as constraints

The Brief is now the specification. Implementation must:
- Follow the recommended approach
- Respect all listed constraints
- Use the specified libraries/APIs (not alternatives)
- Check off items from the implementation checklist

### 4b — Implement

Standard implementation, but with one key rule:

**If during implementation you discover the Brief's recommendation doesn't work** (API doesn't exist, behavior differs from docs, breaking change):

1. **STOP** — don't silently work around it
2. Update the Brief with the new finding
3. Tell the user: "The Brief said X, but in practice Y. Updated the Brief. Proceeding with Z instead."

### 4c — Mark Brief as complete

After implementation:
```markdown
**Status:** Locked
**Implemented:** 2026-06-21
**Branch:** feature/bottom-sheet-native
```

Update the implementation checklist with completion marks.

## Cross-platform porting (special case)

When the task is porting from one platform to another (iOS → Expo, Swift → React Native):

### Transfer Spec

Before implementation, create a Transfer Spec that maps source → target:

```markdown
## Transfer Spec: iOS → Expo BottomSheet

| iOS (source) | Expo (target) | Notes |
|-------------|---------------|-------|
| UISheetPresentationController | @expo/ui Sheet | Native on iOS |
| .presentationDetents([.medium]) | snapPoints={['50%']} | Different API |
| .prefersGrabberVisible(true) | enableGrabber={true} | Same concept |
| UIAction in menu | Context menu from @expo/ui | Not 1:1 |

### Behavioral parity checklist
- [ ] Half-screen snap point
- [ ] Full-screen expansion
- [ ] Drag-to-dismiss
- [ ] Grabber indicator
- [ ] Background dimming
```

This Transfer Spec is reusable — if the same port needs refinement later, the mapping is already done.

## Interaction with other skills

| Skill | Relationship |
|-------|-------------|
| `deep-research` | For pure research questions without implementation intent |
| `bb-browser` | Tool used in Phase 2 for web research |
| `context7` | Tool used in Phase 2 for official docs |
| `/wrap` | Should persist the Brief path in `context_refs` |
| `/continue` | Should read the Brief on resume |
| `codebase-design` | May inform the Brief's constraints section |

## Anti-patterns

- Researching and implementing in the same mental breath (research, then implement — don't interleave)
- Re-researching a topic that has a valid Brief in `.planning/research/`
- Writing a Brief so vague it doesn't constrain implementation ("use best practices")
- Silently deviating from the Brief during implementation without updating it
- Spending 3 rounds researching when the topic is well-documented (use context7 first)
- Researching without producing a Brief (findings evaporate with the session)
- Porting code line-by-line instead of understanding the design intent first

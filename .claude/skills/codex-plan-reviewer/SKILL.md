---
name: codex-plan-reviewer
description: Cross-model adversarial review loop for markdown plans. Use this skill whenever the user asks to review, validate, or stress-test a markdown plan file using Codex (OpenAI). Triggers on phrases like "review this plan with codex", "let codex check the plan", "adversarial review", "cross-model review", "codex audit", "validate plan", or when the user has a markdown plan and wants a second opinion from a different AI model. Also trigger when the user says "plan review loop", "codex review loop", or references having Codex critique a plan before execution.
---

# Codex Plan Reviewer

## Purpose

Use OpenAI Codex CLI as an adversarial reviewer for markdown plan files authored by Claude Code. The two models engage in a structured review loop: Codex critiques the plan, Claude Code evaluates each piece of feedback, applies what's valid, escalates disagreements to the user, and resubmits until Codex approves — or the loop cap is reached.

This creates a cross-model checks-and-balances system where neither model operates unchecked.

## Prerequisites

- `codex` CLI installed and authenticated (`npm install -g @openai/codex` or equivalent)
- The plan must be a markdown file (`.md`) accessible in the working directory or a provided path

Before starting, verify codex is available:

```bash
command -v codex >/dev/null 2>&1 || { echo "ERROR: codex CLI not found. Install with: npm install -g @openai/codex"; exit 1; }
```

## Workflow

### Step 0: Locate the Plan

Identify the target markdown plan file. It could be:

- Explicitly provided by the user: "review `docs/plan.md`"
- The most recently created/modified `.md` file in the project
- A plan Claude Code just finished writing in the current session

Read the plan and confirm with the user: "I'll send `<filename>` to Codex for review. Proceed?"

### Step 1: Spawn a Subagent to Run Codex Review

**CRITICAL: Each review round MUST run inside a subagent** using the Agent tool. This keeps the main conversation context clean and prevents long codex outputs from polluting the primary thread.

Spawn a `general-purpose` subagent with a prompt like:

```
You are running a Codex plan review round. Your job:

1. Call the codex_review.py script via Bash:
   python3 <skill-path>/scripts/codex_review.py \
     --plan-file "<path-to-plan.md>" \
     --round <N> \
     --max-rounds 10 \
     --output-dir "<workspace>/review-rounds" \
     [--prior-context "<workspace>/review-log.md"]

2. Read the result JSON from <workspace>/review-rounds/round-<N>/result.json
3. Read the raw Codex response from <workspace>/review-rounds/round-<N>/codex-response.md
4. Return a concise summary containing:
   - The verdict (APPROVED or NEEDS_REVISION)
   - Each finding: ID, severity, description, and suggestion
   - Any errors encountered

Do NOT evaluate or apply the findings — just report them back.
```

The script constructs a review prompt that asks Codex to:

1. Identify logical gaps, missing edge cases, or flawed assumptions
2. Flag ambiguous or under-specified sections
3. Check feasibility and internal consistency
4. Assess whether the plan is ready for execution
5. Return a structured verdict: `APPROVED` or `NEEDS_REVISION` with numbered findings

If the script is unavailable, the subagent can call codex directly:

```bash
cat <<PROMPT | codex exec --full-auto -
You are a senior technical reviewer. Review the following plan and provide structured feedback.

For each issue found, output in this exact format:
  FINDING-<N>: <severity: CRITICAL|MAJOR|MINOR>
  <description of the issue>
  SUGGESTION: <concrete fix>

At the end, output exactly one of:
  VERDICT: APPROVED — this plan is ready for execution
  VERDICT: NEEDS_REVISION — the issues above must be addressed

Here is the plan:
$(cat <path-to-plan.md>)
PROMPT
```

### Step 2: Process Subagent Results

When the subagent returns, extract the structured findings from its response. Each finding has:

- **ID**: `FINDING-1`, `FINDING-2`, etc.
- **Severity**: `CRITICAL`, `MAJOR`, or `MINOR`
- **Description**: What the issue is
- **Suggestion**: How to fix it

Also check the **verdict**: `APPROVED` or `NEEDS_REVISION`.

If the verdict is `APPROVED`, skip to Step 5 (wrap-up).

### Step 3: Evaluate Each Finding

For each finding, Claude Code (in the main conversation) makes an independent judgment:

#### 3a. AGREE — The finding is valid

Apply the fix to the plan. Log the change:

```markdown
## Round <N> — Finding <ID>: ACCEPTED

- **Issue**: <description>
- **Action**: <what was changed in the plan>
```

#### 3b. DISAGREE — The finding seems incorrect or inappropriate

Do NOT silently ignore it. Escalate to the user with full context:

```markdown
## Disagreement on Finding <ID> (<severity>)

**Codex says**: <description>
**Codex suggests**: <suggestion>

**My assessment**: <why I think this is wrong, with reasoning>

**Options**:

1. Accept Codex's suggestion anyway — I'll modify the plan
2. Reject and keep current plan — I'll note the rejection in the review log
3. Modify differently — Tell me what you'd prefer
```

Wait for user input before proceeding. Record the decision **with reasoning** in the review log:

```markdown
## Round <N> — Finding <ID>: REJECTED

- **Codex issue**: <description>
- **Codex suggestion**: <suggestion>
- **Rejection reason**: <why the suggestion was not adopted — from CC assessment and/or user input>
```

#### 3c. PARTIALLY AGREE — Valid concern but different fix preferred

Explain to the user what you'd change differently, and ask for confirmation:

```markdown
## Partial Agreement on Finding <ID>

**Codex says**: <description>
**Codex suggests**: <suggestion>
**My proposed alternative**: <different fix with reasoning>

Accept my alternative, or use Codex's original suggestion?
```

Record the decision **with reasoning** in the review log:

```markdown
## Round <N> — Finding <ID>: PARTIALLY ACCEPTED

- **Codex issue**: <description>
- **Codex suggestion**: <suggestion>
- **Alternative applied**: <what was actually changed and why it differs from Codex's suggestion>
```

### Step 4: Resubmit for Next Round (Cross-Model Discussion)

**CRITICAL**: The resubmission must carry full decision context so Codex can understand _why_ certain suggestions were rejected or modified. This enables a genuine cross-model discussion rather than a one-sided review loop.

After all findings are processed and the plan is updated:

1. Increment the round counter
2. Check if round > 10 (max rounds). If so, go to Step 5 with a timeout notice
3. Save the updated plan as `<workspace>/review-rounds/round-<N>/plan-after-revision.md`
4. Append the current round's decisions to `<workspace>/review-log.md` (this is the prior context for the next round)
5. **Spawn a new subagent** (repeat from Step 1) with the updated round number and `--prior-context` pointing to the review log

The review log passed as `--prior-context` allows Codex to see the full decision history. The subagent's resubmission prompt to Codex will include:

```
This is round <N> of review. The prior context below contains the full decision log from
previous rounds, including which findings were ACCEPTED, REJECTED (with reasons), or
PARTIALLY ACCEPTED (with alternative fixes and rationale).

When you encounter a previously rejected or modified suggestion:
- If the rejection reason is valid, do NOT re-raise the same issue.
- If you believe the rejection reason is flawed or the alternative fix is insufficient,
  you MAY re-raise with a COUNTERARGUMENT that specifically addresses the stated reason.
  Use this format:
    FINDING-<N>: <CRITICAL|MAJOR|MINOR> [RE-RAISED]
    Previously raised in Round <M> as FINDING-<K>, rejected because: <stated reason>
    COUNTERARGUMENT: <why the rejection reason is insufficient or the alternative is flawed>
    SUGGESTION: <revised suggestion that addresses the concerns>

Focus on:
- Whether previously accepted fixes adequately resolve the original issues
- Any NEW issues introduced by revisions
- Genuine disagreements where the rejection rationale may be incorrect

===== PRIOR REVIEW DECISIONS =====
<content of review-log.md>
===== END PRIOR DECISIONS =====

Please review the UPDATED plan below.
If all concerns are adequately addressed and no new critical issues exist, respond with VERDICT: APPROVED.
```

### Step 5: Wrap Up

When the loop ends (either `APPROVED` or max rounds reached), produce a summary:

```markdown
# Plan Review Summary

- **File**: <plan filename>
- **Rounds**: <N> of 10
- **Final Verdict**: <APPROVED | MAX_ROUNDS_REACHED>

## Review History

### Round 1

- Finding 1 (MAJOR): <desc> → ACCEPTED, plan modified
- Finding 2 (MINOR): <desc> → REJECTED by user (reason: ...)

### Round 2

- Finding 1 (MINOR): <desc> → ACCEPTED
- VERDICT: APPROVED

## Statistics

- Total findings: <count>
- Accepted: <count>
- Rejected: <count>
- User-escalated: <count>
```

Save this summary to `<workspace>/review-summary.md`.

If max rounds reached without approval, tell the user clearly:

```
⚠️ Codex did not approve the plan after 10 rounds.
Remaining concerns: <list>
You may want to review these manually or refine the plan further before proceeding.
```

## Review Log Format

Maintain a running log at `<workspace>/review-log.md` across all rounds:

```markdown
# Review Log: <plan filename>

Started: <timestamp>

## Round 1 — <timestamp>

### Codex Verdict: NEEDS_REVISION

| Finding | Severity | CC Decision | User Override | Action               |
| ------- | -------- | ----------- | ------------- | -------------------- |
| F-1     | CRITICAL | AGREE       | —             | Modified section 3.2 |
| F-2     | MAJOR    | DISAGREE    | REJECT        | Kept original        |

### Plan diff:

<brief description of changes made>

## Round 2 — <timestamp>

...
```

## Edge Cases

### Codex returns unparseable output

If the Codex response doesn't follow the expected format:

1. Save the raw response for the user to review
2. Attempt best-effort extraction of any identifiable concerns
3. Ask the user: "Codex returned unstructured feedback. Want me to interpret it as best I can, or retry the round?"

### Codex CLI errors or timeouts

```bash
# Retry once with a simpler prompt if codex fails
if [ $? -ne 0 ]; then
    echo "Codex CLI failed. Retrying with simplified prompt..."
    # retry logic
fi
```

If Codex fails twice, report to the user and offer to skip this round or abort.

### Plan is very long (>5000 words)

For large plans, consider splitting into sections and reviewing individually, then doing a final holistic pass. Warn the user that large plans may produce lower-quality reviews due to context limits.

### All findings in a round are rejected

If CC disagrees with every finding and the user confirms rejection of all, still resubmit. Include the rejection context so Codex can reassess. If the same findings keep recurring across rounds, flag this pattern to the user — it likely indicates a genuine disagreement between models that needs human judgment.

## Configuration

The skill uses these defaults, overridable by the user:

| Parameter           | Default                    | Description                                         |
| ------------------- | -------------------------- | --------------------------------------------------- |
| `max-rounds`        | 10                         | Maximum review-revision cycles                      |
| `severity-filter`   | all                        | Review all severities, or only CRITICAL+MAJOR       |
| `auto-accept-minor` | false                      | Auto-apply MINOR findings without user confirmation |
| `workspace`         | `./codex-review-workspace` | Directory for review artifacts                      |

## Safety Principles

1. **Never blindly accept Codex feedback** — CC independently evaluates every finding
2. **Human-in-the-loop for disagreements** — When CC and Codex disagree, the user decides
3. **Full audit trail** — Every decision, accepted or rejected, is logged with reasoning
4. **Bounded loops** — Hard cap at 10 rounds prevents infinite back-and-forth
5. **Transparency** — User sees exactly what Codex said, what CC thinks, and what changed

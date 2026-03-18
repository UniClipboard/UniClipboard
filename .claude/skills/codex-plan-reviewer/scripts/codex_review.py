#!/usr/bin/env python3
"""
codex_review.py — Sends a markdown plan to OpenAI Codex CLI for structured review.

Usage:
    python3 codex_review.py \
        --plan-file path/to/plan.md \
        --round 1 \
        --max-rounds 10 \
        --output-dir ./review-rounds \
        [--prior-context path/to/review-log.md] \
        [--severity-filter all|critical+major]
"""

import argparse
import os
import subprocess
import sys
import json
import re
from datetime import datetime, timezone
from pathlib import Path


REVIEW_PROMPT_ROUND1 = """\
You are a senior technical reviewer performing round {round} of {max_rounds} of an adversarial review.

Your job is to critically examine the plan below and find:
1. Logical gaps or flawed assumptions
2. Missing edge cases or error handling
3. Ambiguous or under-specified sections
4. Feasibility concerns (can this actually be built as described?)
5. Internal contradictions or inconsistencies
6. Missing dependencies or prerequisites

For each issue, output in this EXACT format (one block per issue):

FINDING-<N>: <CRITICAL|MAJOR|MINOR>
<Clear description of the problem>
SUGGESTION: <Concrete, actionable fix>

After all findings, output exactly ONE of these verdicts:
VERDICT: APPROVED — this plan is ready for execution
VERDICT: NEEDS_REVISION — the issues above must be addressed first

===== PLAN TO REVIEW =====

{plan_content}
"""

REVIEW_PROMPT_FOLLOWUP = """\
You are a senior technical reviewer performing round {round} of {max_rounds} of an adversarial review.

This is a FOLLOW-UP round. The prior decision log below contains the full history from previous \
rounds, including which of your findings were ACCEPTED, REJECTED (with reasons), or PARTIALLY \
ACCEPTED (with alternative fixes and rationale).

When you encounter a previously rejected or modified suggestion:
- If the rejection reason is valid, do NOT re-raise the same issue.
- If you believe the rejection reason is flawed or the alternative fix is insufficient, \
you MAY re-raise with a counterargument that specifically addresses the stated reason. Use this format:
    FINDING-<N>: <CRITICAL|MAJOR|MINOR> [RE-RAISED]
    Previously raised in Round <M> as FINDING-<K>, rejected because: <stated reason>
    COUNTERARGUMENT: <why the rejection reason is insufficient or the alternative is flawed>
    SUGGESTION: <revised suggestion that addresses the concerns>

For NEW issues, use the standard format:

FINDING-<N>: <CRITICAL|MAJOR|MINOR>
<Clear description of the problem>
SUGGESTION: <Concrete, actionable fix>

Focus on:
- Whether previously accepted fixes adequately resolve the original issues
- Any NEW issues introduced by revisions
- Genuine disagreements where the rejection rationale may be incorrect

After all findings, output exactly ONE of these verdicts:
VERDICT: APPROVED — this plan is ready for execution
VERDICT: NEEDS_REVISION — the issues above must be addressed first

===== PRIOR REVIEW DECISIONS =====
{prior_context}
===== END PRIOR DECISIONS =====

===== PLAN TO REVIEW =====

{plan_content}
"""


def read_file(path: str) -> str:
    with open(path, "r", encoding="utf-8") as f:
        return f.read()


def ensure_dir(path: str):
    Path(path).mkdir(parents=True, exist_ok=True)


def build_prompt(plan_content: str, round_num: int, max_rounds: int, prior_context: str | None = None) -> str:
    if prior_context and round_num > 1:
        return REVIEW_PROMPT_FOLLOWUP.format(
            round=round_num,
            max_rounds=max_rounds,
            prior_context=prior_context,
            plan_content=plan_content,
        )
    return REVIEW_PROMPT_ROUND1.format(
        round=round_num,
        max_rounds=max_rounds,
        plan_content=plan_content,
    )


def call_codex(prompt: str) -> tuple[int, str]:
    """Call codex CLI and return (returncode, output)."""
    try:
        result = subprocess.run(
            ["codex", "exec", "--full-auto", "--ephemeral", "-"],
            input=prompt,
            capture_output=True,
            text=True,
            timeout=300,  # 5 minute timeout
        )
        output = result.stdout.strip()
        if result.returncode != 0:
            stderr = result.stderr.strip()
            return result.returncode, f"CODEX_ERROR: {stderr}\n{output}"
        return 0, output
    except subprocess.TimeoutExpired:
        return 1, "CODEX_ERROR: Timeout after 300 seconds"
    except FileNotFoundError:
        return 127, "CODEX_ERROR: codex CLI not found. Install with: npm install -g @openai/codex"


def parse_findings(response: str) -> list[dict]:
    """Extract structured findings from codex response."""
    findings = []
    # Match FINDING-N: SEVERITY\ndescription\nSUGGESTION: suggestion
    pattern = r'FINDING-(\d+):\s*(CRITICAL|MAJOR|MINOR)\s*\n(.*?)(?=SUGGESTION:)\s*SUGGESTION:\s*(.*?)(?=FINDING-\d+:|VERDICT:|$)'
    matches = re.finditer(pattern, response, re.DOTALL | re.IGNORECASE)

    for match in matches:
        findings.append({
            "id": f"F-{match.group(1)}",
            "severity": match.group(2).upper(),
            "description": match.group(3).strip(),
            "suggestion": match.group(4).strip(),
        })

    return findings


def parse_verdict(response: str) -> str:
    """Extract verdict from codex response."""
    match = re.search(r'VERDICT:\s*(APPROVED|NEEDS_REVISION)', response, re.IGNORECASE)
    if match:
        return match.group(1).upper()
    return "UNKNOWN"


def main():
    parser = argparse.ArgumentParser(description="Send a plan to Codex for review")
    parser.add_argument("--plan-file", required=True, help="Path to the markdown plan file")
    parser.add_argument("--round", type=int, default=1, help="Current review round number")
    parser.add_argument("--max-rounds", type=int, default=10, help="Maximum number of rounds")
    parser.add_argument("--output-dir", default="./review-rounds", help="Directory for review artifacts")
    parser.add_argument("--prior-context", default=None, help="Path to review log for context in subsequent rounds")
    parser.add_argument("--severity-filter", default="all", choices=["all", "critical+major"],
                       help="Filter findings by severity")
    args = parser.parse_args()

    # Validate inputs
    if not os.path.isfile(args.plan_file):
        print(f"ERROR: Plan file not found: {args.plan_file}", file=sys.stderr)
        sys.exit(1)

    if args.round > args.max_rounds:
        print(f"ERROR: Round {args.round} exceeds max rounds {args.max_rounds}", file=sys.stderr)
        sys.exit(1)

    # Read plan
    plan_content = read_file(args.plan_file)
    word_count = len(plan_content.split())
    if word_count > 5000:
        print(f"WARNING: Plan is {word_count} words. Large plans may produce lower-quality reviews.", file=sys.stderr)

    # Read prior context if provided
    prior_context = None
    if args.prior_context and os.path.isfile(args.prior_context):
        prior_context = read_file(args.prior_context)

    # Build prompt and call codex
    prompt = build_prompt(plan_content, args.round, args.max_rounds, prior_context)

    print(f"=== Codex Review Round {args.round}/{args.max_rounds} ===", file=sys.stderr)
    print(f"Plan: {args.plan_file} ({word_count} words)", file=sys.stderr)
    print(f"Calling codex CLI...", file=sys.stderr)

    returncode, response = call_codex(prompt)

    # Set up output directory
    round_dir = os.path.join(args.output_dir, f"round-{args.round}")
    ensure_dir(round_dir)

    # Save raw response
    response_path = os.path.join(round_dir, "codex-response.md")
    with open(response_path, "w", encoding="utf-8") as f:
        f.write(response)

    if returncode != 0:
        print(f"ERROR: Codex CLI failed (exit code {returncode})", file=sys.stderr)
        print(response, file=sys.stderr)
        # Save error info
        error_info = {
            "status": "error",
            "round": args.round,
            "error_code": returncode,
            "error_message": response,
            "timestamp": datetime.now(timezone.utc).isoformat(),
        }
        with open(os.path.join(round_dir, "result.json"), "w") as f:
            json.dump(error_info, f, indent=2)
        sys.exit(1)

    # Parse response
    findings = parse_findings(response)
    verdict = parse_verdict(response)

    # Apply severity filter
    if args.severity_filter == "critical+major":
        findings = [f for f in findings if f["severity"] in ("CRITICAL", "MAJOR")]

    # Save structured result
    result = {
        "status": "success",
        "round": args.round,
        "max_rounds": args.max_rounds,
        "plan_file": args.plan_file,
        "verdict": verdict,
        "finding_count": len(findings),
        "findings": findings,
        "severity_summary": {
            "critical": len([f for f in findings if f["severity"] == "CRITICAL"]),
            "major": len([f for f in findings if f["severity"] == "MAJOR"]),
            "minor": len([f for f in findings if f["severity"] == "MINOR"]),
        },
        "timestamp": datetime.now(timezone.utc).isoformat(),
        "raw_response_path": response_path,
    }

    result_path = os.path.join(round_dir, "result.json")
    with open(result_path, "w", encoding="utf-8") as f:
        json.dump(result, f, indent=2, ensure_ascii=False)

    # Copy current plan state to round dir
    plan_snapshot = os.path.join(round_dir, "plan-snapshot.md")
    with open(plan_snapshot, "w", encoding="utf-8") as f:
        f.write(plan_content)

    # Output JSON to stdout for CC to consume
    print(json.dumps(result, indent=2, ensure_ascii=False))

    print(f"\nVerdict: {verdict}", file=sys.stderr)
    print(f"Findings: {len(findings)} ({result['severity_summary']})", file=sys.stderr)
    print(f"Results saved to: {round_dir}", file=sys.stderr)


if __name__ == "__main__":
    main()

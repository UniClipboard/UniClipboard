---
name: maintain-docs
description: Operate the unic documentation loop — propose a decision, land a finished decision into living docs (docs/architecture, docs/contracts, AGENTS.md), archive it, supersede it, or audit stale pages. Use when a decision is implemented, when asked to "落地/归档 decision", to write or refresh an architecture page, or when `go run ./tools/docscheck` reports stale/broken docs.
---

# maintain-docs

Rules of the loop (authoritative text: `docs/decisions/README.md`):

- Living docs = `docs/architecture/` (what the system is now) + `docs/contracts/` (exact specs) + the 约定 in `AGENTS.md`. They are the **only** thing code, READMEs and agents may cite.
- Decisions answer "why". `proposed/` = in flight / not yet landed. `archive/` = landed, read-only, **never linked** from anything live.
- A decision may only move to `archive/` after its conclusions live in living docs, verified against code.
- Always finish with `go run ./tools/docscheck` (must print `ok`; stale warnings for pages you touched must be gone).

Pick the operation from the user's request. If ambiguous, run `go run ./tools/docscheck` and `git status` first to see the state.

## Operation: audit

1. `go run ./tools/docscheck` — list errors and `stale` warnings.
2. For each stale page: `git diff <verified>..HEAD -- <paths>` to see what changed; decide whether the page's 现状/不变量/代码地图 are still true. Fix the page if not.
3. `go run ./tools/docscheck -bump docs/architecture/<page>.md` (needs the code paths committed; it records HEAD).
4. Re-run docscheck.

## Operation: propose

1. Next number = highest in `docs/decisions/proposed/` + `archive/` + 1. File `docs/decisions/proposed/NNNN-<slug>.md`, status `- 状态：草案` or `已采纳，待实现`. Cover context, options, the choice and why.
2. Add a row to the ledger table in `docs/decisions/README.md` (target page column = the architecture page that will absorb it).
3. Add a row to the 进行中的决策 table in `AGENTS.md`. Progress tracking goes in `docs/exec/`, not in the decision.

## Operation: land (decision implemented → living docs)

Precondition: implementation and its tests are done. If something is unimplemented, it goes in the page's 缺口 section, not into 现状.

1. **Gather**: read the decision, the contracts it names, `docs/exec/` plan if any, and the ledger's target page(s). Identify what is still true, what was cancelled/changed during implementation (the status line and dated notes usually say), and the hard rules.
2. **Draft from code, not from the decision**: for each claim, locate the code and confirm. Write/extend `docs/architecture/<topic>.md` following the template in `docs/architecture/README.md` (front matter `paths` + `verified`, 现状, 不变量, 代码地图, 规格与背景, 缺口). Short, factual, one screen and a half. Link contracts with relative links; do **not** link the decision. Don't copy rationale — only invariants a newcomer must respect.
3. **Independent fact-check**: spawn a fresh `Explore` or `general-purpose` agent. Give it the drafted page and the code paths, not the decision's conclusions, and ask it to list every claim that is wrong, unverifiable, or missing (entry points, invariants violated by code, stale names). Fix the findings. This is what keeps pages true; do not skip it for non-trivial pages.
4. **Move contract-level detail and hard rules**: exact DTOs/protocols → `docs/contracts/` (update the existing contract, don't duplicate into the page). Cross-cutting rules every contributor must follow → `AGENTS.md` 约定 (sparingly, one line each).
5. **Repoint references**: grep the repo for the decision's number/path (`grep -rn "NNNN-" --exclude-dir=.git .`) in code comments, READMEs, contracts, `docs/exec/`, AGENTS.md; replace each with the architecture page or contract that now carries the fact. Contract "依据" lines that cite the decision should cite the page/contract instead.
6. Register the page in the 索引 list of `docs/architecture/README.md`, and remove the decision's row from AGENTS.md's 进行中的决策 table (add a line for the page under 文档目录 if the topic is new).
7. Commit the code state, then `go run ./tools/docscheck -bump docs/architecture/<page>.md`.
8. Continue with **archive**.

## Operation: archive

Precondition: land is complete (page exists, docscheck passes with no reference to the decision from live files).

1. `git mv docs/decisions/proposed/NNNN-*.md docs/decisions/archive/`.
2. In that file: change the status line to `- 状态：已归档（YYYY-MM-DD）`; add right below it `- 落地：[<页名>](../../architecture/<页>.md)、[<契约名>](../../contracts/<x>.md)` (at least one existing architecture/contract page); at the top of the body note any parts that were cancelled or changed since (e.g. "自动写回已取消，见…"). Don't rewrite the historical reasoning.
3. Update the ledger row in `docs/decisions/README.md`: link → `archive/…`, state → `archive`.
4. Run `go run ./tools/docscheck`; fix anything it reports (live docs still linking the archived file are the usual cause — repoint them, don't allow-list).
5. Report to the user: pages written/updated, references repointed, what was left in 缺口.

## Operation: supersede / change of mind

- Behaviour change with the same rationale: edit the living docs, `-bump`, done — no new decision.
- Rationale reversed: write a new decision in `proposed/` that names the old one; when it lands, mark the old archived file's status `已归档，被 NNNN 取代` and mention it in the new page's 规格与背景 only if newcomers need the history.

## Guardrails

- Never put "已实现/未做" progress lines into decisions or architecture 现状; gaps go to 缺口, progress to `docs/exec/`.
- Don't archive drafts or decisions whose implementation is partial: land the finished part (gaps stay listed) and keep the decision in `proposed/` until the whole decision is done, unless the user explicitly says the remainder is a separate effort — then split the remainder into a new proposed decision.
- Don't edit decision reasoning to "fix" history; only status, 落地 and a note on what changed.
- Unclear intent: if code and decision disagree and the code's intent is unclear, ask the user rather than pick one.

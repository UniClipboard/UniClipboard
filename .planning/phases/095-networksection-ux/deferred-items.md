# Phase 95 — Deferred Items

Items discovered during execution that are out of scope for the current plan
but need follow-up handling.

## DEF-95-03-01: README.md:222 contains forbidden marketing word "fully offline"

**Discovered during:** Plan 03 Task 3 audit (`grep -rnE "fully offline|..." README.md`)

**Location:** `README.md:222`
> **Does it work fully offline / LAN-only?**
> Yes. Devices on the same Wi-Fi connect directly without going through the relay...

**Why deferred (out of scope for Plan 03):**

- Plan 03 modifies only `src/i18n/locales/{zh-CN,en-US}.json` (declared in
  `<files_modified>` frontmatter and Task 1/2 acceptance criteria).
- The Plan 03 `<verify>` `automated` clause for Task 3 is **strictly scoped to
  `src/i18n/`** — that scope returns 0 matches (PASS).
- `README.md` is a **Phase 97 surface** (DOC-01 `docs/lan-only.md` + DOC-03
  changelog reverse-copy from Phase 95 i18n; the README LAN-only FAQ is in the
  same documentation family).
- Phase 97 will rewrite this section using the Phase 95 i18n as the canonical
  source of truth, with Pitfall 5 reviewer checklist enforcement.
- Editing `README.md` here would silently expand Plan 03 scope and produce a
  commit that crosses the i18n / docs boundary set by ROADMAP §Phase 95 vs
  §Phase 97.

**Forwarded to:** Phase 97 — `docs/lan-only.md` (DOC-01) / changelog (DOC-03)
plan(s) **must** include a sub-task to rewrite `README.md:222` FAQ entry to:

1. Replace "Does it work fully offline / LAN-only?" with a phrasing that does
   NOT use "fully offline" (e.g. "Does it work without internet / in LAN-only
   mode?" — final wording set in Phase 97 by reviewer-checklist gate).
2. Replace the "Yes." opener with a transparency-boundary aware answer that
   references the same 4-class disclosure (rendezvous / OTLP / pkarr /
   auto-update) from Phase 95 i18n.

**Severity:** Medium (Pitfall 5 marketing-language violation visible in
public-facing README, but no immediate UX risk — feature not yet shipped).

**Owner:** Phase 97 planner.

---

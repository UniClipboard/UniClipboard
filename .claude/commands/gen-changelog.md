Generate a user-facing changelog for the current release.

## Input

- `$ARGUMENTS`: base reference to diff against (tag or commit hash, e.g. `v0.2.0`). If not provided, ask the user.

## Steps

1. **Read the changelog template** at `docs/CHANGELOG_TEMPLATE.md` to understand the format and rules.

2. **Get the current version** from `src-tauri/tauri.conf.json` (the `version` field).

3. **Collect commits** since the base reference:

   ```
   git log <base>..HEAD --oneline
   ```

   Squash-merge commits look like `feat(scope): summary (#778)` — the trailing `(#778)` is the PR number you must capture for each entry. Also inspect full commit messages (`git show <hash>`) when needed to understand the scope of changes. Skip `release:` commits (the merge commit that cuts the version itself); their PR is the release PR, not a user-visible change.

4. **Consolidate changes by PR/intent**: Multiple commits from the same PR or addressing the same issue MUST be merged into a single changelog entry. Do NOT list each commit separately and do NOT let the same `(#PR)` appear twice in the same section — describe the user-visible outcome once.

5. **Classify** each entry by conventional commit type per the template rules (feat→Features, fix→Fixes, etc.). Skip `chore:`, `ci:`, `test:`, `refactor:`, `docs:` commits unless they produce a user-visible change.

6. **Append the PR number** to every entry in the form ` (#123)` at the end of the line. Take the number from the squash commit's trailing `(#NNN)`. Do NOT add contributor handles — New Contributors are surfaced in a separate GitHub Release section.

7. **Write the changelog** in English to `docs/changelog/{version}.md`, and in Chinese to `docs/changelog/{version}.zh.md`. Follow the template format exactly:
   - Only include sections that have content
   - Use today's date (YYYY-MM-DD)
   - Descriptions should be concise and user-facing (explain the impact, not the implementation)
   - Each entry ends with ` (#PR)`

8. **Show the user** the generated content for review before finishing.

## Key Rules

- One PR = one changelog entry per section, even if it contains multiple commits
- Every bullet ends with ` (#PR)`; never duplicate the same `(#PR)` within a section
- Write from the user's perspective: what was broken, what's new, what improved
- Keep descriptions concise but informative
- Chinese version should be natural Chinese, not a literal translation

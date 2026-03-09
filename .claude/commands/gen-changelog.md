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

   Also inspect full commit messages (`git show <hash>`) when needed to understand the scope of changes.

4. **Consolidate changes by PR/intent**: Multiple commits from the same PR or addressing the same issue should be merged into a single changelog entry. Do NOT list each commit separately — describe the user-visible outcome.

5. **Classify** each entry by conventional commit type per the template rules (feat→Features, fix→Fixes, etc.). Skip `chore:` commits.

6. **Write the changelog** in English to `docs/changelog/{version}.md`, and in Chinese to `docs/changelog/{version}.zh.md`. Follow the template format exactly:
   - Only include sections that have content
   - Use today's date (YYYY-MM-DD)
   - Descriptions should be concise and user-facing (explain the impact, not the implementation)

7. **Show the user** the generated content for review before finishing.

## Key Rules

- One PR / one logical fix = one changelog entry, even if it contains multiple commits
- Write from the user's perspective: what was broken, what's new, what improved
- Keep descriptions concise but informative
- Chinese version should be natural Chinese, not a literal translation

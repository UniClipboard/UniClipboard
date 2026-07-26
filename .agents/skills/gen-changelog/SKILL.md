---
name: gen-changelog
description: Generate bilingual, user-facing release changelogs from Git history. Use when the user asks to generate a changelog, prepare release notes, or update `docs/changelog` for a release and provides a base tag or commit.
---

# Generate Changelog

Generate the changelog for the current release.

1. Require a base tag or commit. Ask only when it cannot be inferred from the request or current release context.
2. Read `docs/CHANGELOG_TEMPLATE.md` for the required format and rules.
3. Read the current version from `src-tauri/tauri.conf.json`.
4. Inspect commits from the base through `HEAD` with `git log <base>..HEAD --oneline`. Read full commits when the subject is insufficient.
5. Ignore release-cut commits and internal-only changes unless they have user-visible impact.
6. Consolidate entries by pull request and user-visible intent. Never repeat the same pull request in one section.
7. Classify entries according to the template. Every entry must end in ` (#<number>)` taken from the squash commit.
8. Write English to `docs/changelog/{version}.md` and natural Chinese to `docs/changelog/{version}.zh.md`.
9. Include only non-empty sections, use today's date, and describe outcomes rather than implementation.
10. Do not copy the pinned announcement files; the release process prepends them.
11. Validate both files against the template and show the generated content in the result.

If a commit has no pull request number, investigate the associated history before writing. Do not invent one.

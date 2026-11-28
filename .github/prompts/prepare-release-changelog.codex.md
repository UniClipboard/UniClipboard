Generate the end-user changelog for Unic {{PLATFORM}} {{IDENTITY}} ({{CHANNEL}}).

## Release context

- Platform: {{PLATFORM}}
- Product scope: {{PLATFORM_SCOPE}}
- Target version: {{VERSION}} (build {{BUILD_NUMBER}}, identity: {{IDENTITY}})
- Previous published version: {{PREVIOUS_VERSION}} (tag: {{PREVIOUS_TAG}})
- Allowed commit range: {{RANGE_EXPRESSION}} on the current release branch
- Release channel: {{CHANNEL}}
- Prerelease: {{IS_PRERELEASE}}
- Release date: {{RELEASE_DATE}}
- Is Desktop release: {{IS_DESKTOP}}
- Has structured summary notes JSON: {{HAS_NOTES_JSON}}

## Scope rules

- This is an end-user update log, not a developer changelog.
- Only include user-visible changes introduced inside the allowed commit range (`{{RANGE_EXPRESSION}}`) that affect `{{PLATFORM}}`.
- Do not include changes that only affect other platforms unless they directly change `{{PLATFORM}}` user experience.
- For prereleases (`alpha`), summarize only the delta from `{{PREVIOUS_TAG}}` to `HEAD`.
- For `stable` releases, summarize only the allowed commit range (`{{RANGE_EXPRESSION}}`).

## Evidence gathering

- Run `git log --oneline {{RANGE_EXPRESSION}}` to collect candidate commits.
- Inspect touched files or commit diffs (`git show --stat <sha>`) when commit titles are ambiguous.
- Use the skeleton files at `{{CHANGELOG_EN}}` and `{{CHANGELOG_ZH}}` as the structural starting point and replace every placeholder comment.

## Writing rules

- Keep only `### Features` and `### Fixes` (plus `### Breaking Changes` only if there is a verified user-facing breaking change). Omit empty sections.
- Every bullet must describe something a normal user can notice, in one clear sentence of plain language.
- Append the PR number at the end of each bullet when present in commit subjects, e.g. `(#2413)`.
- Merge multiple commits from one PR into a single user-facing bullet per section.
- Do not mention internal tooling, observability, logging, refactors, CI, tests, dependencies, architecture work, or naming cleanup unless directly user-visible.
- Avoid implementation terms such as protocol names, internal module names, algorithm names, framework internals, or state-management details.
- Do not guess. If you cannot verify the user impact from the allowed commit range, leave it out.
- If there are no user-visible changes for `{{PLATFORM}}` in the commit range, include a single concise maintenance bullet under `### Fixes` noting general stability improvements.
- Chinese copy must use natural, idiomatic Simplified Chinese UI terminology rather than literal word-for-word translation.

## Required file outputs

1. Update `{{CHANGELOG_EN}}` in English, keeping the exact first line heading:
   `## {{IDENTITY}} ({{PLATFORM}}) - {{RELEASE_DATE}}`
2. Update `{{CHANGELOG_ZH}}` in Simplified Chinese, keeping the exact first line heading:
   `## {{IDENTITY}} ({{PLATFORM}}) - {{RELEASE_DATE}}`
3. If `{{HAS_NOTES_JSON}}` is `true`, ALSO update `{{NOTES_JSON_PATH}}`:
   - Valid JSON object with exact keys `"en"` and `"zh"`.
   - Each value is a concise, user-friendly plain-text summary (1-4 sentences, no raw HTML comments `<!--`) covering the key user-visible highlights from this release, ending with `{{NOTES_END_EN}}` (English) and `{{NOTES_END_ZH}}` (Chinese).
   - Never include `<!--` anywhere in `{{NOTES_JSON_PATH}}` because the updater feed uses `<!-- zh -->` as an internal separator.
4. Only modify `{{CHANGELOG_EN}}`, `{{CHANGELOG_ZH}}`, and (when `{{HAS_NOTES_JSON}}` is `true`) `{{NOTES_JSON_PATH}}`. Do not edit `release/products.toml` or any other file.
5. Remove all HTML comments (`<!-- ... -->`) before finishing.

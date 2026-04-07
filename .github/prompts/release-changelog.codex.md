Generate the changelog for version {{VERSION}}.

## Release context

- Target version: {{VERSION}} (tag: {{CURRENT_TAG}})
- Previous published version: {{PREVIOUS_VERSION}} (tag: {{PREVIOUS_TAG}})
- Allowed commit range: {{RANGE_EXPRESSION}} on the current release branch
- Release channel: {{CHANNEL}}
- Prerelease: {{IS_PRERELEASE}}

## Scope rules

- This is an end-user update log, not a developer changelog.
- Only include user-visible changes introduced inside the allowed commit range.
- Do not pull entries from older prerelease changelogs unless the same user-visible change was introduced again inside the allowed commit range.
- For prereleases, summarize only the delta from the previous published version to this version.
- For stable releases, still summarize only the allowed commit range unless a human explicitly asks for a broader range.

## Evidence gathering

- Read `docs/CHANGELOG_TEMPLATE.md` for output structure and writing rules.
- Run `git log --oneline {{RANGE_EXPRESSION}}` to collect candidate changes.
- Inspect touched files or diffs when commit titles are ambiguous.
- Use existing drafts at `docs/changelog/{{VERSION}}.md` and `docs/changelog/{{VERSION}}.zh.md` only as rough input; remove anything that is inaccurate or outside the allowed range.

## Writing rules

- Keep only `Features` and `Fixes` unless there is a verified user-facing breaking change.
- Every bullet must describe something a normal user can notice.
- Do not mention internal tooling, observability, logging, refactors, CI, tests, dependencies, architecture work, or naming cleanup.
- Do not guess. If you cannot verify the user impact from the allowed commit range, leave it out.
- Merge related bullets into one line when they describe one user-visible outcome.
- Keep each bullet to one sentence in plain language.
- Avoid implementation terms such as protocol names, internal module names, algorithm names, framework internals, or state-management details.

## Output

- Update `docs/changelog/{{VERSION}}.md` in English.
- Update `docs/changelog/{{VERSION}}.zh.md` in Chinese.
- Follow the exact Markdown structure from `docs/CHANGELOG_TEMPLATE.md`.
- Only modify files under `docs/changelog/`.
- If the existing draft contains items outside the allowed range, delete them.

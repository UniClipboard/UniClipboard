---
phase: quick-6
plan: 01
type: execute
wave: 1
depends_on: []
files_modified:
  - .github/workflows/prepare-release.yml
  - .github/workflows/tag-on-merge.yml
autonomous: true
requirements: [QUICK-6]
must_haves:
  truths:
    - "Running prepare-release workflow with a version input creates a release/vX.Y.Z branch, bumps version, generates changelog, uses Codex to polish docs, and opens a PR to main"
    - "Merging a release/* PR into main triggers tag-on-merge which creates an annotated tag on main HEAD"
    - "The annotated tag push automatically triggers the existing release.yml build+deploy pipeline"
  artifacts:
    - path: ".github/workflows/prepare-release.yml"
      provides: "Workflow: branch creation, version bump, codex polish, PR creation"
    - path: ".github/workflows/tag-on-merge.yml"
      provides: "Workflow: tag creation on release PR merge"
  key_links:
    - from: "prepare-release.yml"
      to: "scripts/bump-version.js"
      via: "node scripts/bump-version.js --to <version>"
      pattern: "bump-version\\.js"
    - from: "tag-on-merge.yml"
      to: ".github/workflows/release.yml"
      via: "git tag push triggers release.yml on: push: tags: v*"
      pattern: "git push origin.*v\\$"
---

<objective>
Create two GitHub Actions workflows that automate the release preparation and tagging process.

Purpose: Replace manual release steps with an automated bot flow: (1) prepare-release creates a branch, bumps version, polishes changelog with Codex, and opens a PR; (2) tag-on-merge creates an annotated tag when the release PR merges, which triggers the existing release.yml pipeline.

Output: Two new workflow files that complement (not replace) the existing release.yml.
</objective>

<execution_context>
@/home/wuy6/.claude/get-shit-done/workflows/execute-plan.md
@/home/wuy6/.claude/get-shit-done/templates/summary.md
</execution_context>

<context>
@.github/workflows/release.yml (existing pipeline triggered by tag push v*)
@scripts/bump-version.js (version bumping: --to <exact-version> or --type + --channel)
@scripts/generate-release-notes.js (release notes generation)
@.github/release-notes/release.md.tmpl (release notes template)

<interfaces>
<!-- Existing release.yml trigger — tag-on-merge must create tags matching this pattern -->
From .github/workflows/release.yml:
```yaml
on:
  push:
    tags:
      - 'v*'
```

<!-- bump-version.js supports exact version targeting -->
From scripts/bump-version.js:
```
Usage: node scripts/bump-version.js --to 0.2.3
Files updated: package.json, src-tauri/tauri.conf.json, src-tauri/Cargo.toml, src-tauri/Cargo.lock
Output: GITHUB_OUTPUT version=<newVersion>
```

<!-- generate-release-notes.js for changelog generation -->
From scripts/generate-release-notes.js:
```
Usage: node scripts/generate-release-notes.js \
  --version "X.Y.Z" --repo "owner/repo" --previous-tag "vA.B.C" \
  --channel "stable" --is-prerelease "false" \
  --artifacts-dir "release-assets" \
  --template ".github/release-notes/release.md.tmpl" \
  --output "release-notes.md"
```
</interfaces>
</context>

<tasks>

<task type="auto">
  <name>Task 1: Create prepare-release.yml workflow</name>
  <files>.github/workflows/prepare-release.yml</files>
  <action>
Create `.github/workflows/prepare-release.yml` with:

**Trigger:** `workflow_dispatch` with inputs:
- `version` (string, required): Exact semver version e.g. "0.3.0" (no "v" prefix)
- `bump` (choice: patch/minor/major, default: patch): Used only if `version` is empty — fallback to --type bump
- `channel` (choice: stable/alpha/beta/rc, default: stable): Release channel
- `base_branch` (string, default: main): Branch to base the release off

**Job: prepare** (runs-on: ubuntu-latest, permissions: contents: write, pull-requests: write)

Steps:
1. **Checkout** with `actions/checkout@v4`, ref: `${{ inputs.base_branch }}`, token: `${{ secrets.REPO_BOT_TOKEN }}` (PAT for cross-workflow trigger), fetch-depth: 0

2. **Setup Node.js** with `actions/setup-node@v4`, node-version: lts/*

3. **Determine version**: If `inputs.version` is non-empty, use it directly. Otherwise run `node scripts/bump-version.js --type ${{ inputs.bump }} --channel ${{ inputs.channel }} --dry-run` and extract version from output. Store in `VERSION` env var and step output.

4. **Create release branch**: `git checkout -b release/v${VERSION}`. Fail if branch already exists on remote.

5. **Bump version**: `node scripts/bump-version.js --to ${VERSION}`. This updates package.json, tauri.conf.json, Cargo.toml, Cargo.lock.

6. **Generate changelog skeleton**: Run `node scripts/generate-release-notes.js` with appropriate args to create `docs/changelog/${VERSION}.md` if it does not already exist. Use a simplified invocation — the script needs --artifacts-dir but we have no artifacts yet, so create an empty temp dir. If the file already exists, skip this step.

7. **Commit version bump**: Configure git user as "github-actions[bot]", add all changed files (`package.json src-tauri/tauri.conf.json src-tauri/Cargo.toml src-tauri/Cargo.lock docs/changelog/`), commit with message `chore: bump version to ${VERSION}`.

8. **Codex polish** (conditional: only if `CODEX_API_KEY` secret is available): Use `openai/codex-action@v1` action with:
   - `responses-api-endpoint`: `${{ secrets.CODEX_API_ENDPOINT || 'https://api.openai.com/v1' }}`
   - `api-key`: `${{ secrets.CODEX_API_KEY }}`
   - `prompt`: "Polish the changelog at docs/changelog/${VERSION}.md — improve wording, fix grammar, ensure consistent formatting. Also create docs/changelog/${VERSION}.zh.md as a Chinese translation if it does not exist. Only modify files under docs/changelog/. Do not change any code files."
   - After codex action, commit any changes: `git add docs/changelog/ && git diff --cached --quiet || git commit -m "docs: polish changelog for v${VERSION}"`

9. **Push branch**: `git push origin release/v${VERSION}`

10. **Create PR**: Use `gh pr create` with:
    - title: `release: v${VERSION}`
    - body: PR body with summary of version bump, link to changelog, and note that merging will trigger tag creation + release pipeline
    - base: `${{ inputs.base_branch }}`
    - head: `release/v${VERSION}`
    - Use `GITHUB_TOKEN: ${{ secrets.REPO_BOT_TOKEN }}` env for gh CLI

11. **Summary**: Write step summary with version, PR URL, and next steps.

Key details:
- Use `REPO_BOT_TOKEN` (not `GITHUB_TOKEN`) for checkout and PR creation so that the merge event triggers tag-on-merge
- The codex step should be wrapped in `if: env.CODEX_API_KEY != ''` with `env: CODEX_API_KEY: ${{ secrets.CODEX_API_KEY }}` so it gracefully skips if not configured
- All git operations use `github-actions[bot]` identity
  </action>
  <verify>
    <automated>cd /home/wuy6/myprojects/UniClipboard.auto-pr-for-release && cat .github/workflows/prepare-release.yml | head -5 && python3 -c "import yaml; yaml.safe_load(open('.github/workflows/prepare-release.yml'))" && echo "YAML valid"</automated>
  </verify>
  <done>prepare-release.yml exists, is valid YAML, has workflow_dispatch trigger with version/bump/channel inputs, uses REPO_BOT_TOKEN, calls bump-version.js, has codex polish step, creates PR via gh CLI</done>
</task>

<task type="auto">
  <name>Task 2: Create tag-on-merge.yml workflow</name>
  <files>.github/workflows/tag-on-merge.yml</files>
  <action>
Create `.github/workflows/tag-on-merge.yml` with:

**Trigger:** `pull_request: types: [closed]` on branches `[main]`

**Job: tag-release** (runs-on: ubuntu-latest, permissions: contents: write)

**Condition (job-level `if`):**
```yaml
if: >-
  github.event.pull_request.merged == true &&
  startsWith(github.event.pull_request.head.ref, 'release/v')
```

Steps:
1. **Extract version** from branch name:
   ```bash
   BRANCH="${{ github.event.pull_request.head.ref }}"
   VERSION="${BRANCH#release/v}"
   echo "version=$VERSION" >> "$GITHUB_OUTPUT"
   ```

2. **Checkout** main at merge commit with `actions/checkout@v4`, ref: main, token: `${{ secrets.REPO_BOT_TOKEN }}`, fetch-depth: 0

3. **Validate version matches**: Read version from `package.json` and compare to extracted version. Fail if mismatch.

4. **Check tag does not exist**: `git tag -l "v${VERSION}"` — fail if tag already exists.

5. **Create annotated tag**:
   ```bash
   git config user.name "github-actions[bot]"
   git config user.email "github-actions[bot]@users.noreply.github.com"
   git tag -a "v${VERSION}" -m "Release v${VERSION}"
   ```

6. **Push tag**: `git push origin "v${VERSION}"` — This triggers the existing release.yml pipeline via `on: push: tags: ['v*']`.

7. **Delete release branch** (cleanup): `git push origin --delete "release/v${VERSION}"` (allow failure with `continue-on-error: true`).

8. **Summary**: Write step summary confirming tag created, linking to the Actions run that will be triggered by the tag push.

Key details:
- Use `REPO_BOT_TOKEN` for checkout and push so the tag push event is not suppressed (GITHUB_TOKEN events don't trigger other workflows)
- The job-level `if` condition ensures this ONLY runs for merged release/* PRs, not other PRs
- No build logic — that is handled entirely by the existing release.yml
- Branch cleanup is best-effort (continue-on-error)
  </action>
  <verify>
    <automated>cd /home/wuy6/myprojects/UniClipboard.auto-pr-for-release && cat .github/workflows/tag-on-merge.yml | head -5 && python3 -c "import yaml; yaml.safe_load(open('.github/workflows/tag-on-merge.yml'))" && echo "YAML valid"</automated>
  </verify>
  <done>tag-on-merge.yml exists, is valid YAML, triggers on pull_request closed, has job-level if for merged release/* PRs, extracts version from branch name, creates annotated tag, pushes with REPO_BOT_TOKEN, deletes release branch</done>
</task>

</tasks>

<verification>
1. Both workflow files are valid YAML
2. prepare-release.yml uses workflow_dispatch with correct inputs
3. tag-on-merge.yml uses pull_request closed trigger with correct if condition
4. Both use REPO_BOT_TOKEN (not GITHUB_TOKEN) for cross-workflow triggering
5. prepare-release.yml calls existing scripts/bump-version.js with --to flag
6. tag-on-merge.yml creates annotated tag matching release.yml's `v*` pattern
7. No duplication of build/release logic from release.yml
</verification>

<success_criteria>
- prepare-release.yml: workflow_dispatch -> creates release branch -> bumps version -> optional codex polish -> opens PR
- tag-on-merge.yml: release PR merge -> creates annotated tag -> triggers existing release.yml
- Both workflows use REPO_BOT_TOKEN for auth
- Existing release.yml is not modified
</success_criteria>

<output>
After completion, create `.planning/quick/6-create-auto-pr-release-bot-with-two-gith/6-SUMMARY.md`
</output>

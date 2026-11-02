---
name: pr-evidence
description: Attach local screenshots or videos to a GitHub pull request body or comment with gh, then verify the published attachment URLs. Use when publishing PR evidence or when create-pr has visual evidence to include.
---

# PR evidence

Use GitHub CLI's native `--attach` support. It uploads media and replaces local Markdown references with hosted URLs in one command. Do not implement a separate uploader, extract browser cookies, commit media to a branch, or create a release to host evidence.

## Select the destination

- For a new PR, let `create-pr` own branch review, commits, push, and PR creation. Supply the evidence section and attachment arguments to that same creation command.
- For an existing PR, add a comment by default. Edit the PR body only when requested.
- A request to write or improve this skill does not authorize a demonstration upload or comment. Publish only the supplied evidence to the requested destination.

## Prepare

1. Resolve the explicit repository, PR, and local files. For an existing PR, read `gh pr view NUMBER --repo OWNER/REPO --json url,headRefOid,body`. For a new PR, record `git rev-parse HEAD` after the final commit.
2. Check `gh version` and the selected command's `--help` for `--attach`. Prefer this capability check over a hard-coded minimum version. If unavailable, report the need to upgrade; do not silently install software or switch to an undocumented upload API.
3. Inspect the screenshots or recording before publication. Use synthetic test content or already approved evidence. Exclude credentials and private clipboard content.
4. Associate evidence with its actual source SHA, platform, test command or reproduction steps, and observed result. State unknown values. Evidence from an older SHA must be labeled; it cannot prove the current head.
5. Confirm the local files exist and are supported media. Current CLI supports PNG, JPG/JPEG, GIF, WebP, SVG, MP4, MOV, and WebM, with at most 50 attachments per invocation. Images/GIFs have a 10 MB limit. Videos have a 10 MB limit on free plans and a 100 MB limit on eligible paid plans. Do not assume the larger limit without checking eligibility.

Uploads require repository write access. GitHub.com and GHE.com are supported; GitHub Enterprise Server is not. Some GitHub App tokens cannot upload. Report permission errors without exposing tokens or substituting a public host.

## Publish

Use an explicit `--repo OWNER/REPO`. Use a body file for substantial Markdown. The example names below are placeholders; resolve them to the actual files and target before execution.

For a new PR, add this section to the normal PR description. Keep paths identical between Markdown and `--attach`. Run from a known directory; absolute local paths avoid body-file path ambiguity.

```markdown
## Evidence

- Source: <full commit SHA>
- Environment: <OS and application build>
- Reproduce: <command or ordered steps>
- Observed: <verified result and acceptance gaps>

### Before

![Before the change](./before.png)

### After

![After the change](./after.png)

### Recording

![](./repro.mp4)
```

Keep the video reference in its own paragraph so GitHub renders a player. Image references retain their Markdown alt text. Files absent from the body are appended in attachment order. Do not add empty evidence sections when no media exists.

Create the PR through `create-pr`, adding only the relevant attachments:

```bash
gh pr create --repo OWNER/REPO --base main --head BRANCH \
  --title 'fix(scope): Describe the behavior change' \
  --body-file /absolute/path/pr-body.md \
  --attach ./before.png --attach ./after.png --attach ./repro.mp4
```

For an existing PR, publish a comment:

```bash
gh pr comment NUMBER --repo OWNER/REPO \
  --body-file /absolute/path/evidence.md \
  --attach ./after.png --attach ./repro.mp4
```

For a quick screenshot comment, use an image description after `#`:

```bash
gh pr comment NUMBER --repo OWNER/REPO \
  --body 'Verified the updated layout on macOS. Source: FULL_SHA.' \
  --attach './after.png#Updated layout on macOS'
```

For a requested append to an existing PR body, `gh pr edit NUMBER --repo OWNER/REPO --attach ./after.png` preserves the existing text. Replacing the body with `--body-file` requires reading and preserving its current content first.

## Verify and recover

1. Record the command's exit status and returned PR or comment URL. A nonzero status can still mean that the PR/comment was published with some attachments. Do not blindly repeat a mutation.
2. Read the published text. For the PR body use `gh pr view NUMBER --repo OWNER/REPO --json url,headRefOid,body`. For a comment, extract its ID from the returned `#issuecomment-ID` URL and use `gh api repos/OWNER/REPO/issues/comments/ID --jq '{html_url,body}'`. If no URL was returned, inspect recent comments using `gh api --paginate repos/OWNER/REPO/issues/NUMBER/comments`; identify the intended comment before retrying.
3. Confirm each expected file has a hosted attachment URL and no unresolved local reference. Report the PR/comment URL and attachment URLs. Reuse those URLs when linking the evidence elsewhere; no second upload is needed. Keep private-repository access restrictions intact.
4. If files are missing, recover only the missing attachments on the existing PR or in a clearly labeled follow-up comment. Never create another PR as an automatic retry. Do not use `--edit-last` unless the last authored comment has been verified as the intended target.
5. Separate published text, URL verification, and visual rendering. Claim inline image/player rendering only after opening the published page and checking it. If that check was not run, say so.

Do not use `gh pr create --dry-run` as an upload test. It cannot be combined with `--attach`, and plain PR dry runs can still push changes.

## Sources

Read these when behavior differs from the installed CLI:

- [Attaching files with GitHub CLI](https://docs.github.com/en/github-cli/github-cli/attaching-files-with-github-cli)
- [PR comment command](https://cli.github.com/manual/gh_pr_comment)
- [PR create command and partial upload behavior](https://cli.github.com/manual/gh_pr_create)
- [Attachment limits and visibility](https://docs.github.com/en/get-started/writing-on-github/working-with-advanced-formatting/attaching-files)
- [CLI attachment implementation guidance](https://github.com/cli/cli/blob/trunk/skills/gh/SKILL.md#attaching-images-and-videos)

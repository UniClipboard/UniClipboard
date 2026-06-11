# Changelog

## Pinned announcement

If `announcement.md` / `announcement.zh.md` exist in this directory, the release
pipeline automatically prepends their content to every release's notes — both
the in-app update dialog (`scripts/assemble-update-manifest.js`) and the GitHub
Release body (`scripts/generate-release-notes.js`). Keep the announcement to a
short blockquote. Write URLs as inline code (backticks): clients older than
0.14.1 render clickable links by navigating the updater webview away from the
app, so links must not be clickable there.

Delete both files once the announcement is no longer needed.

## Releases

- `0.2.0-alpha.5`
  - English: `docs/changelog/0.2.0-alpha.5.md`
  - 中文：`docs/changelog/0.2.0-alpha.5.zh.md`
- `0.2.0-alpha.4`
  - English: `docs/changelog/0.2.0-alpha.4.md`
  - 中文：`docs/changelog/0.2.0-alpha.4.zh.md`

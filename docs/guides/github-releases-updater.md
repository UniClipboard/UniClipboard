# GitHub Releases Updater

This guide documents how UniClipboard publishes Tauri updater artifacts to GitHub Releases and serves `latest.json` for auto-updates.

## Prerequisites

- Tauri updater plugin enabled in `apps/gui/src-tauri/tauri.conf.json`.
- `createUpdaterArtifacts` enabled so `.sig` files are generated.
- A signing keypair generated with `cargo tauri signer generate`.

## Required Tauri Configuration

Update `apps/gui/src-tauri/tauri.conf.json`:

```json
{
  "bundle": {
    "createUpdaterArtifacts": true
  },
  "plugins": {
    "updater": {
      "endpoints": [
        "https://github.com/UniClipboard/UniClipboard/releases/latest/download/latest.json"
      ],
      "pubkey": "<PUBLIC_KEY_CONTENT>"
    }
  }
}
```

We keep the main workflow in `.github/workflows/build.yml` and allow both tag pushes and manual runs.

Notes:

- `pubkey` must be the content of the generated `.key.pub` file (not a path).
- `latest.json` is a static updater manifest. Tauri validates the JSON structure and uses the `.sig` files uploaded with the release.

## Signing Keys

Generate the keypair locally:

```bash
cargo tauri signer generate -w ~/.tauri/uniclipboard.key
```

Store these in CI secrets:

- `TAURI_SIGNING_PRIVATE_KEY` (contents or path to the private key)
- `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` (if you set one)

## Release Workflow (Example)

The `tauri-apps/tauri-action` workflow can build bundles, upload updater artifacts, and generate `latest.json` on GitHub Releases.

```yaml
name: Release

on:
  push:
    tags:
      - 'v*'

jobs:
  release:
    runs-on: ubuntu-22.04
    steps:
      - uses: actions/checkout@v4

      - name: setup node
        uses: actions/setup-node@v4
        with:
          node-version: lts/*

      - name: install Bun
        uses: oven-sh/setup-bun@v1
        with:
          bun-version: latest

      - name: install Rust stable
        uses: dtolnay/rust-toolchain@stable

      - name: install frontend dependencies
        run: bun install

      - name: build and publish
        uses: tauri-apps/tauri-action@v0
        env:
          GITHUB_TOKEN: ${{ secrets.GITHUB_TOKEN }}
          TAURI_SIGNING_PRIVATE_KEY: ${{ secrets.TAURI_SIGNING_PRIVATE_KEY }}
          TAURI_SIGNING_PRIVATE_KEY_PASSWORD: ${{ secrets.TAURI_SIGNING_PRIVATE_KEY_PASSWORD }}
        with:
          tagName: ${{ github.ref_name }}
          releaseName: UniClipboard ${{ github.ref_name }}
          releaseBody: 'See the assets to download.'
          releaseDraft: true
```

## Alpha Manual Builds

Use the manual alpha workflow for prerelease builds:

- Workflow: `.github/workflows/alpha-build.yml`
- Trigger: GitHub Actions → `Build Alpha` (workflow_dispatch)
- Output: GitHub Release marked as `prerelease` + `draft`
- Endpoint: still uses the same `latest.json` in the release assets

This lets us publish alpha drafts without affecting the stable release flow.

References:

- Tauri Updater docs: https://v2.tauri.app/plugin/updater/
- Tauri Action: https://github.com/tauri-apps/tauri-action

## Static JSON Format (latest.json)

When using a static updater file, Tauri expects JSON in this shape (simplified):

```json
{
  "version": "1.2.3",
  "notes": "Release notes",
  "pub_date": "2026-01-01T00:00:00Z",
  "platforms": {
    "darwin-aarch64": {
      "url": "https://github.com/UniClipboard/UniClipboard/releases/download/v1.2.3/UniClipboard.app.tar.gz",
      "signature": "<SIG_CONTENT>"
    }
  }
}
```

Tauri Action generates this file automatically when updater artifacts are enabled.

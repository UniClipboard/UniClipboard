# Built-in content tag acceptance

## Scope

`builtin.classify` annotates new text, images, files and links through the existing processing pipeline. Stable IDs are declared in `plugin/tags.go`. Desktop localizes the four labels, preserves custom labels and protects built-in management operations. See the [pipeline](../architecture/pipeline.md#built-in-content-tags) and [plugin contract](../contracts/plugin-contract.md#built-in-content-classification).

## Repeatable process acceptance

```sh
TMPDIR=/private/tmp UNIC_E2E=1 UNIC_E2E_ARTIFACTS=/private/tmp/unic-builtin-tags-evidence go test ./e2e/cli -run '^TestCLI/builtin-content-tags$' -v -count=1
```

Use a fresh evidence directory for each run. The runner retains `manifest.json`, executable/source/fixture hashes, replay steps and synthetic history metadata. The accepted run recorded `passed: true` at `/private/tmp/unic-builtin-tags-evidence/script-builtin-content-tags/manifest.json`.

The run exercises two real CLI/service processes, pairing, unchanged text/image/file delivery, local and received tags, link Action results and persistence across restart. Negative assertions reject text/link tags on images and files. It does not use the system clipboard or open a native window.

## Desktop acceptance

```sh
make desktop-e2e CASE=i18n
make desktop-e2e CASE=tag-manager
make desktop-e2e CASE=data-binding
```

The i18n matrix passed for all five languages across three skins and two themes. Report: `.build/desktop-e2e/20261112T090357Z-ded10237/index.html`. Its manifest and SHA256SUMS identify the frozen source and software-rendered evidence. The report contains 61 assertions. These are software-view results, not native UI acceptance.

The tag-manager scenario passed with 31 report assertions: `.build/desktop-e2e/20261112T090450Z-8240f6b8/index.html`. It covers protected built-in operations, filtering, visibility and custom tag management.

The real-service data-binding scenario passed: `.build/desktop-e2e/20261112T090612Z-436181ba/index.html`. It verifies empty history with the four built-in tags and existing live mutation/reconnect behavior. No native window was launched.

## Other verification and limits

The 26 existing host integration checks pass with `TMPDIR=/private/tmp`. Configuration and existing built-in plugin checks pass. The ordinary docs checker passes. Architecture `verified` markers are refreshed after the implementation commit so they identify committed source. Unrelated stale-page warnings remain separate from this acceptance.

Existing history is not backfilled. Existing builtin.text email/url hints remain independent. Native UI, system clipboard and full-suite acceptance were not run. Sidebar visibility persists across refresh only within the current desktop session.

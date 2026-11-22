# PR 2285 Desktop E2E repair

Date: 2026-11-22. Starting source: `f9dad9a0ab4a0b0e2d5b0d55c71227a9f2527883`.
PR: [#2285](https://github.com/UniClipboard/UniClipboard/pull/2285).

## Original failures

[Run 44967512014](https://github.com/UniClipboard/UniClipboard/actions/runs/44967512014)
failed exactly two Desktop cases. The original reports and failed-step logs
remain under `.cache/pr2285-ci/first-shard3` and `first-shard4`.

- Shard 3, `tray-icon-settings`: `unchanged style republished image`.
- Shard 4, `token-search`: both themes failed `Up did not wrap to the last candidate`.

The unrelated toolbar-alignment case passed. Other CI jobs passed.

## Causes and changes

The shared device preview now includes a spare computer after the phone.
The keyboard test still expected the phone to be the last source candidate.
It now derives the last source from the preview device list. It retains the
actual Up/Tab interaction, exact selected-source assertion, Down/Enter assertion
and input-focus assertions. All fourteen scenarios still execute in both themes.

The tray scenario checked that the UI and configuration owner contained the
saved style. `Manager.publish` exposes `Current` before invoking `OnChange`.
The image callback can therefore remain pending after those checks pass. The
test then called `SetStyle` before the image owner had processed the change and
misclassified that first publication as a duplicate.

The scenario now tracks successful image-controller processing while draining
the real configuration watcher callbacks. A style choice must reach this
delivery boundary before the test checks duplicate suppression. Existing
timeouts, PNG assertions, restart persistence, CLI/file edits, invalid-value
rejection, repaint and failed-delivery retry assertions remain active.

These changes repair the E2E drivers. Production behavior and public contracts
are unchanged.

## Controlled comparison

The initial local tray case passed, so it did not explain away the hosted
failure. A temporary scheduling probe paused only the second cat-transition
callback for two seconds after `Current` changed and before the image event
was enqueued. The unchanged readiness check then failed with the exact hosted
duplicate-publication assertion.

The same delayed scenario passed after adding the delivery check. The probe
and its counter were removed. The final canonical tray case passed again.
No delay or retry was added to the committed source. First failures and
controlled red/green reports are retained separately.

## Local verification and replay

```sh
make desktop-e2e CASE=token-search
make desktop-e2e CASE=tray-icon-settings
GOTOOLCHAIN=go1.27.3 go run github.com/golangci/golangci-lint/v2/cmd/golangci-lint@v2.15.0 run ./...
go run ./tools/docscheck
```

Both final canonical cases passed and emitted screenshots, assertions and
cleanup verdicts. Pinned lint passed with zero issues. Formatting and diff
checks passed. Documentation checks passed with stale-page warnings.
The compact [receipt](evidence/pr2285-ci-repair/receipt.json) binds the changed
source and report verdicts. Hosted PR CI is the final gate for this repair;
its status must be checked against the pushed head.

All local acceptance uses software views and private service processes.
It does not establish native tray visibility, system clipboard behavior or
user visual approval. The prior Tailcat historical-attribution merge hold
remains separate and unchanged. PR review-state changes remain user-managed.

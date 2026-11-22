# Desktop settings PR validation

- Date: 2026-11-22.
- Base: `c307f39f2` on `v2`.
- Scope: publish the current settings implementation as a non-draft PR.

All five canonical production-process cases passed on the submitted runtime
implementation. Their report checksums were independently verified. Every
manifest reports unchanged frozen source and private cleanup PASS. After the
runs, `gofmt` changed only `live_settings.go` and `settings_appearance.go`.
No runtime behavior changed after these runs. The [receipt](evidence/settings-pr/receipt.json)
records the original manifest hashes, executable hashes, replay commands and
final hashes of those two formatted files. Its [checksums](evidence/settings-pr/SHA256SUMS)
bind the retained small artifact. Original reports remain in ignored `.build/`.

Validation:

- `TMPDIR=/private/tmp go test -race ./...`: PASS, 453 tests in 91 packages.
- Repository-pinned golangci-lint 2.15.0: two formatting findings corrected with
  `gofmt`; the final run passed with zero issues.
- Canonical catalogue freshness and `git diff --check`: PASS.
- Documentation check: PASS with existing freshness warnings. The Desktop
  runtime architecture marker is updated after committing its source inputs.

Historical receipts are preserved without rewriting their source identities.
The earlier five-case receipt and macOS screenshot receipt precede later
settings layout and locale changes. They are historical evidence. This PR
validation reruns process acceptance only. Native rendering of the final layout,
native file dialogs, actual permission grants, setup/Quick Panel theme behavior,
other-platform runtime behavior, system clipboard and cross-application paste
remain unverified by this turn. No GUI, browser or VM was launched.

The [parallel receipt](desktop-settings-parallel-receipt.md) and
[work agreement](desktop-settings-parity.md) retain the delivered behavior,
first failures and remaining contract decisions. Full v1 settings parity is
not claimed.

# Headless acceptance

## v1 migration tag acceptance

Run the production CLI migration and daemon restart scenario with:

```sh
TMPDIR=/private/tmp UNIC_MIGRATION_TAGS_E2E=1 \
UNIC_MIGRATION_TAGS_ARTIFACTS=/private/tmp/unic-migration-tags \
go test ./e2e/headless -run '^TestProductionMigrationTags$' -count=1 -v
```

The scenario uses synthetic data only. It checks custom assignments, canonical
built-ins, unassigned definitions, name collisions, actual history/search filters,
restart persistence, automatic repair, target edits/deletions, unrelated history,
unsupported favorites and damaged assignments. `receipt.json` binds source and
binary identities, fixture checksums and expected/actual mappings. `SHA256SUMS`
checks the receipt. Profiles, binaries and raw process logs are temporary.
Native GUI acceptance is outside this scenario.

The same scenario checks v1 palette/hex colors, default built-in colors,
protection of existing target overrides, automatic color completion and persistence
through the real service configuration API. Missing colors and color failures
must report partial completion.

## macOS daemon clipboard acceptance

```sh
UNIC_MACOS_CLIPBOARD_ARTIFACTS="$(mktemp -d /tmp/unic-macos-clipboard.XXXXXX)" \
go test -tags unic_e2e ./e2e/headless -run '^TestMacOSDaemonClipboard$' -count=1 -v
```

Run this from the active macOS desktop user's session. The test uses a private
named pasteboard and isolated daemon profiles. It checks headless startup,
explicit Desktop opt-in on an existing standalone generation, duplicate adapter
rejection, native content transfer, tracked application and lifecycle handoff. The daemon runs as a secondary
executable in a signed test bundle to verify notification ownership admission.
The receipt binds source and binary hashes. The general pasteboard and native
application focus remain outside this test.

## macOS daemon update acceptance

```sh
UNIC_MACOS_UPDATE_ARTIFACTS="$(mktemp -d /tmp/unic-macos-update.XXXXXX)" \
go test -race ./e2e/headless -run '^TestMacOSDaemonUpdate$' -count=1 -v
```

The runner starts a real daemon and an isolated loopback release feed with an
ephemeral signing key. It checks numbered selection, localized notes, tamper
rejection and the verified transfer across daemon exit. It does not modify the
running application or install an update. The receipt binds source and binary
hashes.

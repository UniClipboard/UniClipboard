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
restart persistence, explicit repair, target edits/deletions, unrelated history,
unsupported favorites and damaged assignments. `receipt.json` binds source and
binary identities, fixture checksums and expected/actual mappings. `SHA256SUMS`
checks the receipt. Profiles, binaries and raw process logs are temporary.
Native GUI acceptance is outside this scenario.

The same scenario checks v1 palette/hex colors, default built-in colors,
protection of existing target overrides, explicit color repair and persistence
through the real service configuration API. Missing colors and color failures
must report partial completion.

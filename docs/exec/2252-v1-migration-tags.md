# Issue 2252: v1 migration tags

## Implementation contract

Read the source in its existing read-only SQLite snapshot. Decode custom definitions and assignments with their existing authenticated AAD. Read built-in membership from the separate profile-scoped search tag table. Never infer built-in identity from a custom display name. Map link, image, directory and code to canonical v2 identifiers. Favorites remain outside migration and must be reported when present.

Normal migration never changes mapped target records. It diagnoses missing tags after reopening the target. An explicit `--repair-tags` may add missing source tags only to mapped records with revision 1. Preserve all target tags. Preserve edited and deleted records. Use the public CAS update API. Repeated repair must make no further changes.

## Failure inventory

- A missing custom definition or malformed/authentication-failed assignment must not silently become an empty tag set.
- Missing optional search tables are supported older formats, not empty custom definitions.
- Built-in membership must use the unlocked profile, not another profile's rows.
- A custom Links tag must remain distinct from builtin:link.
- Multiple tags, unassigned definitions and repeated imports must remain stable.
- Unknown custom reserved namespace names must fail explicitly.
- Unsupported favorites must prevent a complete report.
- Recovery must not overwrite edits, resurrect deletions, duplicate records or change content.
- A crash after a CAS update must be safe to rerun.
- Reports and artifacts must contain synthetic data only.

## Acceptance command

```sh
TMPDIR=/private/tmp UNIC_MIGRATION_TAGS_E2E=1 \
UNIC_MIGRATION_TAGS_ARTIFACTS=/private/tmp/unic-migration-tags \
go test ./e2e/headless -run '^TestProductionMigrationTags$' -count=1 -v
```

The runner builds production CLI/service binaries with race instrumentation. It imports a frozen Rust fixture through `unic migrate`, reopens through public Engine APIs, and checks real service history, tag and search APIs after restart. It retains source identities, binary hashes, fixture hashes, expected/actual mappings and a checksummed JSON receipt. Temporary profiles, binaries and process logs are removed.

Native desktop visual acceptance and personal-profile diagnosis are NOT_RUN.

## Verification result

Baseline: `bd89ff5a12885079e4259c3a72c90ab5e8b420ca`.
The initial production process run retained custom tags `Links` and `标签`,
but lost the separate built-in `code` membership. This establishes a missing
built-in import path. It does not establish why a personal profile reportedly
lost custom tags; no personal data was read.

The final production process run passed. The synthetic source includes an
HTTP URL, rich text, an image, an encrypted directory manifest, multiple custom
assignments, an unassigned definition, a custom Links name and a different
profile's misleading membership. All four supported reserved IDs map correctly.
History and search filters return the expected record after two service starts.
The dry run with repair enabled creates no target.

Recovery diagnosis reported three tag mismatches. Explicit recovery repaired
two untouched records and preserved one edited record. A deleted mapped record
remained deleted. An unrelated target record remained unchanged. A second repair
changed no records. Remaining edits/deletions correctly keep the result partial.
Unsupported favorites, damaged assignments and orphaned assignments each produce
partial reports. Source checksums remain unchanged during each migration.

- Existing regression suites: `TMPDIR=/private/tmp go test -race ./internal/v1migration ./app/service/cmd/unic ./engine ./plugin`: 207 tests passed.
- `go vet ./internal/v1migration ./app/service/cmd/unic ./e2e/headless`: passed.
- Desktop package compilation with `go test ./app/desktop/internal/desktopui -run '^$'`: passed; no GUI test executed.
- `go run ./tools/docscheck`: passed. Six other architecture pages retain stale-page warnings.
- After implementation commit `81d9764`, `go run ./tools/docscheck -bump` updated `pipeline.md` and `overview.md` to the verified implementation commit.
- `git diff --check`: passed.

The [JSON receipt](evidence/2252-v1-migration-tags/receipt.json) contains the
baseline commit plus per-file checksums for the exercised working-tree source,
binary identities, fixture hashes, reports and expected/actual mappings.
The [checksum manifest](evidence/2252-v1-migration-tags/SHA256SUMS) verifies it:

```sh
cd docs/exec/evidence/2252-v1-migration-tags
shasum -a 256 -c SHA256SUMS
```

Temporary profiles, binaries and process logs were removed. Native UI and
personal-profile acceptance remain NOT_RUN. The implementation and receipt are
committed together. The normal migration command diagnoses existing imports; only an
explicit `--repair-tags` modifies eligible mapped records.

## PR 2257 CI lint follow-up

The hosted Lint job on `0a487e277` rejected two unchecked deferred close
results in the migration process scenario. The pinned `golangci-lint v2.15.0`
reproduced both failures locally. The scenario now checks Engine and content
reader close results and fails if either close fails.

After the fix, the pinned full lint run reported `0 issues`. The CI formatter
check produced no diff. The production migration process scenario passed again.
The attached receipt and checksum manifest were regenerated from this source.
No production runtime behavior changed.

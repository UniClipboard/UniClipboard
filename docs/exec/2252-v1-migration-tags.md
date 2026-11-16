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

## Color preservation implementation contract

Read `history-tags/layout.v1.json` with a 1 MiB bound. Version 1 contains
`sidebar` and `colors`. Resolve custom IDs through authenticated definitions;
resolve built-in IDs through the canonical migration mapping. Preserve palette
colors and normalize six-digit hexadecimal colors to lowercase. A missing file
uses v1 built-in defaults, including purple Code. Ignore stale custom IDs with
no surviving definition. Do not infer built-in identity from display names.

Use the existing target `desktop.tag_colors` setting and atomic configuration
CAS. The migration's exclusive target lock covers the color write. Preserve
all existing overrides and unrelated settings. A fresh source import fills
missing overrides; a mapped source requires explicit `--repair-tags` to fill
missing colors. Existing target overrides win. Verify persisted colors after
reopening configuration. Keep presentation metadata outside Engine storage.
Desktop must project saved colors for both built-in and custom tags.

### Color failure inventory

Malformed/truncated/oversized layout; unsupported version; invalid color;
ambiguous duplicate-name colors; unknown stale ID; localized-name collision;
missing layout defaults; existing target colors; invalid target config;
interruption/repeat; repair of already-imported profiles; preservation of
unrelated target settings; config CAS conflict; lost overrides on restart;
dry-run writes; source writes; raw personal data in receipts. Layout or
configuration failures must report partial migration without claiming color
success. Native visual acceptance remains outside this process scenario.

### Color acceptance result

The initial production migration scenario reproduced the missing target color
configuration. The corrected scenario passed with palette and uppercase hex
source colors, an unused colored definition, a stale layout ID and a custom
Links name distinct from the built-in Link ID. Source hex colors normalize to
lowercase. Missing layouts preserve all four v1 built-in defaults.

The scenario verified colors through parsed persisted settings and the real
service configuration API after two service starts. Both initial import into
an existing target and later repair preserved explicit target overrides and
unrelated settings. A normal rerun of an older import diagnosed six missing
colors without adding them. Explicit repair added six and preserved one.
Repeated repair added none. Malformed, unsupported-version, oversized and
invalid-color layouts reported partial completion. Invalid target configuration
remained unchanged and produced a color failure. Source checksums remained
unchanged. The refreshed JSON receipt contains expected colors, actual hashed
configuration entries and color counts alongside the tag acceptance evidence.

Desktop projection now reads saved built-in overrides as well as custom-tag
overrides. Code defaults to purple. Local native visual acceptance remains
NOT_RUN; the desktop package is compiled without opening an application.

The canonical `data-binding` case also checks built-in color overrides in the
production Desktop model, refresh reconciliation and a newly opened model.
Its later settings-menu checks use the software tester, not a native window.
The receipt identifies this software rendering explicitly. The first run passed
the color assertions but exposed a menu test race: the first frame can start a
background read and disable the trigger. The scenario now waits for the rendered
idle state and uses Reduce Motion to keep the sheet stationary before pressing
the menu. Animation behavior remains covered by its dedicated scenarios. Existing tag-manager count assertions now
include the sixth built-in Code tag.

The setup result also displays color failures and missing-color counts. The
new labels are translated into every supported desktop language.

Validation for the color follow-up:

- `TMPDIR=/private/tmp go test -race ./internal/v1migration ./app/service/cmd/unic ./internal/localconfig ./engine`: passed.
- `go vet ./internal/v1migration ./e2e/headless ./app/service/cmd/unic`: passed.
- Pinned `golangci-lint v2.15.0` full lint: `0 issues`; formatter check: no diff.
- Production migration color E2E: passed with a refreshed checksummed receipt.
- Canonical `make desktop-e2e CASE=data-binding`: passed after menu synchronization.
- Canonical `make desktop-e2e CASE=tag-manager`: passed with the six built-in tags.
- Desktop package compilation and translation/setup package checks: passed.

These results establish process persistence, production model projection and
software UI behavior. They do not establish native-window appearance or system
clipboard behavior. Retain only the small migration and binding receipts; owned
source snapshots, binaries, logs and software screenshots are disposable.

The [Desktop color binding receipt](evidence/2252-v1-migration-tags/desktop-color-binding.json)
records successful production model color projection, reconciliation and reopen.
It reports software rendering for the later settings-menu checks and records
that no native window or system clipboard was used. Both JSON receipts are
covered by the checksum manifest.

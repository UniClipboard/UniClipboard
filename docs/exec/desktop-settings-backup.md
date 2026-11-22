# Portable Desktop configuration backup

## Portable field allowlist (before implementation)

Only these typed preferences are portable:

- Desktop: `language`, `color_mode`, `tray_icon`, `update_channel`,
  `system_window_frame`, `mode`, `start_in_background`.
- Logging: `level`.
- Telemetry: `enabled`; `logs.enabled`, `logs.level`; `metrics.enabled`,
  `traces.enabled`, `analytics.enabled`.
- Sync: `enabled`, `automatic`, `on_restore`, `files`, `directories.automatic`.
- History: `auto_cleanup`, `retention_days`, `max_items`, `max_bytes`.
- File cache: `max_bytes`.
- Limits: `max_file_bytes`, `max_total_bytes`.

Exclude the device name/identity, profile and inbox paths, rendezvous and
telemetry endpoints/tokens, login registration, OS shortcut bindings, tag IDs and
colors, every plugin setting/private directory, credentials and private keys.
The mygo updater preference store is not included. Exports are preferences, not
complete profile backups or complete Desktop state.

## Failure inventory (before implementation)

- Export leaks an excluded value or copies private profile files.
- Export replaces an existing user file or follows a destination symlink.
- Oversized, malformed, unknown-field, missing-field, unsupported-version or
  trailing-document input changes live configuration.
- Imported values bypass the existing Config validation or platform bounds.
- The preview changes configuration before explicit confirmation.
- Cancellation writes a preview anyway or late dialog completion overwrites a
  newer preview.
- Confirmation uses a fresh revision with an old snapshot and overwrites an
  external edit.
- Import resets excluded local values, device identity, migration admission or
  plugin state.
- Apply failure is reported as successful runtime application.
- Native file dialogs block the shared service job queue.

## Planned acceptance

A canonical production-process case exports only the allowlisted preferences,
previews a valid import without a write, confirms it through the existing service
configuration path, verifies excluded local state, rejects invalid documents,
rejects stale confirmation after an external edit, and restarts the service to
check persistence. Host file dialogs and visual/native acceptance remain separate.

## Implemented format and flow

The exported file is a JSON document with `format=unic-portable-preferences`,
`version=1`, and a `preferences` object containing all 26 allowlisted fields.
There are no optional preference fields. Input is bounded to 256 KiB and must
contain exactly one document. Unknown fields, missing fields, null preferences,
unsupported format/version and invalid typed values are rejected before a
preview is produced. The merged configuration must pass the existing Config
validation. No server route, Engine API or root configuration field was added.

The import preview shows user-facing setting labels and old/new values. Reading
or cancelling a preview does not change configuration. Confirmation is bound to
the preview's original revision. A changed revision requires a new preview;
service-side revision checks also reject an external disk edit that the GUI has
not observed. The existing save path reports saved-but-not-applied state and
restart requirements instead of claiming immediate runtime success.

Native file panels run outside the shared service worker. Export creates a new
0600 file with exclusive creation and never replaces an existing file or link.
Failure cleanup removes only the newly created inode when its identity still
matches. Import accepts a bounded regular file and rechecks its identity after
opening. These native host callbacks are present but were not invoked by the
process scenario; their native acceptance remains NOT_RUN.

## Validation

The before-implementation compile probe failed on the deliberately absent backup
methods and state. Its retained log is `.cache/settings-backup/first-red.log`.
The first canonical production-process run passed:
`.build/desktop-e2e/20261122T073255Z-9bfd6143/index.html`.
The manifest reports unchanged frozen source and private cleanup PASS.

The scenario verifies explicit private migration initialization, production
Desktop/service config operations, the export allowlist, no-write preview and
cancellation, malformed/oversize/unknown-version/invalid-value/unknown-field/
trailing-JSON rejection, confirmed import, excluded local-state preservation,
stale-revision refusal and restart persistence. Native dialogs, host file
callbacks, rendered preview/confirmation and other-platform native acceptance
remain NOT_RUN. No browser, application window, updater or VM was launched.

The [small receipt](evidence/settings-backup/receipt.json) and
[checksums](evidence/settings-backup/SHA256SUMS) retain run/source identity,
program hashes, scenario checks and native limits. Later shared integration
changes require their own integrated evidence.

Reproduce from the repository root:

```sh
make desktop-e2e CASE=settings-backup
```

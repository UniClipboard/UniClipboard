# 0029 Execution plan: Extensible sync backend integration

- Decision: [0029](../decisions/proposed/0029-extensible-sync-backend-integration.md).
- Contract: [Sync integration contract](../contracts/sync-integration-contract.md).
- Status: Stage 1 slice 1a not started. The independent plugin has a text and image process slice. Generic management, file streaming, and forwarding remain open.

## Findings from the code survey

- Plugin implementations and configured instances have distinct IDs. `plugin.Spec.SettingsSHA256` identifies the published settings schema. The generalized integration profile remains planned.
- `TransportKind` plus `ExternalChannels` must become one profile discriminator (contract section 2). This changes SDK, RPC schema and vectors together.
- `plugin/settings` already parses, validates and digests schemas. Reuse it. Do not add a second schema path.
- The local service exposes installed plugins and their runtime status through `GET /v1/plugins`. The proposed integration view will add effective sync capabilities and target types.

## Stages

1. [ ] Definitions, instances, schemas and capability views through SDK, RPC and local service.
   - [ ] 1a. Read-only Engine snapshot: definition, instance lifecycle, effective capabilities, schema digest. Local route `GET /v1/integrations`. CLI `unic integrations`. One E2E test with real processes.
   - [ ] 1b. Extend the existing revision-checked settings API to expose separate saved and active integration states. Reuse existing secret presence flags.
   - [ ] 1c. Profile discriminator replaces `TransportKind` plus `ExternalChannels` (SDK, RPC, vectors).
2. [ ] Management operations (form, display, confirm, wait) and one renderer; tailcat pairing and SyncClipboard client creation.
3. [ ] Streaming channel reception and `unic.syncclipboard` server.
4. [ ] Explicit one-hop forwarding and gateway topology acceptance.
5. [ ] Client compatibility matrix.

## Failure cases for slice 1a (write tests first)

- Disabled third-party plugin: snapshot must not start the process.
- Unsupported profile version or inconsistent declaration: instance is `unavailable` with a stable reason.
- Schema digest mismatch at handshake: instance is `unavailable`.
- Snapshot of a restarted or crashed process plugin: lifecycle follows the real state.
- Public identifiers contain no path, URL or credential.
- Two configured instances of one implementation have isolated snapshots and effective capabilities.

## Acceptance

Follow contract section 8. Each stage ends with a sanitized, repeatable receipt. Move facts to architecture pages and archive the decision only after stage 5.

## Independent SyncClipboard process acceptance

The first process scenario fails if the installed executable cannot start, if
an unauthenticated or paused client can read a publication, if one client's
credentials work for another channel, or if global sync disable leaves the
publication accessible. It also fails if inbound text does not reach Engine
history, if inbound text overwrites another channel's publication, if invalid
or unsupported documents are accepted, or if the plugin survives daemon
shutdown. Record process binary hashes and assertions without content or
credentials.

Run the installed-process acceptance on macOS or Linux.

```sh
UNIC_E2E=1 UNIC_E2E_SYNCCLIPBOARD=/path/to/unic-plugin-syncclipboard \
  UNIC_E2E_ARTIFACTS=/path/to/receipts \
  go test ./e2e/cli -run '^TestSyncClipboardExternalProcess$' -count=1 -v
```

The [initial acceptance receipt](evidence/syncclipboard/receipt.json) records the first tested commits. The [main integration receipt](evidence/syncclipboard/main-integration.json) records the official marketplace install and the process regression run.
The test writes `syncclipboard-external.json` with executable SHA-256 values
and passed assertions. It uses an isolated disk profile, the normal plugin
installer with a loopback GitHub fixture, a real plugin child, and real
SyncClipboard HTTP requests. Native or third-party clients require separate
acceptance.

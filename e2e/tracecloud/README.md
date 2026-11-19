# Production trace acceptance driver

The [failure inventory](FAILURES.md) precedes this driver. It uses the production
`unic` and `unicd`, explicit empty-profile initialization and the public CLI/API.
It never enables sync, accesses the system clipboard or starts a GUI.

Run local TLS acceptance first:

```sh
GOCACHE="$PWD/.cache/go-build" TMPDIR=/private/tmp \
  go run ./e2e/tracecloud --local --artifact .cache/tracecloud/local
```

The runner builds race-enabled production programs without test tags. It sets
only build-time ingestion metadata, an isolated service version and a test-only
100% root sampling ratio. Four CLI commands use three unique idempotency keys.
The local receiver requires exactly three submission roots and three commit
children, validates their relationship and rejects privacy canaries. Other
remote signals stay disabled. The normal five-second trace batch period remains.

The driver removes its programs, discovery home, profile and database after it
reaps the service. The artifact contains only sanitized configuration, source
and program hashes, decoded local spans, exporter state, assertions and cleanup.
Verify it with `shasum -a 256 -c SHA256SUMS` in the artifact directory.
The source manifest must remain unchanged throughout a run. Repeat after any
implementation change.

## Hosted replay

Hosted mode accepts only the existing authorized UniClipboard US Cloud project
722858. Provide `UNIC_POSTHOG_HOST`, `UNIC_POSTHOG_PROJECT_ID` and its ingestion-only
`UNIC_POSTHOG_PROJECT_TOKEN` through a pre-existing controlled private environment.
Do not embed credential values in tool arguments, shell command text or artifacts.
The driver refuses missing metadata and does not create resources or change policy.

```sh
GOCACHE="$PWD/.cache/go-build" TMPDIR=/private/tmp \
  go run ./e2e/tracecloud --artifact .cache/tracecloud/cloud
```

The hosted driver records local upload/exporter results only. Confirm Cloud
arrival separately with the authenticated PostHog connector. Select and verify
project 722858 before querying. Discover the service/resource keys, then query
the receipt's exact synthetic `service.version` and UTC interval. Require three
roots, three commit children and one matching parent per child. Retain only
sanitized aggregates and synthetic structure. Restore the connector's original
project selection afterward. HTTP success is insufficient evidence.

This task's hosted execution was blocked by credential-injection approval. See
[the hosted receipt](../../docs/exec/observability-traces-hosted.md). The target
and trace capability are available; no upload or read-back is claimed.

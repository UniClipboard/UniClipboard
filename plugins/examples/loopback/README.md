# Loopback acceptance backend

This external Go `sync_backend` fixture uses the public plugin SDK and the Engine-owned identity handshake. It originates from `engine/peere2e_test.go`.

It listens only on random IPv4 loopback ports. Its transport is unencrypted and exists only for isolated acceptance. Do not distribute it as a production network backend. It does not implement WebDAV, S3 or a third-party login protocol.

Build with `go build ./plugins/examples/loopback/cmd/plugin`. Replay the production CLI/service workflow with `UNIC_E2E=1 go test ./e2e/cli -run '^TestSyncPluginWorkflow$' -count=1 -v`.

The Schema includes a public `marker` and a synthetic write-only `credential` for configuration-isolation acceptance. Each process records only its public marker in its instance directory. The fixture retains a loopback endpoint across process restart. It closes sessions according to each session's verified authority revision. The private `fail-send` and one-shot `crash-send` files inject isolated transfer failures; they are acceptance controls, not product settings. No failed content is retried automatically.

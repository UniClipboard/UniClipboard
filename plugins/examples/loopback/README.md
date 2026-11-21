# Loopback acceptance backend

This external Go `sync_backend` fixture uses the public plugin SDK and the Engine-owned identity handshake. It originates from `engine/peere2e_test.go`.

It listens only on random IPv4 loopback ports. Its transport is unencrypted and exists only for isolated acceptance. Do not distribute it as a production network backend. It does not implement WebDAV, S3 or a third-party login protocol.

Build with `go build ./plugins/examples/loopback/cmd/plugin`. Replay the production CLI/service workflow with `UNIC_E2E=1 go test ./e2e/cli -run '^TestSyncPluginWorkflow$' -count=1 -v`.

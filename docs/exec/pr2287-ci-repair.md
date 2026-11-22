# PR 2287 CI repair

## Failure and contract

The `Test and build` job in [run 44986452348](https://github.com/UniClipboard/UniClipboard/actions/runs/44986452348/job/137097966203) failed at `engine_test.go:519` on source `195e0edc332ca5a055a4a69d85fd62bc81237d6d`.
`TestPairByTicketNeedsApprovalOnBothSides` required an empty device list immediately after unpairing. The observed device was pending at revision 4 with no bindings.

The [identity architecture](../architecture/identity-and-trust.md) permits a verified handshake to return a revoked device to pending. The [device contract](../contracts/device-contract.md#5-权限与撤销) requires revocation of trust and path authorization, followed by new approval. An empty list is not stable while the other device is running.

## Reproduction and change

The unchanged test passed 30 local race-enabled runs. The original CI failure remains the first failure evidence; those passes do not explain away the failure.
Adding an explicit fresh ticket handshake after unpairing made the old empty-list assertion fail deterministically: the same device returned at revision 4, with pending trust and a pending binding.
This controls the return-to-pending state; it does not reproduce the exact scheduling of the original background handshake.

The existing test now checks the authorization invariant immediately after unpairing and after the explicit handshake. A returned device must have pending trust, a newer revision, and only pending bindings. Both connections must close, and the fresh handshake must not restore a connection without approval. Production behavior is unchanged.

## Repeatable validation

```sh
TMPDIR=/private/tmp go test -race ./engine -run '^TestPairByTicketNeedsApprovalOnBothSides$' -count=30 -timeout=3m
TMPDIR=/private/tmp UNIC_E2E=1 UNIC_E2E_ARTIFACTS=/tmp/pr2287-e2e go test ./e2e/cli -run '^TestSyncPluginWorkflow/singleton$' -count=1 -v -timeout=4m
(cd /tmp/pr2287-e2e && shasum -a 256 -c SHA256SUMS)
```

`TMPDIR=/private/tmp go test -race ./...` passed. `go run ./tools/docscheck` passed with six existing stale-page warnings. `git diff --check` passed. The production `unic`, `unicd` and `unic-desktop` builds passed. The corrected test passed all 30 runs. The existing real CLI/service/external-plugin E2E passed in 16.31 seconds, excluding binary construction. It checked bidirectional text, PNG and files, policy enforcement, revocation, restart without restored trust, backend disablement and child cleanup. It uses private loopback transport and no native UI or system clipboard.

The local JSON receipt is `/tmp/pr2287-e2e/sync-plugin-workflow-singleton.json`. Its SHA-256 is `0f22c154b5c4fac19015a2775d321a6459f53653461e7e33eda3ebe0faf1c80e`. It contains program hashes and passed steps. To regenerate `SHA256SUMS` after another run, use `shasum -a 256 sync-plugin-workflow-singleton.json > SHA256SUMS` in the artifact directory. First-failure and local run logs remain temporary files under `/tmp/pr2287-*`.
